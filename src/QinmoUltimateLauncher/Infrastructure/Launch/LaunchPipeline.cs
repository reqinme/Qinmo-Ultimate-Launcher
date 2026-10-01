using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Threading;
using Qul.Application.Diagnostics;
using Qul.Application.Identity;
using Qul.Application.Java;
using Qul.Application.Launch;
using Qul.Application.Ports;
using Qul.Domain.Assets;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Domain.Identity;
using Qul.Domain.Metadata;
using Qul.Domain.Runtime;
using Qul.Infrastructure.Boot;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.IO;
using Qul.Infrastructure.Serialization;
using Qul.Infrastructure.Downloads;
using Qul.Infrastructure.Metadata;
using Qul.Infrastructure.Net;
using Qul.Infrastructure.Platform;
using Qul.Infrastructure.Processes;
using Qul.Infrastructure.Runtime;

namespace Qul.Infrastructure.Launch;

public enum LaunchPipelineMode
{
    /// <summary>只算下载计划，不下载、不启动。</summary>
    PlanOnly = 0,

    /// <summary>下载缺失文件，不启动。</summary>
    Install = 1,

    /// <summary>下载 + 组装启动计划 + 解压 natives，但不拉起进程。</summary>
    Prepare = 2,

    /// <summary>整条链路，直到进程被拉起。</summary>
    Launch = 3,
}

public sealed class LaunchPipelineRequest
{
    public string VersionId { get; set; } = string.Empty;

    public LaunchPipelineMode Mode { get; set; } = LaunchPipelineMode.Launch;

    public IdentitySource IdentitySource { get; set; } = IdentitySource.Offline;

    public string OfflineUserName { get; set; } = "Player";

    /// <summary>非离线来源必须提供已登录的会话。</summary>
    public AuthSession? Session { get; set; }

    public int MaxMemoryMb { get; set; } = 2048;

    /// <summary>用户**显式配置**的目标服务器。为空即"用户没说要去哪"。</summary>
    public string? ServerTarget { get; set; }

    public bool CleanNatives { get; set; } = true;
}

public sealed class LaunchPipelineResult
{
    public LaunchPipelineMode Mode { get; set; }

    public bool Succeeded { get; set; }

    public bool Cancelled { get; set; }

    public ErrorCode? Error { get; set; }

    public string? ErrorDetail { get; set; }

    public VersionDetail? Version { get; set; }

    public AssetIndex? AssetIndex { get; set; }

    public JavaRuntimeCandidate? SelectedJava { get; set; }

    public IReadOnlyList<JavaRuntimeCandidate> JavaCandidates { get; set; } = Array.Empty<JavaRuntimeCandidate>();

    public PlayerIdentity? Identity { get; set; }

    public DownloadPlan? Plan { get; set; }

    public DownloadReport? Download { get; set; }

    public GameLaunchOutcome? Launch { get; set; }

    public IReadOnlyList<PreflightItem> Preflight { get; set; } = Array.Empty<PreflightItem>();

    public IReadOnlyList<string> Notes { get; set; } = Array.Empty<string>();

    /// <summary>启动计划骨架的指纹。不含任何秘密，可以安全地放进诊断报告。</summary>
    public string? SkeletonHash { get; set; }

    public GameProcess? Process => Launch?.Process;
}

/// <summary>
/// 从版本 id 到进程被拉起的完整链路。
///
/// **命令行与界面必须走同一条链路。** 两条各自实现的路径一定会漂移，
/// 然后出现"命令行能启动、界面不能"这种最难查的问题。
/// </summary>
public sealed class LaunchPipeline
{
    private readonly BootContext _boot;
    private readonly SessionLog _log;

    // 探测 Java 要真的把每个 java.exe 跑一遍——19 个运行时就是 19 次进程启动。
    // 界面每次改选版本都会问一次预检，不缓存的话界面会明显卡顿。
    private readonly object _javaGate = new object();
    private readonly Dictionary<int, JavaResolutionOutcome> _javaCache = new Dictionary<int, JavaResolutionOutcome>();

    public LaunchPipeline(BootContext boot, SessionLog? log = null)
    {
        _boot = boot ?? throw new ArgumentNullException(nameof(boot));
        _log = log ?? boot.Log;
    }

