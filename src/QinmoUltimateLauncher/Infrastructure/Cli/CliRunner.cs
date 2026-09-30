using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Threading;
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
using Qul.Infrastructure.Downloads;
using Qul.Infrastructure.Launch;
using Qul.Infrastructure.Metadata;
using Qul.Infrastructure.Net;
using Qul.Infrastructure.Platform;
using Qul.Infrastructure.Processes;
using Qul.Infrastructure.Runtime;

namespace Qul.Infrastructure.Cli;

/// <summary>
/// 无界面驱动整条链路：读元数据 → 选 Java → 生成下载计划 → 下载 → 组装启动计划 → 解压 natives → 拉起进程。
///
/// 两个用途：
///   1) 真实验收 —— "能不能启动"这件事必须在没有界面的情况下也能被验证；
///   2) 架构要求 —— "UI 挂了不影响命令行启动"。
/// </summary>
public static class CliRunner
{
    public const int ExitOk = 0;
    public const int ExitFailed = 1;
    public const int ExitUsage = 2;

    public static int Run(BootContext boot, IReadOnlyList<string> args)
    {
        if (boot == null)
        {
            throw new ArgumentNullException(nameof(boot));
        }

        if (args == null || args.Count == 0)
        {
            return Usage(boot);
        }

        string command = args[0].Trim().ToLowerInvariant();

        switch (command)
        {
            case "plan":
                return RunPlan(boot, args);
            case "install":
                return RunInstall(boot, args);
            case "launch":
                return RunLaunch(boot, args);
            default:
                return Usage(boot);
        }
    }

    // ---------- 命令 ----------

    private static int RunPlan(BootContext boot, IReadOnlyList<string> args)
    {
        string versionId = Require(args, 1);
        if (versionId.Length == 0)
        {
            return Usage(boot);
        }

        Resolved resolved = Resolve(boot, versionId);
        if (resolved.Error != null)
        {
            return Fail(boot, resolved.Error.Value);
        }

        DownloadPlan plan = BuildDownloadPlan(boot, resolved);
        Report(boot, "下载计划：" + plan.Items.Count + " 项，已知体积 "
                     + Megabytes(plan.KnownTotalBytes) + "，重复路径 " + plan.DuplicatePaths.Count
                     + "，内容冲突 " + plan.ConflictingPaths.Count);

        Dictionary<DownloadItemKind, int> byKind = new Dictionary<DownloadItemKind, int>();
        for (int i = 0; i < plan.Items.Count; i++)
        {
            DownloadItemKind kind = plan.Items[i].Kind;
            byKind.TryGetValue(kind, out int count);
            byKind[kind] = count + 1;
        }

        foreach (KeyValuePair<DownloadItemKind, int> pair in byKind)
        {
            Report(boot, "  " + pair.Key + "：" + pair.Value);
        }

        return ExitOk;
    }

    private static int RunInstall(BootContext boot, IReadOnlyList<string> args)
    {
        string versionId = Require(args, 1);
        if (versionId.Length == 0)
        {
            return Usage(boot);
        }

        Resolved resolved = Resolve(boot, versionId);
        if (resolved.Error != null)
        {
            return Fail(boot, resolved.Error.Value);
        }

        DownloadPlan plan = BuildDownloadPlan(boot, resolved);
        Report(boot, "下载计划：" + plan.Items.Count + " 项，已知体积 " + Megabytes(plan.KnownTotalBytes));

        return Download(boot, plan) ? ExitOk : ExitFailed;
    }

