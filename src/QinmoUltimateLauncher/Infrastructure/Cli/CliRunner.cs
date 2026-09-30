using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Threading;
using Qul.Application.Diagnostics;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Infrastructure.Boot;
using Qul.Infrastructure.Launch;
using Qul.Infrastructure.Processes;

namespace Qul.Infrastructure.Cli;

/// <summary>
/// 无界面驱动整条链路。
///
/// 它不自己实现链路，而是调用 <see cref="LaunchPipeline"/>——
/// **命令行与界面必须走同一条链路**，否则迟早出现"命令行能启动、界面不能"这种最难查的问题。
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
            default:
                return Usage(boot);
        }
    }

    // ---------- 命令 ----------

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

        LaunchPipelineResult result = new LaunchPipeline(boot).Run(
            new LaunchPipelineRequest
            {
                VersionId = versionId,
                Mode = mode,
                IdentitySource = IdentitySource.Offline,
                OfflineUserName = OptionValue(args, "--offline-name", "Player") ?? "Player",
                MaxMemoryMb = ParseInt(OptionValue(args, "--memory", "2048"), 2048),
                ServerTarget = OptionValue(args, "--server", null),
            },
            new ConsoleProgress(boot),
            CancellationToken.None);

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

        process.WaitForExit(TimeSpan.FromSeconds(20));
        GameProcessResult result = process.WaitForExit();

        Report(boot, "✘ 未见游戏窗口；进程已退出，退出码 " + result.ExitCode
                     + "，错误码 " + ErrorCodes.Id(result.Error ?? ErrorCode.None));
        Report(boot, "游戏日志尾部：" + result.LogFilePath);

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
    private sealed class ConsoleProgress : IProgress<string>
    {
        private readonly BootContext _boot;

        public ConsoleProgress(BootContext boot)
        {
            _boot = boot;
        }

        public void Report(string value)
        {
            CliRunner.Report(_boot, "  " + value);
        }
    }
}