    /// <summary>取版本清单（带磁盘缓存）。供界面列出可选版本。</summary>
    public VersionManifest FetchManifest(IProgress<LaunchProgress>? progress, CancellationToken cancellationToken)
    {
        Report(progress, LaunchStage.Resolving, "读取版本清单…");

        MetadataClient metadata = new MetadataClient(new HttpTransport(), _log);

        return metadata.FetchManifest(Path.Combine(
            _boot.Layout.CacheMetaDirectory,
            new CachePathConventions().VersionManifestFile()));
    }

    /// <summary>
    /// 不联网、不下载的启动前预检。
    /// 版本要求未知时按 Java 8 处理——那是所有版本的下限。
    /// </summary>
    public IReadOnlyList<PreflightItem> PreparePreflight(LaunchPipelineRequest request)
    {
        int requiredMajor = ReadCachedRequiredJava(request.VersionId) ?? 8;

        JavaResolutionOutcome outcome = ResolveJavaCached(requiredMajor);

        bool clientInstalled = File.Exists(AbsoluteCachePath(
            new CachePathConventions().ClientJarFile(request.VersionId)));

        return PreflightCheck.Evaluate(new PreflightContext
        {
            IdentitySource = request.IdentitySource,
            IdentityNotices = NoticesFor(request.IdentitySource),
            IsOnlineVerified = request.IdentitySource != IdentitySource.Offline,
            ServerTarget = request.ServerTarget,
            JavaAvailable = outcome.Succeeded,
            ClientInstalled = clientInstalled,
            FreeDiskBytes = FreeDiskBytes(),
            EstimatedBytes = 0,
        });
    }

    public LaunchPipelineResult Run(
        LaunchPipelineRequest request,
        IProgress<LaunchProgress>? progress,
        CancellationToken cancellationToken)
    {
        if (request == null)
        {
            throw new ArgumentNullException(nameof(request));
        }

        LaunchPipelineResult result = new LaunchPipelineResult { Mode = request.Mode };
        List<string> notes = new List<string>();
        result.Notes = notes;

        try
        {
            return RunCore(request, result, notes, progress, cancellationToken);
        }
        catch (OperationCanceledException)
        {
            result.Succeeded = false;
            result.Cancelled = true;
            Report(progress, LaunchStage.Cancelled, "已取消。");
            return result;
        }
        catch (LauncherException ex)
        {
            result.Succeeded = false;
            result.Error = ex.Code;
            result.ErrorDetail = ErrorCodes.Hint(ex.Code);
            return result;
        }
    }

