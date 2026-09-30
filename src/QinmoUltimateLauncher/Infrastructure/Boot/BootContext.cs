using System;
using System.IO;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Configuration;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.IO;
using Qul.Infrastructure.Update;
using Qul.Application.Update;

namespace Qul.Infrastructure.Boot;

/// <summary>
/// 启动上下文：把"解析目录 → 建目录 → 开日志 → 读配置 → 应用日志级别"这条引导链收在一处。
/// 与 WPF 无关，因此可被测试直接调用；App 只负责把异常接住并展示。
/// </summary>
public sealed class BootContext : IDisposable
{
    private BootContext(
        DataLayout layout,
        SessionLog log,
        ConfigLoadResult configResult,
        ConfigStore configStore,
        ErrorCode? dataRootWarning)
    {
        Layout = layout;
        Log = log;
        ConfigResult = configResult;
        ConfigStore = configStore;
        DataRootWarning = dataRootWarning;
    }

    public DataLayout Layout { get; }

    public SessionLog Log { get; }

    public ConfigLoadResult ConfigResult { get; }

    public ConfigStore ConfigStore { get; }

    public LauncherConfig Config => ConfigResult.Config;

    /// <summary>目录创建阶段遇到的非致命错误。非 null 表示已降级运行（例如回退到用户目录）。</summary>
    public ErrorCode? DataRootWarning { get; }

    public static BootContext Start()
    {
        string executableDirectory = AppDomain.CurrentDomain.BaseDirectory;
        string userProfile = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);

        if (string.IsNullOrWhiteSpace(userProfile))
        {
            userProfile = Path.GetTempPath();
        }

        DataLayout layout = DataLayout.Resolve(executableDirectory, userProfile);
        LauncherException? layoutError = layout.EnsureCreated();

        SessionLog log = SessionLog.Open(layout, LogLevel.Info);
        Redactor.RegisterPathPrefix(userProfile, isDataRoot: false);
        Redactor.RegisterPathPrefix(layout.DataRoot, isDataRoot: true);

        log.Info("boot", "session started");
        log.Info("config", "data root placement=" + layout.Placement.ToString().ToLowerInvariant(), layout.DataRoot);

        if (layoutError != null)
        {
            log.Failure("boot", layoutError.Code, layoutError);
        }

        // 处理上次更新留下的标记。**必须放在这里**：任何可能失败的初始化之前。
        // 换上来的坏版本如果连窗口都建不出来，这就是唯一的自救机会。
        HandlePendingUpdate(layout, log);

        ConfigStore store = new ConfigStore(layout);
        ConfigLoadResult result = store.Load();

        if (result.Warning.HasValue)
        {
            log.Warn("config", "config load warning", result.Warning.Value, result.BackupPath);
        }

        // 配置里的日志级别在装载后才生效——引导期的记录一律按 info 兜底。
        log.Minimum = result.Config.Diagnostics.LogLevel;

        if (result.WasReset && !result.IsReadOnly)
        {
            // 损坏配置已备份，立刻落一份干净默认值，免得用户下次启动再看到同一个警告。
            LauncherException? saveError = store.Save(result.Config);
            if (saveError != null)
            {
                log.Failure("config", saveError.Code, saveError);
            }
        }

        return new BootContext(layout, log, result, store, layoutError?.Code);
    }

    /// <summary>
    /// 更新标记的状态机。
    ///
    /// 三种情况必须分开：我们就是刚上来的新版本（正常，标记为待确认）；
    /// 上次启动打过待确认标记却没确认健康（回滚）；替换根本没发生（清掉残留标记）。
    /// </summary>
    private static void HandlePendingUpdate(DataLayout layout, SessionLog log)
    {
        UpdateStore updates = new UpdateStore(layout.UpdatesDirectory, log);
        string currentVersion = typeof(BootContext).Assembly.GetName().Version?.ToString() ?? "0.0.0";

        UpdateBootDecision decision = updates.DecideOnBoot(currentVersion, out PendingUpdate? pending);

        switch (decision)
        {
            case UpdateBootDecision.None:
                return;

            case UpdateBootDecision.Adopt:
                log.Info("update", "adopted pending update " + pending!.TargetVersion + "; awaiting health confirmation");
                return;

            case UpdateBootDecision.Discard:
                log.Info("update", "pending marker discarded; the swap never happened");
                updates.ClearPending();
                return;

            case UpdateBootDecision.Rollback:
                RollBackToPreviousVersion(layout, log, updates, pending!);
                return;
        }
    }

    private static void RollBackToPreviousVersion(
        DataLayout layout,
        SessionLog log,
        UpdateStore updates,
        PendingUpdate pending)
    {
        string selfPath = CurrentExecutablePath();
        string programDirectory = Path.GetDirectoryName(selfPath) ?? layout.ExecutableDirectory;
        string backupPath = updates.ResolveBackupPath(programDirectory, pending.BackupFileName);

        log.Warn("update", "previous update never confirmed health; rolling back to " + pending.TargetVersion, ErrorCode.UpdFailed);

        SelfReplaceOutcome outcome = SelfReplacer.Rollback(selfPath, backupPath, selfPath + ".failed");

        if (outcome.Succeeded)
        {
            log.Info("update", "rolled back; the previous version will run on the next start");
        }
        else
        {
            log.Failure("update", ErrorCode.UpdFailed, new LauncherException(ErrorCode.UpdFailed, outcome.FailureReason ?? "rollback failed"));
        }

        // 无论成败都清掉标记：留着只会让每次启动都重复同一次失败的尝试。
        updates.ClearPending();
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

    public void Dispose()
    {
        Log.Info("boot", "session ended");
        Log.Dispose();
    }
}
