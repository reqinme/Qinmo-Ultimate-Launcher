using System;
using System.Collections.Generic;
using System.IO;
using Qul.Application.Launch;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Domain.Launch;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.Processes;
using Qul.Infrastructure.Runtime;

namespace Qul.Infrastructure.Launch;

public sealed class GameLaunchRequest
{
    public LaunchPlanRequest Plan { get; set; } = new LaunchPlanRequest();

    /// <summary>解压前是否清空 natives 目录。按版本隔离时清空是安全的，也能避免上次的残留。</summary>
    public bool CleanNatives { get; set; } = true;

    /// <summary>游戏 stdout/stderr 落盘位置。为空则不记录——但输出是排障时最有用的东西。</summary>
    public string? GameLogPath { get; set; }
}

public sealed class GameLaunchOutcome
{
    public GameLaunchOutcome(
        LaunchPlan? plan,
        NativeExtractionResult? natives,
        GameProcess? process,
        ErrorCode? error,
        IReadOnlyList<string> notes)
    {
        Plan = plan;
        Natives = natives;
        Process = process;
        Error = error;
        Notes = notes;
    }

    public LaunchPlan? Plan { get; }

    public NativeExtractionResult? Natives { get; }

    public GameProcess? Process { get; }

    public ErrorCode? Error { get; }

    public IReadOnlyList<string> Notes { get; }

    public bool Started => Process != null && Error == null;

    public bool Prepared => Plan != null && Error == null;
}

/// <summary>
/// 启动用例：组装计划 → 解压 natives → 拉起进程。
///
/// 放在基础设施层而不是应用层：它编排的是"文件落位"与"起进程"这两件纯机制的事，
/// 需要 <c>System.IO</c> 与 <c>System.Diagnostics</c>；纯策略部分（计划组装）已经在应用层。
/// </summary>
public sealed class GameLauncher
{
    private readonly SessionLog _log;

    public GameLauncher(SessionLog? log = null)
    {
        _log = log ?? SessionLog.Null;
    }

    /// <summary>只准备不启动。用于预演、排障，以及在真的下载之前先把计划算清楚。</summary>
    public GameLaunchOutcome Prepare(GameLaunchRequest request)
    {
        if (request == null)
        {
            throw new ArgumentNullException(nameof(request));
        }

        List<string> notes = new List<string>();

        LaunchPlan plan;
        try
        {
            plan = LaunchPlanBuilder.Build(request.Plan);
        }
        catch (LauncherException ex)
        {
            _log.Failure("plan", ex.Code, ex);
            return new GameLaunchOutcome(null, null, null, ex.Code, notes);
        }

        notes.Add("骨架指纹 " + plan.Skeleton.ComputeHash().Substring(0, 12));
        notes.Add("类路径 " + plan.Skeleton.ClassPathEntries.Count + " 项");

        // 客户端 jar 缺失时给一句人话，而不是让 JVM 抛 ClassNotFoundException。
        ErrorCode? missingClient = CheckClientJar(request.Plan, plan);
        if (missingClient.HasValue)
        {
            return new GameLaunchOutcome(plan, null, null, missingClient, notes);
        }

        NativeExtractionResult natives = NativeExtractor.Extract(
            request.Plan.Version,
            request.Plan.Environment,
            request.Plan.CacheRoot,
            request.Plan.NativesDirectory,
            cleanTarget: request.CleanNatives);

        notes.Add("natives 解压 " + natives.ArchivesExtracted + " 个包 / " + natives.FilesExtracted + " 个文件");

        if (natives.MissingArchives.Count > 0)
        {
            notes.Add("缺少 " + natives.MissingArchives.Count + " 个本地库包（下载步骤未完成？）");
        }

        if (natives.RejectedArchives.Count > 0)
        {
            _log.Warn("natives", "archives rejected by path policy", ErrorCode.ZipEntryEscape);

            return new GameLaunchOutcome(
                plan,
                natives,
                null,
                ErrorCode.ZipEntryEscape,
                notes);
        }

        if (natives.Error.HasValue)
        {
            _log.Failure("natives", natives.Error.Value);

            return new GameLaunchOutcome(plan, natives, null, natives.Error, notes);
        }

        return new GameLaunchOutcome(plan, natives, null, null, notes);
    }

    /// <summary>准备并拉起。返回时进程已在运行（或带回了失败原因）。</summary>
    public GameLaunchOutcome Launch(GameLaunchRequest request)
    {
        GameLaunchOutcome prepared = Prepare(request);

        if (!prepared.Prepared || prepared.Plan == null)
        {
            return prepared;
        }

        List<string> notes = new List<string>(prepared.Notes);

        List<string> arguments = new List<string>();
        arguments.AddRange(prepared.Plan.ResolveJvmArguments());
        arguments.Add(prepared.Plan.Skeleton.MainClass);
        arguments.AddRange(prepared.Plan.ResolveGameArguments());

        notes.Add("命令行 " + arguments.Count + " 个参数");

        GameProcessOptions options = new GameProcessOptions
        {
            ExecutablePath = prepared.Plan.Secrets.JavaExecutablePath,
            Arguments = arguments,
            WorkingDirectory = prepared.Plan.Secrets.WorkingDirectory,
            Environment = request.Plan.EnvironmentVariables,
            LogFilePath = request.GameLogPath ?? string.Empty,
        };

        if (!GameProcess.TryStart(options, _log, out GameProcess? process, out ErrorCode error))
        {
            return new GameLaunchOutcome(prepared.Plan, prepared.Natives, null, error, notes);
        }

        notes.Add("游戏日志 " + (string.IsNullOrEmpty(request.GameLogPath) ? "未记录" : request.GameLogPath!));

        return new GameLaunchOutcome(prepared.Plan, prepared.Natives, process, null, notes);
    }

    private static ErrorCode? CheckClientJar(LaunchPlanRequest request, LaunchPlan plan)
    {
        IReadOnlyList<string> classPath = plan.Skeleton.ClassPathEntries;
        if (classPath.Count == 0)
        {
            return ErrorCode.MetaVersionInvalid;
        }

        string clientRelative = classPath[classPath.Count - 1];
        string clientAbsolute = Path.Combine(
            request.CacheRoot.Replace('/', '\\'),
            clientRelative.Replace('/', '\\'));

        return File.Exists(clientAbsolute) ? (ErrorCode?)null : ErrorCode.DlFailed;
    }
}
