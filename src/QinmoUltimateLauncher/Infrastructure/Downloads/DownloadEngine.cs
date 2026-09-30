using System;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Security.Cryptography;
using System.Threading;
using System.Threading.Tasks;
using Qul.Application.Ports;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Domain.Security;
using Qul.Infrastructure.Diagnostics;

namespace Qul.Infrastructure.Downloads;

public sealed class DownloadOptions
{
    public int MaxConcurrency { get; set; } = 4;

    public int MaxAttempts { get; set; } = 3;

    /// <summary>退避基数。测试把它设为 0 以便快速收敛。</summary>
    public TimeSpan BaseRetryDelay { get; set; } = TimeSpan.FromMilliseconds(400);

    /// <summary>true = 跟随系统代理；false 时按 <see cref="ProxyAddress"/> 决定显式代理或直连。</summary>
    public bool UseSystemProxy { get; set; } = true;

    public string? ProxyAddress { get; set; }

    /// <summary>磁盘预检的安全余量。</summary>
    public long MinimumFreeBytes { get; set; } = 256L * 1024 * 1024;

    public TimeSpan RequestTimeout { get; set; } = TimeSpan.FromSeconds(60);

    /// <summary>
    /// 下载前是否先探测各源的实际速度。
    /// 只按"失败"换源是不够的——一个慢但能通的源永远不会触发切换，
    /// 几千个文件就那么磨完。探测用计划里最小的一个条目，代价可忽略。
    /// </summary>
    public bool ProbeSourceSpeed { get; set; } = true;

    /// <summary>
    /// 多大的文件才值得分段。**取 1 MB**——由本机 A/B 实测确定（见 `docs/性能实测记录.md`）。
    ///
    /// 低于它分段只是徒增请求开销；而实测里 1.7.10 的资源对象
    /// **39 个大文件就占了 89% 的字节**，尾巴全耗在它们身上。
    /// </summary>
    public long SegmentThresholdBytes { get; set; } = 1024L * 1024;

    /// <summary>
    /// 单个文件最多分几段。
    ///
    /// 段数按体积算（约每 <see cref="SegmentThresholdBytes"/> 一段），这里是上限。
    /// 取 8 而不是 4：1.7.10 最大的资源对象是 9.68 MB 的音乐文件，
    /// 4 段意味着每段 2.4 MB——一段卡住整份就卡住；8 段把它压到 1.2 MB。
    /// 26.3 的客户端 jar 有 41 MB，差别更大。
    ///
    /// 总并发由共享闸门兜住，所以放宽单文件段数不会压垮服务器。
    /// </summary>
    public int MaxSegmentsPerFile { get; set; } = 8;

    /// <summary>是否启用分段。</summary>
    public bool EnableSegmentedDownload { get; set; } = true;
}

/// <summary>
/// 下载引擎。负责"最终正确落盘"这件事的全部语义：
/// 命中缓存不发请求、损坏即重下、中途断开可续传、失败可重试、单文件失败不牵连整体。
///
/// 铁律：**校验基准只来自 <see cref="DownloadItem.Sha1"/>**，也就是只来自官方元数据。
/// 引擎不向任何来源索取哈希，因此"信任镜像提供的哈希"在结构上无处发生。
/// </summary>
public sealed class DownloadEngine
{
    private const int CopyBufferSize = 81920;

    /// <summary>
    /// 某个源在**同一类条目**上缺件多少次之后，本轮就不再优先用它。
    ///
    /// 取 3：一次缺件可能只是那个文件确实不在，三次说明这个源在这类条目上不全。
    /// 实测背景：镜像对资源对象快得多，却缺很多库文件；
    /// 不记这件事的话，每个库都要先失败一次再换源。
    /// </summary>
    private const int SourceFailureThreshold = 3;

    private readonly IHttpTransport _transport;
    private readonly SessionLog _log;

    public DownloadEngine(IHttpTransport transport, SessionLog? log = null)
    {
        _transport = transport ?? throw new ArgumentNullException(nameof(transport));
        _log = log ?? SessionLog.Null;
    }

