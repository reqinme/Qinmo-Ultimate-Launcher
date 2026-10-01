using Qul.Infrastructure.Diagnostics;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Threading;
using Qul.Application.Diagnostics;
using Qul.Application.Launch;
using Qul.Application.Update;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Infrastructure.Boot;
using Qul.Infrastructure.Launch;
using Qul.Infrastructure.Net;
using Qul.Infrastructure.Processes;
using Qul.Infrastructure.Update;

namespace Qul.Infrastructure.Cli;

/// <summary>
/// 无界面驱动整条链路。
///
/// 它不自己实现链路，而是调用 <see cref="LaunchPipeline"/>——
/// **命令行与界面必须走同一条链路**，否则迟早出现"命令行能启动、界面不能"这种最难查的问题。
/// </summary>
public static partial class CliRunner
{
    public const int ExitOk = 0;
    public const int ExitFailed = 1;
    public const int ExitUsage = 2;

    /// <summary>
    /// 用户取消（130 = 128 + SIGINT，shell 惯例）。
    ///
    /// **刻意不并进 ExitFailed**：取消不是失败。脚本里
    /// <c>if ($LASTEXITCODE -ne 0)</c> 会把两者混为一谈，
    /// 而它们的处置完全不同——失败要排查，取消只要重跑。
    /// </summary>
    public const int ExitCancelled = 130;

    public static int Run(BootContext boot, IReadOnlyList<string> args)
    {
        if (boot == null)
        {
            throw new ArgumentNullException(nameof(boot));
        }

        // **启动提示必须先说。**
        // 数据根建不出来时日志本身也写不出来，命令行用户同样是"完全不知情"。
        // 走 stderr 而不是 stdout：脚本解析 stdout 时不会被这句话干扰。
        string startupNotice = StartupNotices.Build(boot.DataRootWarning, boot.UpdateNotice);

        if (startupNotice.Length > 0)
        {
            Console.Error.WriteLine(startupNotice);
        }

        if (args == null || args.Count == 0)
        {
            return Usage(boot);
        }

        switch (args[0].Trim().ToLowerInvariant())
        {
            case "plan":
                return Execute(boot, args, LaunchPipelineMode.PlanOnly);
            case "install":
                return Execute(boot, args, LaunchPipelineMode.Install);
            case "launch":
                return Execute(boot, args, LaunchPipelineMode.Launch);
            case "preflight":
                return Preflight(boot, args);
            case "account":
                return Account(boot, args);
            case "update":
                return Update(boot, args);
            default:
                return Usage(boot);
        }
    }

    // ---------- 命令 ----------

    /// <summary>
    /// 最大内存：<c>--memory</c> 优先；没给就用配置里的 <c>memory.maxMb</c>；都没有才 2048。
    ///
    /// **配置必须排在这个顺序里**，否则 `config.json` 里设了内存的用户会以为它生效了。
    /// </summary>
    private static int ResolveMaxMemory(BootContext boot, IReadOnlyList<string> args)
    {
        string? text = OptionValue(args, "--memory", null);

        if (!string.IsNullOrWhiteSpace(text))
        {
            return ParseInt(text, 2048);
        }

        return boot.Config.Memory.MaxMb ?? 2048;
    }
    private static int Execute(BootContext boot, IReadOnlyList<string> args, LaunchPipelineMode requested)
    {
        string versionId = Require(args, 1);
        if (versionId.Length == 0)
        {
            return Usage(boot);
        }

        LaunchPipelineMode mode = requested;

        if (requested == LaunchPipelineMode.Launch && HasFlag(args, "--dry-run"))
        {
            mode = LaunchPipelineMode.Prepare;
        }

        LaunchPipelineRequest request = new LaunchPipelineRequest
        {
            VersionId = versionId,
            Mode = mode,
            IdentitySource = IdentitySource.Offline,
            OfflineUserName = OptionValue(args, "--offline-name", "Player") ?? "Player",
            MaxMemoryMb = ResolveMaxMemory(boot, args),
            ServerTarget = OptionValue(args, "--server", null),
        };

        LaunchPipelineResult result;

        using (CancellationTokenSource source = new CancellationTokenSource())
        {
            ConsoleCancelEventHandler handler = (sender, e) =>
            {
                // **Ctrl+C 应当优雅取消，而不是让进程直接消失。**
                //
                // 先前这里传的是 CancellationToken.None：命令行用户按一下 Ctrl+C，
                // 进程立刻消失——日志没有收尾，已下到一半的 .qulpart 也没人管，
                // 而它本来是**可续传**的。account login 早就这么处理了，install/launch 却漏了。
                e.Cancel = true;
                source.Cancel();
            };

            Console.CancelKeyPress += handler;

            try
            {
                result = new LaunchPipeline(boot).Run(request, new ConsoleProgress(boot), source.Token);
            }
            finally
            {
                Console.CancelKeyPress -= handler;
            }
        }

        if (result.Cancelled)
        {
            // **取消不是失败**，退出码也分开：130 = 128 + SIGINT，是 shell 的惯例。
            // 报"已下载的部分还在"是有用的信息——下次运行不会从零开始。
            Report(boot, "已取消。已下载的部分留在缓存里，下次运行会接着下。");
            return ExitCancelled;
        }

        if (!result.Succeeded)
        {
            return Fail(boot, result.Error ?? ErrorCode.DlFailed, result.ErrorDetail);
        }

        if (mode == LaunchPipelineMode.PlanOnly)
        {
            PrintPlan(boot, result.Plan!);
            return ExitOk;
        }

        if (mode == LaunchPipelineMode.Install || mode == LaunchPipelineMode.Prepare)
        {
            Report(boot, mode == LaunchPipelineMode.Install ? "安装完成。" : "预演完成（未启动进程）。");
            return ExitOk;
        }

        GameProcess? process = result.Process;
        if (process == null)
        {
            return Fail(boot, ErrorCode.ProcStartFailed, "进程未能拉起。");
        }

        return AwaitWindow(boot, process, ParseInt(OptionValue(args, "--hold", "5"), 5), HasFlag(args, "--keep"));
    }

