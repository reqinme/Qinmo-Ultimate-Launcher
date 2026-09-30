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

        using (SemaphoreSlim semaphore = new SemaphoreSlim(Math.Max(1, effective.MaxConcurrency)))
        {
            Task[] workers = new Task[plan.Items.Count];

            for (int i = 0; i < plan.Items.Count; i++)
            {
                int index = i;
                workers[i] = Task.Run(
                    () =>
                    {
                        try
                        {
                            semaphore.Wait(cancellationToken);
                        }
                        catch (OperationCanceledException)
                        {
                            return;
                        }

                        try
                        {
                            DownloadItemReport report = ProcessItem(
                                plan.Items[index], cacheRoot, effective, cancellationToken);
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
                        catch (OperationCanceledException)
                        {
                            reports[index] = new DownloadItemReport(
                                plan.Items[index], DownloadItemState.Failed, ErrorCode.DlFailed, 0, 0);
                        }
                        catch (Exception ex)
                        {
                            // 编排层永不因单个条目抛出：一个坏文件不该让整次安装失败。
                            _log.Failure("download", ErrorCode.DlFailed, ex);
                            reports[index] = new DownloadItemReport(
                                plan.Items[index], DownloadItemState.Failed, ErrorCode.DlFailed, 0, 0);
                        }
                        finally
                        {
                            semaphore.Release();
                        }
                    },
                    cancellationToken);
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
        CancellationToken cancellationToken)
    {
        string destination = Path.Combine(cacheRoot, item.RelativePath);
        string partial = destination + ".part";

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

        for (attempt = 1; attempt <= Math.Max(1, options.MaxAttempts); attempt++)
        {
            cancellationToken.ThrowIfCancellationRequested();

            try
            {
                transferred += FetchOnce(item, UrlForAttempt(item, attempt), destination, partial, options, cancellationToken);

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
                if (!IsRetryable(ex.Code))
                {
                    _log.Failure("download", ex.Code, ex, item.RelativePath);
                    break;
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

            if (attempt < options.MaxAttempts)
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
            Math.Min(attempt, Math.Max(1, options.MaxAttempts)),
            transferred);
    }

    /// <summary>
    /// 第 N 次尝试该用哪个源。
    ///
    /// **一轮换一个源**：首选失败就换备用，备用也失败再回到首选。
    /// 这是从 PCL2 学来的一招——官方被限速或镜像抽风时，
    /// 死磕同一个源只会把重试次数浪费在同一个故障上。
    /// </summary>
    private static string UrlForAttempt(DownloadItem item, int attempt)
    {
        int sources = 1 + item.FallbackUrls.Count;
        int index = (attempt - 1) % sources;

        return index == 0 ? item.Url : item.FallbackUrls[index - 1];
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
                    // 本地残留比远端还长：丢弃残留，让下一轮从头取。
                    SafeDelete(partial);
                    throw new LauncherException(ErrorCode.DlFailed, "range not satisfiable; partial discarded");

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

    /// <summary>null = 跟随系统代理；空串 = 直连；其他 = 显式代理地址。</summary>
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