    public DownloadReport EnsureAll(
        DownloadPlan plan,
        string cacheRoot,
        DownloadOptions? options = null,
        IProgress<DownloadProgress>? progress = null,
        CancellationToken cancellationToken = default)
    {
        if (plan == null)
        {
            throw new ArgumentNullException(nameof(plan));
        }

        if (string.IsNullOrWhiteSpace(cacheRoot))
        {
            throw new ArgumentException("cache root is required", nameof(cacheRoot));
        }

        DownloadOptions effective = options ?? new DownloadOptions();

        // 先给每个条目一个默认失败结论：任何提前退出（取消、预检失败）都不会留下空洞。
        DownloadItemReport[] reports = new DownloadItemReport[plan.Items.Count];
        for (int i = 0; i < reports.Length; i++)
        {
            reports[i] = new DownloadItemReport(plan.Items[i], DownloadItemState.Failed, ErrorCode.DlFailed, 0, 0);
        }

        bool preferFallbackFirst = ProbeSourceOrder(plan, effective, cancellationToken);

        // 本次运行的来源健康度：某个源在某一类条目上反复缺件，就跳过它。
        SourceHealth health = new SourceHealth(SourceFailureThreshold);

        ErrorCode? spaceProblem = CheckFreeSpace(cacheRoot, plan.KnownTotalBytes, effective.MinimumFreeBytes);
        if (spaceProblem.HasValue)
        {
            _log.Failure("download", spaceProblem.Value);
            for (int i = 0; i < reports.Length; i++)
            {
                reports[i] = new DownloadItemReport(plan.Items[i], DownloadItemState.Failed, spaceProblem, 0, 0);
            }

            return new DownloadReport(reports);
        }

        int completed = 0;
        long bytesTransferred = 0;
        object progressGate = new object();

        // 分段请求共享一道闸门。取 32 而不是跟并发数同值：
        // 单文件最多 8 段，但**只有大文件才分段**——26.3 的 5224 项里只有 87 项 ≥1MB，
        // 所以闸门几乎不会成为瓶颈；而它把最坏情况下的总连接数摁在 96 以内
        // （64 个 worker + 32 个分段），不至于出现"每个 worker 再各开 8 段"。
        using (SemaphoreSlim segmentGate = new SemaphoreSlim(
            Math.Min(32, Math.Max(1, effective.MaxConcurrency))))
        {

        // **固定数量的 worker，而不是每个条目一个任务。**
        //
        // 先前是 `Task.Run` × 条目数，每个任务在里面**阻塞地**等信号量。
        // 1.7.10 有一千多个条目，于是上千个工作项去抢线程池的线程、并阻塞在信号量上；
        // 线程池补线程约每秒一个，有效并发只能从 8 慢慢往上爬。
        //
        // 实测症状：前 12 秒下了 26 MB（接近网络真实能力），之后 84 秒只下了 9 MB；
        // 而且线程数会一路涨向上千。**慢的不是网络，是调度方式。**
        //
        // 改成 MaxConcurrency 个 worker 从共享游标取活：线程数恒定，
        // 并发从第一秒起就是满的。
        int workerCount = Math.Min(
            Math.Max(1, effective.MaxConcurrency),
            Math.Max(1, plan.Items.Count));

        int cursor = -1;
        Task[] workers = new Task[workerCount];

        for (int w = 0; w < workerCount; w++)
        {
            // 刻意不把取消令牌交给 Task.Run：取消由循环条件与 ProcessItem 处理，
            // 否则被取消的任务会让 Task.WaitAll 抛聚合异常，而逐条结论已经记在 reports 里了。
            workers[w] = Task.Run(() =>
            {
                while (!cancellationToken.IsCancellationRequested)
                {
                    int index = Interlocked.Increment(ref cursor);

                    if (index >= plan.Items.Count)
                    {
                        return;
                    }

                    DownloadItemReport report;

                    try
                    {
                        report = ProcessItem(
                            plan.Items[index], cacheRoot, effective, preferFallbackFirst, health, segmentGate, cancellationToken);

                        // **取消请求之后产生的任何失败，都算取消。**
                        //
                        // 取消会让传输层抛出各种形状的异常（WebException / IOException /
                        // ObjectDisposedException），它们与"下载真的坏了"在**类型上无法区分**；
                        // 而且 ProcessItem 内部的失败出口也不抛异常、直接返回 Failed。
                        // 两条路都得在这里归一。
                        //
                        // 这一条是写"取消"用例时发现的：那条用例**间歇失败**——
                        // 取消正好落在分段任务内部时，异常形状就变了。
                        if (cancellationToken.IsCancellationRequested
                            && report.State == DownloadItemState.Failed)
                        {
                            reports[index] = new DownloadItemReport(
                                report.Item, DownloadItemState.Cancelled, null, report.Attempts, report.BytesTransferred);
                            return;
                        }
                    }
                    catch (OperationCanceledException)
                    {
                        // **取消不是失败**：不带错误码，状态单独记，
                        // 否则界面上"你按了取消"和"下载真的坏了"长得一模一样。
                        reports[index] = new DownloadItemReport(
                            plan.Items[index], DownloadItemState.Cancelled, null, 0, 0);
                        return;
                    }

                    catch (LauncherException ex)
                    {
                        // **取消优先于失败判定**：取消期间抛出的异常形状不可预测，
                        // 而用户确实按了取消，就不该看到"下载失败"。
                        if (cancellationToken.IsCancellationRequested)
                        {
                            reports[index] = new DownloadItemReport(
                                plan.Items[index], DownloadItemState.Cancelled, null, 0, 0);
                            return;
                        }

                        // **保留具体的错误码。**
                        // 兜底成 DlFailed 会把"路径越界""磁盘不可写"这类**可操作**的结论
                        // 抹平成笼统的"下载失败"——这个项目已经踩过好几次
                        // "错误码把人引向错误方向"的坑。
                        _log.Failure("download", ex.Code, ex, plan.Items[index].RelativePath);
                        reports[index] = new DownloadItemReport(
                            plan.Items[index], DownloadItemState.Failed, ex.Code, 0, 0);
                        continue;
                    }
                    catch (Exception ex)
                    {
                        if (cancellationToken.IsCancellationRequested)
                        {
                            reports[index] = new DownloadItemReport(
                                plan.Items[index], DownloadItemState.Cancelled, null, 0, 0);
                            return;
                        }

                        // 编排层永不因单个条目抛出：一个坏文件不该让整次安装失败。
                        // 走到这里的都是没被分类的异常，才允许兜底成 DlFailed。
                        _log.Failure("download", ErrorCode.DlFailed, ex);
                        reports[index] = new DownloadItemReport(
                            plan.Items[index], DownloadItemState.Failed, ErrorCode.DlFailed, 0, 0);
                        continue;
                    }

                    reports[index] = report;

                    lock (progressGate)
                    {
                        completed++;
                        bytesTransferred += report.BytesTransferred;

                        // **进度上报失败绝不能拖垮一个已经下好的文件。**
                        // 界面被关掉、输出流被释放、日志在轮转——这些都不该让下载失败。
                        // 先前这里没有任何保护，一个 ObjectDisposedException 就是这么
                        // 逃到编排层的，而那里没有重试，条目一次都没试就被记成失败。
                        try
                        {
                            progress?.Report(new DownloadProgress
                            {
                                FilesCompleted = completed,
                                FilesTotal = plan.Items.Count,
                                BytesTransferred = bytesTransferred,
                                KnownTotalBytes = plan.KnownTotalBytes,
                                CurrentPath = report.Item.RelativePath,
                            });
                        }
                        catch (Exception ex) when (ex is ObjectDisposedException || ex is InvalidOperationException || ex is IOException)
                        {
                        }
                    }
                }
            });
        }

        try
        {
            Task.WaitAll(workers);
        }
        catch (AggregateException)
        {
            // 逐条结论已经记录在 reports 里，聚合异常不再额外上抛。
        }

        }

        return new DownloadReport(reports);
    }