    private LaunchPipelineResult RunCore(
        LaunchPipelineRequest request,
        LaunchPipelineResult result,
        List<string> notes,
        IProgress<LaunchProgress>? progress,
        CancellationToken cancellationToken)
    {
        DataLayout layout = _boot.Layout;
        CachePathConventions conventions = new CachePathConventions();
        MetadataClient metadata = new MetadataClient(new HttpTransport(), _log);

        // ---------- 元数据 ----------
        Report(progress, LaunchStage.Resolving, "读取版本清单…");
        VersionManifest manifest = metadata.FetchManifest(
            Path.Combine(layout.CacheMetaDirectory, conventions.VersionManifestFile()));

        Report(progress, LaunchStage.Resolving, "清单共 " + manifest.Versions.Count + " 个版本；最新正式版 " + manifest.LatestRelease);

        VersionSummary? summary = manifest.Find(request.VersionId);
        if (summary == null)
        {
            return Fail(result, ErrorCode.MetaIndexFailed, "版本清单里没有「" + request.VersionId + "」。");
        }

        Report(progress, LaunchStage.Resolving, "读取 " + request.VersionId + " 的元数据…");
        result.Version = metadata.FetchVersion(
            summary.Url,
            request.VersionId,
            Path.Combine(layout.CacheMetaDirectory, conventions.VersionDetailFile(request.VersionId)));

        VersionDetail version = result.Version;
        int requiredJava = version.JavaVersion?.MajorVersion ?? 8;
        notes.Add("版本要求 Java " + requiredJava + "；库 " + version.Libraries.Count + " 个");

        // ---------- Java ----------
        JavaResolutionOutcome resolution = ResolveJavaCached(requiredJava);

        for (int i = 0; i < resolution.Notes.Count; i++)
        {
            notes.Add(resolution.Notes[i]);

            // **把 Java 解析的说明写进日志。**
            //
            // 解析器精心记了"为什么没用你指定的 Java"（不可用 / 不满足要求），
            // 但先前**没有任何地方显示它们**——于是它注释里那句
            // "静默回落会让用户以为自己的设置生效了"在实际上依然成立：
            // 用户在配置里指定了 Java，程序用了别的，而他一无所知。
            //
            // 这些说明同时是排障时最需要的东西，所以进日志（也就进了诊断报告）。
            _log.Info("java", resolution.Notes[i]);
        }

        if (!resolution.Succeeded)
        {
            return Fail(
                result,
                resolution.Selection.Error ?? ErrorCode.JavaNotFound,
                "找不到满足要求的 Java：" + (resolution.Selection.Explanation ?? string.Empty));
        }

        result.SelectedJava = resolution.Selection.Selected;
        Report(progress, LaunchStage.Resolving, "选定 " + result.SelectedJava!.Describe());
        notes.Add("选定 Java：" + result.SelectedJava.Describe());

        // ---------- 资源索引 ----------
        if (version.AssetIndex?.Url != null)
        {
            Report(progress, LaunchStage.Resolving, "读取资源索引 " + version.AssetIndex.Id + "…");
            result.AssetIndex = metadata.FetchAssetIndex(
                version.AssetIndex.Url!,
                version.AssetIndex.Id,
                Path.Combine(layout.CacheMetaDirectory, conventions.AssetIndexFile(version.AssetIndex.Id)));

            notes.Add("资源对象 " + result.AssetIndex.Count + " 个");
        }

        // ---------- 身份 ----------
        AuthOutcome auth = ResolveIdentity(request, cancellationToken);
        if (!auth.Succeeded)
        {
            return Fail(result, auth.Error ?? ErrorCode.AuthOfflineNameInvalid, auth.Explanation ?? string.Empty);
        }

        result.Identity = auth.Session!.ToIdentity();

        for (int i = 0; i < result.Identity.CapabilityNotices.Count; i++)
        {
            notes.Add(result.Identity.CapabilityNotices[i]);
        }

        // ---------- 下载计划 ----------
        EnvironmentProfile environment = PlatformProbe.ForJavaRuntime(result.SelectedJava);
        result.Plan = new DownloadPlanBuilder().Build(version, environment, result.AssetIndex);

        notes.Add("下载计划 " + result.Plan.Items.Count + " 项 / " + Megabytes(result.Plan.KnownTotalBytes));

        // ---------- 预检 ----------
        result.Preflight = PreflightCheck.Evaluate(new PreflightContext
        {
            IdentitySource = request.IdentitySource,
            IdentityNotices = result.Identity.CapabilityNotices,
            IsOnlineVerified = result.Identity.IsOnlineVerified,
            ServerTarget = request.ServerTarget,
            JavaAvailable = true,
            ClientInstalled = File.Exists(AbsoluteCachePath(conventions.ClientJarFile(request.VersionId))),
            FreeDiskBytes = FreeDiskBytes(),
            EstimatedBytes = result.Plan.KnownTotalBytes,
        });

        if (PreflightCheck.HasBlocking(result.Preflight))
        {
            return Fail(result, FirstBlockingError(result.Preflight), "启动前预检未通过。");
        }

        if (request.Mode == LaunchPipelineMode.PlanOnly)
        {
            result.Succeeded = true;
            Report(progress, LaunchStage.Resolving, "计划已生成（未下载）。");
            return result;
        }

        // ---------- 下载 ----------
        Report(progress, LaunchStage.Downloading, "开始获取所需文件…");

        DownloadEngine engine = new DownloadEngine(new HttpTransport(), _log);
        result.Download = engine.EnsureAll(
            result.Plan,
            layout.CacheDirectory,
            new DownloadOptions
            {
                MaxConcurrency = Math.Max(1, _boot.Config.Network.MaxConcurrency),
                MaxAttempts = 3,
                MinimumFreeBytes = 128L * 1024 * 1024,
                BaseRetryDelay = TimeSpan.FromMilliseconds(400),
            },
            WrapDownloadProgress(progress),
            cancellationToken);

        // 取消不是失败。
        // 下载引擎在取消时会把在途项记成"失败"并正常返回，所以必须先看取消标记——
        // 否则用户点了取消，看到的却是"启动失败"外加一个错误码。
        if (cancellationToken.IsCancellationRequested)
        {
            result.Cancelled = true;
            Report(progress, LaunchStage.Cancelled, "已取消。");
            return result;
        }

        Report(
            progress,
            LaunchStage.Downloading, "下载完成：新下 " + result.Download.DownloadedCount
            + "，命中缓存 " + result.Download.PresentCount
            + "，失败 " + result.Download.Failures.Count);

        notes.Add("下载：新下 " + result.Download.DownloadedCount + "，命中 " + result.Download.PresentCount
                  + "，失败 " + result.Download.Failures.Count);

        // **取消要在"失败"之前判。**
        // 否则取消会掉进下面的失败分支，报一个 QUL-DL-* 的错误码——
        // 用户按了取消，却收到一条看起来像"下载坏了"的提示。
        if (result.Download.WasCancelled)
        {
            result.Cancelled = true;
            Report(progress, LaunchStage.Cancelled, "已取消，未下载的文件留待下次继续。");
            return result;
        }

        if (!result.Download.IsComplete)
        {
            DownloadItemReport first = result.Download.Failures[0];
            return Fail(
                result,
                first.Error ?? ErrorCode.DlFailed,
                first.Item.Describe() + "（尝试 " + first.Attempts + " 次）");
        }

        if (request.Mode == LaunchPipelineMode.Install)
        {
            result.Succeeded = true;
            return result;
        }

        // 下载完再跑一遍：此刻文件已齐全，这一遍不传任何字节，只逐个核对摘要，
        // 并自愈写入损坏或事后被改动的文件。
        // 它不是为了"凑一个阶段出来"——确实会重新核对每一个文件。
        Report(progress, LaunchStage.Verifying, "正在校验全部文件…");

        DownloadReport verification = engine.EnsureAll(
            result.Plan,
            layout.CacheDirectory,
            new DownloadOptions
            {
                MaxConcurrency = Math.Max(1, _boot.Config.Network.MaxConcurrency),
                MaxAttempts = 3,
                MinimumFreeBytes = 128L * 1024 * 1024,
                BaseRetryDelay = TimeSpan.FromMilliseconds(400),
            },
            null,
            cancellationToken);

        // 取消不是失败。
        // 下载引擎在取消时会把在途项记成"失败"并正常返回，所以必须先看取消标记——
        // 否则用户点了取消，看到的却是"启动失败"外加一个错误码。
        if (cancellationToken.IsCancellationRequested)
        {
            result.Cancelled = true;
            Report(progress, LaunchStage.Cancelled, "已取消。");
            return result;
        }

        if (!verification.IsComplete)
        {
            DownloadItemReport broken = verification.Failures[0];
            return Fail(result, broken.Error ?? ErrorCode.DlFailed, "校验未通过：" + broken.Item.Describe());
        }

        notes.Add("校验：核对 " + verification.PresentCount + " 个文件，重新获取 " + verification.DownloadedCount + " 个");

        // ---------- 启动计划与进程 ----------
        GameLaunchRequest launchRequest = new GameLaunchRequest
        {
            CleanNatives = request.CleanNatives,
            GameLogPath = Path.Combine(layout.LogDirectory, "game-" + request.VersionId + ".log"),
            Plan = new LaunchPlanRequest
            {
                Version = version,
                Environment = environment,
                Java = result.SelectedJava,
                Identity = result.Identity,
                CacheRoot = layout.CacheDirectory,
                GameDirectory = layout.GameRoot,
                NativesDirectory = Path.Combine(layout.CacheDirectory, "natives", request.VersionId),
                MaxMemoryMb = request.MaxMemoryMb,

                // **把配置里的额外 JVM 参数接上。**
                //
                // 先前 `launch.extraJvmArgs` 只被读写进 config.json，构建器也支持它，
                // 但**没有任何地方把它从配置传进来**——用户改了等于没改，而且不报错。
                // 这是本项目第三次出现"配置项读写了却从不生效"（前两次是代理三态与 mirrorBaseUrl）。
                ExtraJvmArgs = _boot.Config.Launch.ExtraJvmArgs,
                AssetsDirectory = Path.Combine(layout.CacheDirectory, conventions.AssetsRoot),
                LibraryDirectory = Path.Combine(layout.CacheDirectory, conventions.LibrariesPrefix),
            },
        };

        Report(progress, LaunchStage.Extracting, "正在校验客户端并解压 natives…");

        if (request.Mode != LaunchPipelineMode.Prepare)
        {
            Report(progress, LaunchStage.Starting, "正在拉起游戏进程…");
        }

        GameLauncher launcher = new GameLauncher(_log);

        // 记下启动时刻：崩溃报告定位要靠它把"本次"与"上一次留下的"分开。
        DateTimeOffset launchStartedAt = DateTimeOffset.UtcNow;

        result.Launch = request.Mode == LaunchPipelineMode.Prepare
            ? launcher.Prepare(launchRequest)
            : launcher.Launch(launchRequest);

        for (int i = 0; i < result.Launch.Notes.Count; i++)
        {
            notes.Add(result.Launch.Notes[i]);
        }

        result.SkeletonHash = FindSkeletonHash(result.Launch.Notes);

        if (result.SkeletonHash != null)
        {
            // **归到"启动"阶段，不是"解压"。**
            //
            // 这一行在 launcher.Launch(...) 之后执行，而那里已经报过"启动"阶段了；
            // 挂到"解压"下会让阶段顺序变成 解压 → 启动 → 解压，**往回跳**。
            // 用户看到的是"解压"出现两次，会以为解压跑了两遍。
            //
            // 指纹描述的是"将要启动什么"，本来就属于启动阶段。
            Report(progress, LaunchStage.Starting, "骨架指纹 " + result.SkeletonHash);
        }

        if (!result.Launch.Prepared)
        {
            return Fail(
                result,
                result.Launch.Error ?? ErrorCode.PlanUnresolvedPlaceholder,
                "启动计划未能组装完成。");
        }

        if (request.Mode == LaunchPipelineMode.Prepare)
        {
            result.Succeeded = true;
            Report(progress, LaunchStage.Extracting, "预演完成（未启动进程）。");
            return result;
        }

        if (!result.Launch.Started || result.Launch.Process == null)
        {
            // **拉起失败时也看一眼崩溃报告**：虽然这条路游戏通常没跑起来，
            // 但"进程对象建好、随即退出"也会走到这里，那时报告是存在的。
            return Fail(
                result,
                result.Launch.Error ?? ErrorCode.ProcStartFailed,
                "进程未能拉起。" + DescribeCrash(result.Launch.Process, layout, launchStartedAt, progress, 1));
        }

        // **拉起后立刻看一眼它是不是已经死了。**
        //
        // 这一步**不等待**（只查一次 HasExited），所以不给正常启动增加任何延迟——
        // 冷启动 341 ms 是预算，不能为了检测而花掉几十毫秒。
        //
        // 而它抓得到最常见的一类失败：JVM 参数不合理（例如最大内存超过物理内存）
        // 时，java.exe 会在**毫秒级**退出。先前那种情况会被报成"启动成功"——
        // 因为进程对象确实建起来了，只是它立刻就没了。
        if (result.Launch.Process.HasExited)
        {
            int exitCode = result.Launch.Process.WaitForExit().ExitCode;

            return Fail(
                result,
                ErrorCode.ProcNonZeroExit,
                "游戏进程启动后立即退出（退出码 " + exitCode.ToString(System.Globalization.CultureInfo.InvariantCulture) + "）。"
                + DescribeCrash(result.Launch.Process, layout, launchStartedAt, progress, exitCode));
        }

        result.Succeeded = true;
        Report(progress, LaunchStage.Running, "已拉起进程 pid=" + result.Launch.Process.ProcessId);
        return result;
    }