    private static int RunLaunch(BootContext boot, IReadOnlyList<string> args)
    {
        string versionId = Require(args, 1);
        if (versionId.Length == 0)
        {
            return Usage(boot);
        }

        bool dryRun = HasFlag(args, "--dry-run");
        bool keepRunning = HasFlag(args, "--keep");
        string offlineName = OptionValue(args, "--offline-name", "Player");
        int memory = ParseInt(OptionValue(args, "--memory", "2048"), 2048);
        int holdSeconds = ParseInt(OptionValue(args, "--hold", "5"), 5);

        Resolved resolved = Resolve(boot, versionId);
        if (resolved.Error != null)
        {
            return Fail(boot, resolved.Error.Value);
        }

        DownloadPlan plan = BuildDownloadPlan(boot, resolved);
        Report(boot, "下载计划：" + plan.Items.Count + " 项，已知体积 " + Megabytes(plan.KnownTotalBytes));

        if (!HasFlag(args, "--skip-download") && !Download(boot, plan))
        {
            return ExitFailed;
        }

        PlayerIdentity identity;
        try
        {
            identity = OfflineIdentityFactory.Create(offlineName);
        }
        catch (LauncherException ex)
        {
            return Fail(boot, ex.Code);
        }

        foreach (string notice in identity.CapabilityNotices)
        {
            Report(boot, "告知：" + notice);
        }

        EnvironmentProfile environment = PlatformProbe.ForJavaRuntime(resolved.Java);

        GameLaunchRequest request = new GameLaunchRequest
        {
            CleanNatives = true,
            GameLogPath = Path.Combine(boot.Layout.LogDirectory, "game-" + versionId + ".log"),
            Plan = new LaunchPlanRequest
            {
                Version = resolved.Version!,
                Environment = environment,
                Java = resolved.Java!,
                Identity = identity,
                CacheRoot = boot.Layout.CacheDirectory,
                GameDirectory = boot.Layout.GameRoot,
                NativesDirectory = Path.Combine(boot.Layout.CacheDirectory, "natives", versionId),
                MaxMemoryMb = memory,
                AssetsDirectory = Path.Combine(boot.Layout.CacheDirectory, "assets"),
                LibraryDirectory = Path.Combine(boot.Layout.CacheDirectory, "libraries"),
            },
        };

        GameLauncher launcher = new GameLauncher(boot.Log);
        GameLaunchOutcome outcome = dryRun ? launcher.Prepare(request) : launcher.Launch(request);

        foreach (string note in outcome.Notes)
        {
            Report(boot, "  " + note);
        }

        if (!outcome.Prepared)
        {
            return Fail(boot, outcome.Error ?? ErrorCode.PlanUnresolvedPlaceholder);
        }

        if (dryRun)
        {
            Report(boot, "预演完成（未启动进程）");
            return ExitOk;
        }

        if (!outcome.Started || outcome.Process == null)
        {
            return Fail(boot, outcome.Error ?? ErrorCode.ProcStartFailed);
        }

        return AwaitWindow(boot, outcome.Process, holdSeconds, keepRunning);
    }

    // ---------- 主流程 ----------

    private sealed class Resolved
    {
        public VersionDetail? Version { get; set; }

        public AssetIndex? AssetIndex { get; set; }

        public JavaRuntimeCandidate? Java { get; set; }

        public ErrorCode? Error { get; set; }
    }