    private DownloadItemReport ProcessItem(
        DownloadItem item,
        string cacheRoot,
        DownloadOptions options,
        bool preferFallbackFirst,
        SourceHealth health,
        SemaphoreSlim segmentGate,
        CancellationToken cancellationToken)
    {
        // **落盘路径必须校验在缓存根之下。**
        //
        // 进入 item.RelativePath 的是**元数据里的字符串**（artifact.path、资源索引 id、
        // logging 文件名……），而元数据并非全部来自官方：本地 cache/meta/version-*.json
        // 优先于网络，接入镜像后主机也会被改写。
        // 任一处给出 ..\..\..\Users\Public\evil.dll，不校验就等于**任意位置写入**——
        // 而且哈希来自同一份元数据，校验拦不住。
        // 项目里解压那条路早有同款防护（ArchiveEntryPolicy 的 zip-slip 判定），下载这边此前没有。
        string destination = SafeDestination(cacheRoot, item.RelativePath);
        // 后缀刻意不是 ".part"：元数据里若有条目相对路径正好以 .part 结尾，
        // 它的**正式文件**就会与另一条目的**临时文件**同名，两个 worker 互相覆盖。
        // 官方元数据目前没有这种路径，属纵深防御。
        string partial = destination + ".qulpart";

        if (!Sha1Hex.TryParse(item.Sha1, out Sha1Hex expected))
        {
            // 元数据没给校验值：拒绝下载。绝不"先下下来再说"。
            _log.Warn("download", "item refused: metadata carries no checksum", ErrorCode.DlChecksumMissing, item.RelativePath);
            return new DownloadItemReport(item, DownloadItemState.RefusedNoChecksum, ErrorCode.DlChecksumMissing, 0, 0);
        }

        if (File.Exists(destination) && Matches(destination, expected))
        {
            // 缓存命中：不产生任何网络请求。
            return new DownloadItemReport(item, DownloadItemState.Present, null, 0, 0);
        }

        ErrorCode? lastError = null;
        long transferred = 0;
        int attempt = 0;

        // 分段只对"够大且知道确切体积"的条目启用。
        // 体积未知就不分段——没有总量就无法划分区间。
        bool segmentOnFirstAttempt = options.EnableSegmentedDownload
                                     && item.Size.HasValue
                                     && item.Size.Value >= options.SegmentThresholdBytes;

        // 源的数量决定"至少要试几次"：一个源说没有，不代表另一个源也没有。
        int sourceCount = 1 + item.FallbackUrls.Count;
        int maxAttempts = Math.Max(Math.Max(1, options.MaxAttempts), sourceCount);

        for (attempt = 1; attempt <= maxAttempts; attempt++)
        {
            cancellationToken.ThrowIfCancellationRequested();

            // 声明在 try 之外：catch 里要按"这次用的是哪个源"记健康度。
            string sourceUrl = UrlForAttempt(item, attempt, preferFallbackFirst, health);

            try
            {
                // 只有第一次尝试走分段：分段失败通常意味着这个源不支持 Range，
                // 重试就该退回单连接，而不是再撞一次。
                if (segmentOnFirstAttempt && attempt == 1)
                {
                    transferred += FetchSegmented(item, sourceUrl, partial, options, segmentGate, cancellationToken);
                }
                else
                {
                    transferred += FetchOnce(item, sourceUrl, destination, partial, options, cancellationToken);
                }

                if (Matches(partial, expected))
                {
                    Publish(partial, destination);
                    return new DownloadItemReport(item, DownloadItemState.Downloaded, null, attempt, transferred);
                }

                // 校验不过：残留与成品都不可信，全部丢弃，下一轮从头来。
                SafeDelete(partial);
                SafeDelete(destination);
                lastError = ErrorCode.DlChecksumMismatch;
                _log.Warn("download", "checksum mismatch, discarded", ErrorCode.DlChecksumMismatch, item.RelativePath);
            }
            catch (LauncherException ex)
            {
                lastError = ex.Code;

                // 只把"这个源没有这个文件"算在来源头上；
                // 超时与连接中断属于传输故障，记上去会误伤好源。
                if (ex.Code == ErrorCode.NetResourceMissing || ex.Code == ErrorCode.NetHttpStatus)
                {
                    health.RecordFailure(sourceUrl, item.Kind);
                }

                if (!IsRetryable(ex.Code))
                {
                    // **一个源的 404 不代表另一个源也没有。**
                    //
                    // 实测踩到过：镜像对一批库返回 404，而官方源上确实有；
                    // 因为 404 被当成终局错误直接 break，一次安装里 19 个条目
                    // 连第二个源都没试就失败了（其中包含 lwjgl 这类必需库）。
                    bool anotherSourceMayHaveIt =
                        ex.Code == ErrorCode.NetResourceMissing && attempt < sourceCount;

                    if (!anotherSourceMayHaveIt)
                    {
                        _log.Failure("download", ex.Code, ex, item.RelativePath);
                        break;
                    }

                    _log.Warn("download", "this source reports missing; trying another", ex.Code, item.RelativePath);
                }
            }
            catch (WebException ex)
            {
                // 连接在传输中途被掐断属于预期内故障，也必须保留 .part 以便续传。
                lastError = ex.Status == WebExceptionStatus.Timeout ? ErrorCode.NetTimeout : ErrorCode.NetUnreachable;
            }
            catch (IOException ex)
            {
                lastError = ErrorCode.DlFailed;
                _log.Warn("download", "io failure during transfer", ErrorCode.DlFailed, ex.GetType().Name);
            }
            catch (ObjectDisposedException)
            {
                // 底层连接或响应被提前释放。它属于可重试的传输故障，
                // 必须留在重试循环里——一旦逃到编排层，那里没有重试，
                // 条目会以"尝试 0 次"永久失败。
                lastError = ErrorCode.DlFailed;
                _log.Warn("download", "connection released early; will retry", ErrorCode.DlFailed);
            }
            catch (UnauthorizedAccessException)
            {
                lastError = ErrorCode.IoDataRootNotWritable;
                break;
            }

            if (attempt < maxAttempts)
            {
                Backoff(attempt, options.BaseRetryDelay, cancellationToken);
            }
        }

        // attempt 在"耗尽重试"时为 MaxAttempts+1，在"立即放弃"时为已尝试的次数，
        // 取 min 才能让两条路径都得到真实的尝试次数。
        return new DownloadItemReport(
            item,
            DownloadItemState.Failed,
            lastError ?? ErrorCode.DlFailed,
            Math.Min(attempt, maxAttempts),
            transferred);
    }