    /// <summary>
    /// 找**本次启动之后**新写的崩溃报告，读出一条能给玩家看的结论；没有就返回空串。
    ///
    /// **绝不让分析失败改变失败本身。** 找不到报告、读不出来、认不出原因，
    /// 都只是少一句解释——原来的错误码与提示照常返回。
    ///
    /// <paramref name="exitCode"/> 是**真实退出码**：拿得到就给真的，
    /// 分析器在"连报告都没有"时会用它给出"进程异常退出但没有留下报告"的结论。
    /// 拿不到时传 0，那条分支就不会误触发。
    /// </summary>
    private string DescribeCrash(GameProcess? process, DataLayout layout, DateTimeOffset notBefore, IProgress<LaunchProgress>? progress, int exitCode)
    {
        string? path = CrashReportLocator.FindNewest(layout.GameRoot, notBefore);
        string? text = CrashReportLocator.Read(path);
        CrashFinding? finding = CrashAnalysis.Analyze(text, exitCode);

        if (finding == null)
        {
            return string.Empty;
        }

        _log.Warn("launch", "crash analysis: " + finding.Summary + " | " + finding.Evidence);

        if (progress != null)
        {
            Report(progress, LaunchStage.Failed, "崩溃分析：" + finding.Summary);
        }

        string detail = " 崩溃分析：" + finding.Summary;

        for (int i = 0; i < finding.Suggestions.Count; i++)
        {
            detail += " " + (i + 1).ToString(System.Globalization.CultureInfo.InvariantCulture)
                    + ") " + finding.Suggestions[i];
        }

        return detail;
    }
    // ---------- 辅助 ----------