    private static Resolved Resolve(BootContext boot, string versionId)
    {
        Resolved resolved = new Resolved();

        MetadataClient client = new MetadataClient(new HttpTransport(), boot.Log);
        string metaDirectory = boot.Layout.CacheMetaDirectory;

        Report(boot, "读取版本清单…");
        VersionManifest manifest;
        try
        {
            manifest = client.FetchManifest(Path.Combine(metaDirectory, "version_manifest_v2.json"));
        }
        catch (LauncherException ex)
        {
            resolved.Error = ex.Code;
            return resolved;
        }

        Report(boot, "  清单共 " + manifest.Versions.Count + " 个版本；最新正式版 " + manifest.LatestRelease);

        VersionSummary? summary = manifest.Find(versionId);
        if (summary == null)
        {
            Report(boot, "  清单里没有版本 " + versionId);
            resolved.Error = ErrorCode.MetaIndexFailed;
            return resolved;
        }

        Report(boot, "读取 " + versionId + " 的元数据…");
        try
        {
            resolved.Version = client.FetchVersion(
                summary.Url, versionId, Path.Combine(metaDirectory, "version-" + versionId + ".json"));
        }
        catch (LauncherException ex)
        {
            resolved.Error = ex.Code;
            return resolved;
        }

        VersionDetail version = resolved.Version;
        int requiredJava = version.JavaVersion?.MajorVersion ?? 8;
        Report(boot, "  主类 " + version.MainClass + "；库 " + version.Libraries.Count
                     + " 个；要求 Java " + requiredJava + "；assets=" + version.Assets);

        JavaRuntimeResolver resolver = new JavaRuntimeResolver(
            new IJavaRuntimeProvider[] { new DetectedJavaRuntimeProvider() });

        JavaResolutionOutcome resolution = resolver.Resolve(
            new JavaSelectionRequest { RequiredMajorVersion = requiredJava });

        for (int i = 0; i < resolution.Notes.Count; i++)
        {
            Report(boot, "  " + resolution.Notes[i]);
        }

        if (!resolution.Succeeded)
        {
            Report(boot, "  找不到满足要求的 Java：" + (resolution.Selection.Explanation ?? string.Empty));
            resolved.Error = resolution.Selection.Error ?? ErrorCode.JavaNotFound;
            return resolved;
        }

        resolved.Java = resolution.Selection.Selected;
        Report(boot, "  选定 " + resolved.Java!.Describe() + " @ " + resolved.Java.ExecutablePath);

        if (version.AssetIndex?.Url != null)
        {
            Report(boot, "读取资源索引 " + version.AssetIndex.Id + "…");
            try
            {
                resolved.AssetIndex = client.FetchAssetIndex(
                    version.AssetIndex.Url!,
                    version.AssetIndex.Id,
                    Path.Combine(metaDirectory, "assets-" + version.AssetIndex.Id + ".json"));
            }
            catch (LauncherException ex)
            {
                resolved.Error = ex.Code;
                return resolved;
            }

            Report(boot, "  " + resolved.AssetIndex!.Count + " 个资源对象，合计 "
                         + Megabytes(resolved.AssetIndex.TotalSize));
        }

        return resolved;
    }

    private static DownloadPlan BuildDownloadPlan(BootContext boot, Resolved resolved)
    {
        EnvironmentProfile environment = PlatformProbe.ForJavaRuntime(resolved.Java);

        return new DownloadPlanBuilder().Build(resolved.Version!, environment, resolved.AssetIndex);
    }

    private static bool Download(BootContext boot, DownloadPlan plan)
    {
        Report(boot, "开始下载（并发 8）…");

        DownloadEngine engine = new DownloadEngine(new HttpTransport(), boot.Log);
        DownloadOptions options = new DownloadOptions
        {
            MaxConcurrency = 8,
            MaxAttempts = 3,
            MinimumFreeBytes = 128L * 1024 * 1024,
            BaseRetryDelay = TimeSpan.FromMilliseconds(400),
        };

        Progress<DownloadProgress> progress = new Progress<DownloadProgress>(p =>
        {
            if (p.FilesCompleted % 250 == 0 || p.FilesCompleted == p.FilesTotal)
            {
                Report(boot, "  进度 " + p.FilesCompleted + "/" + p.FilesTotal + "，已传 "
                             + Megabytes(p.BytesTransferred));
            }
        });

        DownloadReport report = engine.EnsureAll(plan, boot.Layout.CacheDirectory, options, progress, CancellationToken.None);

        Report(boot, "下载完成：新下 " + report.DownloadedCount + "，命中缓存 " + report.PresentCount
                     + "，失败 " + report.Failures.Count + "，传输 " + Megabytes(report.BytesTransferred));

        if (report.IsComplete)
        {
            return true;
        }

        for (int i = 0; i < report.Failures.Count; i++)
        {
            DownloadItemReport failure = report.Failures[i];
            Report(boot, "  失败 " + failure.Item.Describe() + " → " + ErrorCodes.Id(failure.Error ?? ErrorCode.DlFailed)
                         + "（尝试 " + failure.Attempts + " 次）");
        }

        return false;
    }

