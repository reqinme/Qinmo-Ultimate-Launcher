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

    public LaunchPipeline(BootContext boot, SessionLog? log = null)
    {
        _boot = boot ?? throw new ArgumentNullException(nameof(boot));
        _log = log ?? boot.Log;
    }

    /// <summary>
    /// 不联网、不下载的启动前预检。
    /// 版本要求未知时按 Java 8 处理——那是所有版本的下限。
    /// </summary>
    public IReadOnlyList<PreflightItem> PreparePreflight(LaunchPipelineRequest request)
    {
        int requiredMajor = ReadCachedRequiredJava(request.VersionId) ?? 8;

        JavaRuntimeResolver resolver = new JavaRuntimeResolver(
            new IJavaRuntimeProvider[] { new DetectedJavaRuntimeProvider() });

        JavaResolutionOutcome outcome = resolver.Resolve(
            new JavaSelectionRequest { RequiredMajorVersion = requiredMajor });

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
        IProgress<string>? progress,
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
            Report(progress, "已取消。");
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
        IProgress<string>? progress,
        CancellationToken cancellationToken)
    {
        DataLayout layout = _boot.Layout;
        CachePathConventions conventions = new CachePathConventions();
        MetadataClient metadata = new MetadataClient(new HttpTransport(), _log);

        // ---------- 元数据 ----------
        Report(progress, "读取版本清单…");
        VersionManifest manifest = metadata.FetchManifest(
            Path.Combine(layout.CacheMetaDirectory, conventions.VersionManifestFile()));

        Report(progress, "清单共 " + manifest.Versions.Count + " 个版本；最新正式版 " + manifest.LatestRelease);

        VersionSummary? summary = manifest.Find(request.VersionId);
        if (summary == null)
        {
            return Fail(result, ErrorCode.MetaIndexFailed, "版本清单里没有「" + request.VersionId + "」。");
        }

        Report(progress, "读取 " + request.VersionId + " 的元数据…");
        result.Version = metadata.FetchVersion(
            summary.Url,
            request.VersionId,
            Path.Combine(layout.CacheMetaDirectory, conventions.VersionDetailFile(request.VersionId)));

        VersionDetail version = result.Version;
        int requiredJava = version.JavaVersion?.MajorVersion ?? 8;
        notes.Add("版本要求 Java " + requiredJava + "；库 " + version.Libraries.Count + " 个");

        // ---------- Java ----------
        JavaRuntimeResolver resolver = new JavaRuntimeResolver(
            new IJavaRuntimeProvider[] { new DetectedJavaRuntimeProvider() });

        JavaResolutionOutcome resolution = resolver.Resolve(
            new JavaSelectionRequest { RequiredMajorVersion = requiredJava });

        for (int i = 0; i < resolution.Notes.Count; i++)
        {
            notes.Add(resolution.Notes[i]);
        }

        if (!resolution.Succeeded)
        {
            return Fail(
                result,
                resolution.Selection.Error ?? ErrorCode.JavaNotFound,
                "找不到满足要求的 Java：" + (resolution.Selection.Explanation ?? string.Empty));
        }

        result.SelectedJava = resolution.Selection.Selected;
        Report(progress, "选定 " + result.SelectedJava!.Describe());
        notes.Add("选定 Java：" + result.SelectedJava.Describe());

        // ---------- 资源索引 ----------
        if (version.AssetIndex?.Url != null)
        {
            Report(progress, "读取资源索引 " + version.AssetIndex.Id + "…");
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
            Report(progress, "计划已生成（未下载）。");
            return result;
        }

        // ---------- 下载 ----------
        Report(progress, "开始下载（并发 8）…");

        DownloadEngine engine = new DownloadEngine(new HttpTransport(), _log);
        result.Download = engine.EnsureAll(
            result.Plan,
            layout.CacheDirectory,
            new DownloadOptions
            {
                MaxConcurrency = 8,
                MaxAttempts = 3,
                MinimumFreeBytes = 128L * 1024 * 1024,
                BaseRetryDelay = TimeSpan.FromMilliseconds(400),
            },
            WrapDownloadProgress(progress),
            cancellationToken);

        Report(
            progress,
            "下载完成：新下 " + result.Download.DownloadedCount
            + "，命中缓存 " + result.Download.PresentCount
            + "，失败 " + result.Download.Failures.Count);

        notes.Add("下载：新下 " + result.Download.DownloadedCount + "，命中 " + result.Download.PresentCount
                  + "，失败 " + result.Download.Failures.Count);

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
                AssetsDirectory = Path.Combine(layout.CacheDirectory, conventions.AssetsRoot),
                LibraryDirectory = Path.Combine(layout.CacheDirectory, conventions.LibrariesPrefix),
            },
        };

        GameLauncher launcher = new GameLauncher(_log);

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
            Report(progress, "骨架指纹 " + result.SkeletonHash);
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
            Report(progress, "预演完成（未启动进程）。");
            return result;
        }

        if (!result.Launch.Started || result.Launch.Process == null)
        {
            return Fail(result, result.Launch.Error ?? ErrorCode.ProcStartFailed, "进程未能拉起。");
        }

        result.Succeeded = true;
        Report(progress, "已拉起进程 pid=" + result.Launch.Process.ProcessId);
        return result;
    }

    // ---------- 辅助 ----------

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

    private static IProgress<DownloadProgress>? WrapDownloadProgress(IProgress<string>? progress)
    {
        if (progress == null)
        {
            return null;
        }

        return new SynchronousProgress<DownloadProgress>(p =>
        {
            if (p.FilesTotal > 0 && (p.FilesCompleted % 250 == 0 || p.FilesCompleted == p.FilesTotal))
            {
                progress.Report("  进度 " + p.FilesCompleted + "/" + p.FilesTotal
                                + "，已传 " + Megabytes(p.BytesTransferred));
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

    private void Report(IProgress<string>? progress, string message)
    {
        _log.Info("pipeline", message);
        progress?.Report(message);
    }

    private static string Megabytes(long bytes)
    {
        return (bytes / 1024.0 / 1024.0).ToString("F1", CultureInfo.InvariantCulture) + " MB";
    }
}