    /// <summary>
    /// 按配置组装 Java 来源。**手动指定排在第一个**（解析器的覆盖语义是"手动优先"）。
    ///
    /// **抽成静态纯函数是为了能直接测。** <see cref="LaunchPipeline"/> 要 <c>BootContext</c>
    /// 才能实例化，而"配置 → 来源列表"这段逻辑本身什么都不需要。
    ///
    /// 这一段先前**根本不存在**：生产线只传 <c>DetectedJavaRuntimeProvider</c>，
    /// 于是 `java.mode` / `java.manualPath` 两个配置项读写了却从不生效——
    /// 而 P3 规范白纸黑字写着"用户手动指定覆盖"。那是八个死配置里唯一**违反已写下要求**的一个。
    /// </summary>
    public static IReadOnlyList<IJavaRuntimeProvider> BuildJavaProviders(JavaSettings java, SessionLog? log)
    {
        List<IJavaRuntimeProvider> providers = new List<IJavaRuntimeProvider>();

        if (java != null
            && java.Mode == JavaSelectionMode.Manual
            && !string.IsNullOrWhiteSpace(java.ManualPath))
        {
            string manualPath = java.ManualPath!;

            // 用访问器而不是值：Provider 每次 Discover 时现取，路径改了立刻反映。
            providers.Add(new ManualJavaRuntimeProvider(new JavaExecutableProbe(log), () => manualPath));
        }

        // 本机探测永远排在后面：手动指定优先，但它跑不起来时仍要能回落到探测。
        providers.Add(new DetectedJavaRuntimeProvider());

        return providers;
    }
    private JavaResolutionOutcome ResolveJavaCached(int requiredMajor)
    {
        lock (_javaGate)
        {
            if (_javaCache.TryGetValue(requiredMajor, out JavaResolutionOutcome? cached))
            {
                return cached;
            }
        }

        JavaResolutionOutcome outcome = new JavaRuntimeResolver(
            BuildJavaProviders(_boot.Config.Java, _log))
            .Resolve(new JavaSelectionRequest { RequiredMajorVersion = requiredMajor });

        lock (_javaGate)
        {
            _javaCache[requiredMajor] = outcome;
        }

        return outcome;
    }