    /// <summary>
    /// 等游戏建出主窗口。这是"真的起来了"最直接的证据——
    /// 进程活着只说明 JVM 没退，有窗口才说明游戏跑到了界面。
    /// </summary>
    private static int AwaitWindow(BootContext boot, GameProcess process, int holdSeconds, bool keepRunning)
    {
        Report(boot, "已拉起进程 pid=" + process.ProcessId + "，等待游戏窗口…");

        Stopwatch watch = Stopwatch.StartNew();
        TimeSpan windowTimeout = TimeSpan.FromSeconds(180);

        while (watch.Elapsed < windowTimeout)
        {
            if (process.HasMainWindow)
            {
                break;
            }

            if (process.HasExited)
            {
                break;
            }

            Thread.Sleep(500);
        }

        if (process.HasMainWindow)
        {
            Report(boot, "✔ 游戏窗口已出现：" + (process.MainWindowTitle ?? "(无标题)")
                         + "，用时 " + watch.Elapsed.TotalSeconds.ToString("F1", CultureInfo.InvariantCulture) + " 秒");

            if (holdSeconds > 0)
            {
                Thread.Sleep(TimeSpan.FromSeconds(holdSeconds));
            }

            if (keepRunning)
            {
                Report(boot, "按 --keep 要求保留进程运行（pid=" + process.ProcessId + "）");
                return ExitOk;
            }

            process.Kill();
            process.WaitForExit(TimeSpan.FromSeconds(10));
            Report(boot, "已结束进程（验收完成）");
            return ExitOk;
        }

        process.WaitForExit(TimeSpan.FromSeconds(20));
        GameProcessResult result = process.WaitForExit();

        Report(boot, "✘ 未见游戏窗口；进程已退出，退出码 " + result.ExitCode
                     + "，错误码 " + ErrorCodes.Id(result.Error ?? ErrorCode.None));
        Report(boot, "游戏日志尾部（" + result.LogFilePath + "）：");

        foreach (string line in Tail(result.LogFilePath, 40))
        {
            Report(boot, "  | " + line);
        }

        return ExitFailed;
    }

    private static IReadOnlyList<string> Tail(string logFilePath, int lines)
    {
        try
        {
            if (!File.Exists(logFilePath))
            {
                return new[] { "(日志不存在)" };
            }

            string[] all = File.ReadAllLines(logFilePath);
            int from = Math.Max(0, all.Length - lines);

            List<string> tail = new List<string>(all.Length - from);
            for (int i = from; i < all.Length; i++)
            {
                tail.Add(all[i]);
            }

            return tail;
        }
        catch (IOException)
        {
            return new[] { "(日志读取失败)" };
        }
    }

    // ---------- 输出 ----------

    private static void Report(BootContext boot, string message)
    {
        boot.Log.Info("cli", message);

        try
        {
            Console.Out.WriteLine(message);
            Console.Out.Flush();
        }
        catch (IOException)
        {
        }
        catch (ObjectDisposedException)
        {
        }
    }

    private static int Fail(BootContext boot, ErrorCode code)
    {
        Report(boot, "✘ 失败：" + ErrorCodes.Id(code) + " " + ErrorCodes.Hint(code));
        return ExitFailed;
    }

    private static int Usage(BootContext boot)
    {
        Report(boot, "用法：");
        Report(boot, "  plan    <版本 id>                              只算下载计划，不下载");
        Report(boot, "  install <版本 id>                              下载该版本所需全部文件");
        Report(boot, "  launch  <版本 id> [--dry-run] [--skip-download]");
        Report(boot, "                    [--offline-name 名字] [--memory MB]");
        Report(boot, "                    [--hold 秒] [--keep]");
        return ExitUsage;
    }

    // ---------- 参数 ----------

    private static string Require(IReadOnlyList<string> args, int index)
    {
        return args.Count > index ? args[index].Trim() : string.Empty;
    }

    private static bool HasFlag(IReadOnlyList<string> args, string name)
    {
        for (int i = 0; i < args.Count; i++)
        {
            if (string.Equals(args[i], name, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }

        return false;
    }

    private static string OptionValue(IReadOnlyList<string> args, string name, string fallback)
    {
        for (int i = 0; i < args.Count - 1; i++)
        {
            if (string.Equals(args[i], name, StringComparison.OrdinalIgnoreCase))
            {
                return args[i + 1];
            }
        }

        return fallback;
    }

    private static int ParseInt(string text, int fallback)
    {
        return int.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out int value) && value > 0
            ? value
            : fallback;
    }

    private static string Megabytes(long bytes)
    {
        double mb = bytes / 1024.0 / 1024.0;
        return mb.ToString("F1", CultureInfo.InvariantCulture) + " MB";
    }
}