    /// <summary>
    /// 第 N 次尝试该用哪个源。
    ///
    /// **一轮换一个源**：首选失败就换备用，备用也失败再回到首选。
    /// 设计理念上借鉴了"多源回退"这个通行思路（**只借鉴思路，不含任何外部代码**）——官方被限速或镜像抽风时，
    /// 死磕同一个源只会把重试次数浪费在同一个故障上。
    /// </summary>
    private static string UrlForAttempt(
        DownloadItem item, int attempt, bool preferFallbackFirst, SourceHealth health)
    {
        List<string> all = new List<string>(1 + item.FallbackUrls.Count) { item.Url };
        all.AddRange(item.FallbackUrls);

        if (all.Count == 1)
        {
            return all[0];
        }

        // 探测说备用源更快，就从备用源开始；否则从配置的首选开始。
        int start = preferFallbackFirst ? 1 : 0;

        // 先按 start 旋转出一个顺序，剔掉本轮已被判定不可用的源。
        List<string> healthy = new List<string>(all.Count);

        for (int i = 0; i < all.Count; i++)
        {
            string candidate = all[(start + i) % all.Count];

            if (!health.IsUnhealthy(candidate, item.Kind))
            {
                healthy.Add(candidate);
            }
        }

        if (healthy.Count == 0)
        {
            // 全都被判过：宁可再试一次坏源，也不要无源可试。
            return all[(start + attempt - 1) % all.Count];
        }

        return healthy[(attempt - 1) % healthy.Count];
    }