    private AuthOutcome ResolveIdentity(LaunchPipelineRequest request, CancellationToken cancellationToken)
    {
        if (request.IdentitySource == IdentitySource.Offline)
        {
            return new OfflineAuthProvider().Authenticate(
                new AuthRequest { OfflineUserName = request.OfflineUserName },
                cancellationToken);
        }

        if (request.Session == null)
        {
            return AuthOutcome.Failure(
                ErrorCode.AuthPrerequisiteMissing,
                "该身份来源尚未登录，请先完成登录。");
        }

        return AuthOutcome.Success(request.Session);
    }

    private static IReadOnlyList<string> NoticesFor(IdentitySource source)
    {
        return source == IdentitySource.Offline
            ? OfflineIdentityFactory.CapabilityNotices
            : Array.Empty<string>();
    }

    private static IProgress<DownloadProgress>? WrapDownloadProgress(IProgress<LaunchProgress>? progress)
    {
        if (progress == null)
        {
            return null;
        }

        return new SynchronousProgress<DownloadProgress>(p =>
        {
            if (p.FilesTotal > 0 && (p.FilesCompleted % 25 == 0 || p.FilesCompleted == p.FilesTotal))
            {
                progress.Report(new LaunchProgress(
                    LaunchStage.Downloading,
                    "进度 " + p.FilesCompleted + "/" + p.FilesTotal + "，已传 " + Megabytes(p.BytesTransferred),
                    p.FilesCompleted,
                    p.FilesTotal,
                    p.BytesTransferred,
                    p.KnownTotalBytes ?? 0));
            }
        });
    }