    /// <summary>
    /// 检查 / 下载 / 应用更新。无界面路径，便于自动化验证整条链路。
    /// 不加 --apply 时只下载校验，**绝不碰主程序**。
    /// </summary>
    private static int Update(BootContext boot, IReadOnlyList<string> args)
    {
        string manifestUrl = OptionValue(args, "--manifest", null) ?? string.Empty;

        if (manifestUrl.Length == 0)
        {
            Report(boot, "用法：update --manifest <发布清单地址> [--apply]");
            return ExitUsage;
        }

        bool apply = HasFlag(args, "--apply");
        string currentVersion = typeof(CliRunner).Assembly.GetName().Version?.ToString() ?? "0.0.0";
        Report(boot, "当前版本 " + currentVersion);

        UpdateStore store = new UpdateStore(boot.Layout.UpdatesDirectory, boot.Log);
        UpdateService service = new UpdateService(new HttpTransport(), store, boot.Log);

        UpdateRelease? release;
        try
        {
            release = service.Check(manifestUrl, currentVersion, CancellationToken.None);
        }
        catch (LauncherException ex)
        {
            return Fail(boot, ex.Code, ex.Message);
        }

        if (release == null)
        {
            Report(boot, "已是最新版本。");
            return ExitOk;
        }

        Report(boot, "发现新版本 " + release.Version + "（" + Megabytes(release.SizeBytes) + "）");

        if (!string.IsNullOrWhiteSpace(release.Notes))
        {
            Report(boot, "  说明：" + release.Notes);
        }

        string pendingPath;
        try
        {
            pendingPath = service.Download(release, CancellationToken.None);
        }
        catch (LauncherException ex)
        {
            return Fail(boot, ex.Code, ex.Message);
        }

        Report(boot, "已下载并校验通过：" + pendingPath);

        if (!apply)
        {
            Report(boot, "（未加 --apply，不执行替换）");
            return ExitOk;
        }

        string selfPath = CurrentExecutablePath();
        SelfReplaceOutcome outcome = service.Apply(
            selfPath, pendingPath, release, Path.GetFileName(selfPath) + ".old");

        if (!outcome.Succeeded)
        {
            return Fail(boot, ErrorCode.UpdFailed, outcome.FailureReason);
        }

        Report(boot, "替换完成，重启后生效。");
        return ExitOk;
    }