    /// <summary>
    /// 探测各下载源的实际吞吐，判断是否需要把备用源提到首选。
    /// 样本不合适、探测无结论、或只有一个源时一律返回 false——保持配置的行为。
    /// </summary>
    private bool ProbeSourceOrder(DownloadPlan plan, DownloadOptions options, CancellationToken cancellationToken)
    {
        if (!options.ProbeSourceSpeed)
        {
            return false;
        }

        DownloadItem? sample = DownloadSourceProbe.PickSample(plan);

        if (sample == null)
        {
            return false;
        }

        List<string> candidates = new List<string>(1 + sample.FallbackUrls.Count) { sample.Url };
        candidates.AddRange(sample.FallbackUrls);

        IReadOnlyList<string> ranked = new DownloadSourceProbe(_transport, _log)
            .Rank(sample, candidates, cancellationToken);

        return ranked.Count > 0 && !string.Equals(ranked[0], sample.Url, StringComparison.Ordinal);
    }

    /// <summary>
    /// 单次取回。返回本次写入的字节数。
    /// 断点续传的关键：本地 .part 的长度就是 Range 起点；服务端若忽略 Range（回 200），必须从头写而不是追加。
    /// </summary>
    private long FetchOnce(
        DownloadItem item, string url,
        string destination,
        string partial,
        DownloadOptions options,
        CancellationToken cancellationToken)
    {
        EnsureParentDirectory(destination);

        long existing = 0;
        if (File.Exists(partial))
        {
            existing = new FileInfo(partial).Length;
        }

        HttpFetchRequest request = new HttpFetchRequest
        {
            Url = url,
            RangeFrom = existing > 0 ? existing : (long?)null,
            ProxyAddress = ResolveProxy(options),
            Timeout = options.RequestTimeout,
        };

        using (HttpFetchResponse response = _transport.Fetch(request, cancellationToken))
        {
            switch (response.Status)
            {
                case HttpFetchStatus.RangeNotSatisfiable:
                    // **不删残留，正常返回 0 字节，交给调用方去校验它。**
                    //
                    // 先前这里直接删掉残留再抛错，于是每次重试都全量重下。
                    // 而它最常出现的场景恰恰是"下载成功、校验通过、**发布失败**"
                    // （目标文件被游戏或杀软占用）——此时那些字节**是对的**，
                    // 41 MB 的客户端 jar 却每轮都要重下一遍，而且只要占用还在就永远完不成。
                    //
                    // 现在返回 0：调用方会照常校验残留，通过就直接发布，不通过才删掉重来。
                    return 0;

                case HttpFetchStatus.NotFound:
                    throw new LauncherException(ErrorCode.NetResourceMissing, "resource not found");

                case HttpFetchStatus.ServerError:
                    throw new LauncherException(ErrorCode.NetHttpStatus, "server error " + response.StatusCode);

                case HttpFetchStatus.Other:
                    throw new LauncherException(ErrorCode.NetHttpStatus, "unexpected status " + response.StatusCode);
            }

            bool append = response.Status == HttpFetchStatus.PartialContent
                          && existing > 0
                          && response.RangeStart.HasValue
                          && response.RangeStart.Value == existing;

            if (!append)
            {
                existing = 0;
            }

            long written = 0;
            FileMode mode = append ? FileMode.Append : FileMode.Create;

            using (FileStream stream = new FileStream(partial, mode, FileAccess.Write, FileShare.None, CopyBufferSize))
            {
                byte[] buffer = new byte[CopyBufferSize];
                int read;

                while ((read = response.Content.Read(buffer, 0, buffer.Length)) > 0)
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    stream.Write(buffer, 0, read);
                    written += read;
                }

                stream.Flush();
            }

            return written;
        }
    }

    /// <summary>
    /// 大文件的**多连接分段下载**（≥1 MB 才分段，阈值由本机 A/B 实测确定）。设计理念与 PCL2 等启动器一致，**不含其任何代码**。
    ///
    /// 为什么需要它：全程分阶段采样显示，1.7.10 的资源对象里
    /// **39 个 ≥256 KB 的大文件占了 89% 的字节**，而它们各自只在一条连接上下载。
    /// 前 100 秒能跑 830 KB/s，之后大文件越来越少、干活连接数塌到个位数，
    /// 尾巴掉到 22 KB/s——**不是网络慢，是并发度没了。**
    ///
    /// 与既有不变量的关系：分段只负责把字节完整写进 <c>.part</c>，
    /// 之后仍走原来的 SHA-1 校验 + 原子发布，**完整性保证没有打折**。
    /// 分段失败时删掉整个 <c>.part</c> 并回退单连接重试。
    /// </summary>
    private long FetchSegmented(
        DownloadItem item,
        string url,
        string partial,
        DownloadOptions options,
        SemaphoreSlim gate,
        CancellationToken cancellationToken)
    {
        long total = item.Size!.Value;
        long threshold = Math.Max(1, options.SegmentThresholdBytes);

        int count = (int)Math.Min(
            Math.Max(1, options.MaxSegmentsPerFile),
            Math.Max(1, (total + threshold - 1) / threshold));

        EnsureParentDirectory(partial);

        // 预分配整个文件：各段往自己的区间写，互不重叠。
        using (FileStream allocate = new FileStream(partial, FileMode.Create, FileAccess.Write, FileShare.None, 1))
        {
            allocate.SetLength(total);
        }

        long per = (total + count - 1) / count;
        long[] written = new long[count];
        LauncherException? failure = null;
        object failureGate = new object();

        Task[] workers = new Task[count];

        for (int i = 0; i < count; i++)
        {
            int index = i;

            long start = index * per;
            long end = Math.Min(total - 1, start + per - 1);

            if (start > end)
            {
                workers[index] = Task.CompletedTask;
                continue;
            }

            // **先取许可，再起任务。**
            //
            // 先前是"起了任务、再在任务里阻塞等许可"——于是 64 个 worker × 每个最多 8 段
            // 会造出几百个**阻塞在闸门上**的线程池线程，而线程池补线程约每秒一两个。
            // 这正是我先前在编排层修过的那个病（线程数一路涨向上千、有效并发慢慢爬），
            // 结果在分段路径上又犯了一次。
            //
            // 先取许可就把活着的任务数摁在闸门大小以内：
            // 总线程数 ≈ worker 数 + 闸门大小。
            gate.Wait(cancellationToken);

            workers[index] = Task.Run(
                () =>
                {
                    try
                    {
                        written[index] = FetchRange(url, partial, start, end, options, cancellationToken);
                    }
                    catch (OperationCanceledException)
                    {
                        throw;
                    }
                    catch (LauncherException ex)
                    {
                        lock (failureGate)
                        {
                            if (failure == null)
                            {
                                failure = ex;
                            }
                        }
                    }
                    finally
                    {
                        gate.Release();
                    }
                });
        }

        try
        {
            Task.WaitAll(workers);
        }
        catch (AggregateException ex)
        {
            // **取消要原样抛出去，不能吞掉。**
            //
            // 先前这里一律吞掉，于是取消会以"分段总长对不上"的形式浮出来：
            // 残留被删、条目被记成下载失败——而**取消不是失败**，
            // 而且那份残留本来是可以续传的。
            foreach (Exception inner in ex.Flatten().InnerExceptions)
            {
                if (inner is OperationCanceledException)
                {
                    throw new OperationCanceledException(cancellationToken);
                }
            }
        }

        if (failure != null)
        {
            // 分段拼不成一个完整文件，整份残留都不可信。
            SafeDelete(partial);
            throw failure;
        }

        long sum = 0;
        for (int i = 0; i < count; i++)
        {
            sum += written[i];
        }

        if (sum != total)
        {
            SafeDelete(partial);
            throw new LauncherException(
                ErrorCode.DlFailed,
                "segmented download wrote " + sum + " of " + total + " bytes");
        }

        return sum;
    }

    /// <summary>取一个字节区间并写进 <c>.part</c> 的对应偏移。写出的字节数会被核对。</summary>
    private long FetchRange(
        string url,
        string partial,
        long start,
        long end,
        DownloadOptions options,
        CancellationToken cancellationToken)
    {
        HttpFetchRequest request = new HttpFetchRequest
        {
            Url = url,
            RangeFrom = start,
            RangeTo = end,
            ProxyAddress = ResolveProxy(options),
            Timeout = options.RequestTimeout,
        };

        using (HttpFetchResponse response = _transport.Fetch(request, cancellationToken))
        {
            if (response.Status == HttpFetchStatus.NotFound)
            {
                throw new LauncherException(ErrorCode.NetResourceMissing, "resource not found");
            }

            if (response.Status == HttpFetchStatus.ServerError)
            {
                throw new LauncherException(ErrorCode.NetHttpStatus, "server error " + response.StatusCode);
            }

            bool ranged = response.Status == HttpFetchStatus.PartialContent;

            // **服务端忽略 Range 时会回 200 并从 0 开始发。**
            // 首段还能用（截到本段长度即可）；非首段就完全对不上——
            // 必须判定为"不支持分段"，让调用方回退单连接。
            if (!ranged && start > 0)
            {
                throw new LauncherException(ErrorCode.DlFailed, "server ignored the range request");
            }

            if (!ranged && response.Status != HttpFetchStatus.Success)
            {
                throw new LauncherException(ErrorCode.NetHttpStatus, "unexpected status " + response.StatusCode);
            }

            long want = end - start + 1;

            using (FileStream stream = new FileStream(
                partial, FileMode.Open, FileAccess.Write, FileShare.Write, CopyBufferSize))
            {
                stream.Seek(start, SeekOrigin.Begin);

                byte[] buffer = new byte[CopyBufferSize];
                long written = 0;
                int read;

                while (written < want && (read = response.Content.Read(buffer, 0, buffer.Length)) > 0)
                {
                    cancellationToken.ThrowIfCancellationRequested();

                    // **绝不越过本段边界写入**：服务端忽略 Range 时会给整个文件，
                    // 多写的部分会覆盖邻段已经下好的数据。
                    int take = (int)Math.Min(read, want - written);
                    stream.Write(buffer, 0, take);
                    written += take;
                }

                stream.Flush();
                return written;
            }
        }
    }

    /// <summary>null = 跟随系统代理；空串 = 直连；其他 = 显式代理地址。</summary>
    /// <summary>
    /// 把条目的相对路径解析到缓存根之下，并确认它**确实**在根之下。
    ///
    /// 只用 <c>Path.Combine</c> 是不够的：<c>..</c> 段会被解析到根之外，
    /// 而绝对路径还会让 Combine 直接丢掉根。两条都要挡。
    /// </summary>
    private static string SafeDestination(string cacheRoot, string relativePath)
    {
        string root = Path.GetFullPath(cacheRoot);

        string full;
        try
        {
            full = Path.GetFullPath(Path.Combine(root, relativePath));
        }
        catch (Exception ex) when (ex is ArgumentException || ex is NotSupportedException || ex is PathTooLongException)
        {
            throw new LauncherException(ErrorCode.IoPathEscapesRoot, "条目路径无法解析：" + relativePath);
        }

        string prefix = root.EndsWith(Path.DirectorySeparatorChar.ToString(), StringComparison.Ordinal)
            ? root
            : root + Path.DirectorySeparatorChar;

        if (!full.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
        {
            throw new LauncherException(
                ErrorCode.IoPathEscapesRoot, "条目路径越出了缓存根：" + relativePath);
        }

        return full;
    }

    private static string? ResolveProxy(DownloadOptions options)
    {
        if (options.UseSystemProxy)
        {
            return null;
        }

        return options.ProxyAddress ?? string.Empty;
    }

    private static bool IsRetryable(ErrorCode code)
    {
        switch (code)
        {
            case ErrorCode.NetTimeout:
            case ErrorCode.NetUnreachable:
            case ErrorCode.NetHttpStatus:
            case ErrorCode.DlFailed:
            case ErrorCode.DlChecksumMismatch:
                return true;

            default:
                // 资源不存在、缺校验值、代理配置错、证书不通过、磁盘不可写：重试没有意义。
                return false;
        }
    }

    private static bool Matches(string path, Sha1Hex expected)
    {
        try
        {
            using (FileStream stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, CopyBufferSize))
            using (SHA1 sha1 = SHA1.Create())
            {
                byte[] hash = sha1.ComputeHash(stream);
                return Sha1Hex.TryParse(ToHex(hash), out Sha1Hex actual) && actual.Equals(expected);
            }
        }
        catch (IOException)
        {
            return false;
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }
    }

    private static string ToHex(byte[] bytes)
    {
        char[] chars = new char[bytes.Length * 2];
        const string Digits = "0123456789abcdef";

        for (int i = 0; i < bytes.Length; i++)
        {
            chars[i * 2] = Digits[bytes[i] >> 4];
            chars[(i * 2) + 1] = Digits[bytes[i] & 0x0F];
        }

        return new string(chars);
    }

    /// <summary>把 .part 变成正式文件。同卷 File.Move 是重命名，不会留下半个文件。</summary>
    private static void Publish(string partial, string destination)
    {
        EnsureParentDirectory(destination);

        if (File.Exists(destination))
        {
            File.Delete(destination);
        }

        File.Move(partial, destination);
    }

    private static void SafeDelete(string path)
    {
        try
        {
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    private static void EnsureParentDirectory(string path)
    {
        string? directory = Path.GetDirectoryName(path);
        if (!string.IsNullOrEmpty(directory))
        {
            Directory.CreateDirectory(directory!);
        }
    }

    /// <summary>指数退避 + 确定性抖动，避免多线程重试同时打过去。可被取消打断。</summary>
    private static void Backoff(int attempt, TimeSpan baseDelay, CancellationToken cancellationToken)
    {
        if (baseDelay <= TimeSpan.Zero)
        {
            return;
        }

        double milliseconds = baseDelay.TotalMilliseconds * Math.Pow(2, attempt - 1);
        int delay = (int)Math.Min(8000, milliseconds) + ((attempt * 37) % 120);

        if (delay <= 0)
        {
            return;
        }

        if (cancellationToken.WaitHandle.WaitOne(delay))
        {
            cancellationToken.ThrowIfCancellationRequested();
        }
    }

    private static ErrorCode? CheckFreeSpace(string cacheRoot, long requiredBytes, long minimumFreeBytes)
    {
        try
        {
            string full = Path.GetFullPath(cacheRoot);
            string? root = Path.GetPathRoot(full);
            if (string.IsNullOrEmpty(root))
            {
                return null;
            }

            DriveInfo drive = new DriveInfo(root!);
            if (!drive.IsReady)
            {
                return null;
            }

            long needed = requiredBytes + minimumFreeBytes;
            return drive.AvailableFreeSpace < needed ? ErrorCode.IoDiskFull : (ErrorCode?)null;
        }
        catch (ArgumentException)
        {
            return null;
        }
        catch (IOException)
        {
            return null;
        }
    }
}