    /// <summary>
    /// 同步转发的进度。
    /// Progress&lt;T&gt; 会把回调异步投递到线程池，结果是下载进度在流程结束后才到达——
    /// 命令行里表现为顺序错乱，界面里表现为进度条不动然后突然跳完。
    /// </summary>
    private sealed class SynchronousProgress<T> : IProgress<T>
    {
        private readonly Action<T> _handler;

        public SynchronousProgress(Action<T> handler)
        {
            _handler = handler;
        }

        public void Report(T value)
        {
            _handler(value);
        }
    }

    private static ErrorCode FirstBlockingError(IReadOnlyList<PreflightItem> items)
    {
        for (int i = 0; i < items.Count; i++)
        {
            if (!items[i].BlocksLaunch)
            {
                continue;
            }

            switch (items[i].Id)
            {
                case PreflightCheck.IdJavaAvailable:
                    return ErrorCode.JavaNotFound;
                case PreflightCheck.IdDiskSpace:
                    return ErrorCode.IoDiskFull;
                default:
                    return ErrorCode.DlFailed;
            }
        }

        return ErrorCode.DlFailed;
    }

    private static LaunchPipelineResult Fail(LaunchPipelineResult result, ErrorCode error, string detail)
    {
        result.Succeeded = false;
        result.Error = error;
        result.ErrorDetail = detail;
        return result;
    }

    private string AbsoluteCachePath(string relative)
    {
        string text = relative.Replace('/', Path.DirectorySeparatorChar);
        return Path.IsPathRooted(text) ? text : Path.Combine(_boot.Layout.CacheDirectory, text);
    }

    private long FreeDiskBytes()
    {
        try
        {
            string root = Path.GetPathRoot(_boot.Layout.CacheDirectory) ?? "C:\\";
            return new DriveInfo(root).AvailableFreeSpace;
        }
        catch (IOException)
        {
            return long.MaxValue;
        }
        catch (ArgumentException)
        {
            return long.MaxValue;
        }
    }

    /// <summary>从已缓存的版本元数据里读所需 Java 主版本；没有缓存则返回 null。</summary>
    private int? ReadCachedRequiredJava(string versionId)
    {
        try
        {
            string path = Path.Combine(
                _boot.Layout.CacheMetaDirectory,
                new CachePathConventions().VersionDetailFile(versionId));

            if (!File.Exists(path))
            {
                return null;
            }

            JsonObject root = JsonValue.Parse(File.ReadAllText(path)).RequireObject();
            return root.GetObject("javaVersion")?.GetInt("majorVersion");
        }
        catch (Exception ex) when (ex is IOException || ex is LauncherException || ex is JsonFormatException)
        {
            return null;
        }
    }

    /// <summary>骨架指纹由下层写进过程记录；这里取出来给诊断报告用（它不含任何秘密）。</summary>
    private static string? FindSkeletonHash(IReadOnlyList<string> notes)
    {
        const string Marker = "骨架指纹 ";

        for (int i = 0; i < notes.Count; i++)
        {
            int at = notes[i].IndexOf(Marker, StringComparison.Ordinal);
            if (at >= 0)
            {
                return notes[i].Substring(at + Marker.Length).Trim();
            }
        }

        return null;
    }

    private void Report(IProgress<LaunchProgress>? progress, LaunchStage stage, string message)
    {
        _log.Info("pipeline", "[" + stage + "] " + message);
        progress?.Report(new LaunchProgress(stage, message));
    }

    private static string Megabytes(long bytes)
    {
        return (bytes / 1024.0 / 1024.0).ToString("F1", CultureInfo.InvariantCulture) + " MB";
    }
}