    private static string CurrentExecutablePath()
    {
        try
        {
            return System.Diagnostics.Process.GetCurrentProcess().MainModule?.FileName
                   ?? Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "QinmoUltimateLauncher.exe");
        }
        catch (Exception ex) when (ex is InvalidOperationException || ex is NotSupportedException)
        {
            return Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "QinmoUltimateLauncher.exe");
        }
    }

    private static int Preflight(BootContext boot, IReadOnlyList<string> args)
    {
        string versionId = Require(args, 1);
        if (versionId.Length == 0)
        {
            return Usage(boot);
        }

        IReadOnlyList<PreflightItem> items = new LaunchPipeline(boot).PreparePreflight(
            new LaunchPipelineRequest
            {
                VersionId = versionId,
                IdentitySource = IdentitySource.Offline,
                ServerTarget = OptionValue(args, "--server", null),
            });

        Report(boot, "启动前预检（" + items.Count + " 项）：");

        for (int i = 0; i < items.Count; i++)
        {
            PreflightItem item = items[i];
            Report(boot, "  [" + item.Scope + "/" + item.Severity + "] " + item.Id + " " + item.Title);

            string[] detailLines = item.Detail.Split('\n');
            for (int j = 0; j < detailLines.Length; j++)
            {
                Report(boot, "      " + detailLines[j]);
            }
        }

        return ExitOk;
    }

    private static void PrintPlan(BootContext boot, DownloadPlan plan)
    {
        Report(boot, "下载计划：" + plan.Items.Count + " 项，已知体积 " + Megabytes(plan.KnownTotalBytes)
                     + "，重复路径 " + plan.DuplicatePaths.Count + "，内容冲突 " + plan.ConflictingPaths.Count);

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
    }

    /// <summary>
    /// 等游戏建出主窗口。这是"真的起来了"最直接的证据——
    /// 进程活着只说明 JVM 没退，有窗口才说明游戏跑到了界面。
    /// </summary>
    private static int AwaitWindow(BootContext boot, GameProcess process, int holdSeconds, bool keepRunning)
    {
        Report(boot, "等待游戏窗口…");

        Stopwatch watch = Stopwatch.StartNew();
        TimeSpan windowTimeout = TimeSpan.FromSeconds(180);

        while (watch.Elapsed < windowTimeout && !process.HasMainWindow && !process.HasExited)
        {
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

        // 绝不能用无超时的 WaitForExit：游戏还活着的话这里会永远卡住。
        // 26.3 那次就是这样——命令行一直挂到被外部强杀。
        if (!process.WaitForExit(TimeSpan.FromSeconds(20)))
        {
            Report(boot, "游戏进程仍在运行，但未检测到窗口；不再等待。");
            return ExitFailed;
        }

        GameProcessResult result = process.WaitForExit();

        Report(boot, "✘ 未见游戏窗口；进程已退出，退出码 " + result.ExitCode
                     + "，错误码 " + ErrorCodes.Id(result.Error ?? ErrorCode.None));
        Report(boot, "游戏日志尾部：" + result.LogFilePath);

        foreach (string line in Tail(result.LogFilePath, 40))
        {
            Report(boot, "  | " + line);
        }

        // **崩溃分析：游戏自己写的报告比我们tail出来的日志更直接。**
        //
        // 放在这里是因为**只有这里同时具备两个条件**：既知道"进程确实退出了"，
        // 又拿得到**真实退出码**。接在别处（例如"进程未能拉起"）时游戏根本没运行过，
        // 不可能有报告——那样接等于永远不触发。
        string? crashPath = CrashReportLocator.FindNewest(boot.Layout.GameRoot, DateTimeOffset.UtcNow.AddMinutes(-30));
        CrashFinding? finding = CrashAnalysis.Analyze(CrashReportLocator.Read(crashPath), result.ExitCode);

        if (finding != null)
        {
            Report(boot, string.Empty);
            Report(boot, "崩溃分析：" + finding.Summary);

            if (crashPath != null)
            {
                Report(boot, "  报告：" + crashPath);
            }

            for (int i = 0; i < finding.Suggestions.Count; i++)
            {
                Report(boot, "  " + (i + 1).ToString(System.Globalization.CultureInfo.InvariantCulture)
                             + ") " + finding.Suggestions[i]);
            }
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

    private static int Fail(BootContext boot, ErrorCode code, string? detail)
    {
        Report(boot, "✘ 失败：" + ErrorCodes.Id(code) + " " + ErrorCodes.Hint(code));

        if (!string.IsNullOrWhiteSpace(detail))
        {
            Report(boot, "   " + detail);
        }

        return ExitFailed;
    }

    private static int Usage(BootContext boot)
    {
        Report(boot, "用法：");
        Report(boot, "  plan      <版本 id> [--server 地址]      只算下载计划，不下载");
        Report(boot, "  preflight <版本 id> [--server 地址]      只跑启动前预检，不联网");
        Report(boot, "  install   <版本 id>                      下载该版本所需全部文件");
        Report(boot, "  account   [status|signout|login]          查看账户 / 登出 / 微软正版登录");
        Report(boot, "  update    --manifest <地址> [--apply]     检查/下载更新（--apply 才替换主程序）");
        Report(boot, "  launch    <版本 id> [--dry-run]          下载并启动（--dry-run 只准备不启动）");
        Report(boot, "            [--offline-name 名字] [--memory MB]");
        Report(boot, "            [--hold 秒] [--keep] [--server 地址]");
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

    private static string? OptionValue(IReadOnlyList<string> args, string name, string? fallback)
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

    private static int ParseInt(string? text, int fallback)
    {
        return int.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out int value) && value > 0
            ? value
            : fallback;
    }

    private static string Megabytes(long bytes)
    {
        return (bytes / 1024.0 / 1024.0).ToString("F1", CultureInfo.InvariantCulture) + " MB";
    }

    /// <summary>
    /// 同步转发进度。用 Progress&lt;T&gt; 会异步投递到线程池，
    /// 命令行输出就会交错错位——这里必须同步。
    /// </summary>
    private sealed class ConsoleProgress : IProgress<LaunchProgress>
    {
        private readonly BootContext _boot;
        private LaunchStage _stage = LaunchStage.Idle;

        public ConsoleProgress(BootContext boot)
        {
            _boot = boot;
        }

        public void Report(LaunchProgress value)
        {
            // 阶段切换打一行分隔，否则几百行进度会糊成一片。
            if (value.Stage != _stage)
            {
                _stage = value.Stage;
                CliRunner.Report(_boot, "  [" + LaunchStages.Label(value.Stage) + "]");
            }

            // 带总量的进度行太密，命令行里只保留阶段分隔。
            if (value.Total > 0)
            {
                return;
            }

            CliRunner.Report(_boot, "    " + value.Message);
        }
    }
}
