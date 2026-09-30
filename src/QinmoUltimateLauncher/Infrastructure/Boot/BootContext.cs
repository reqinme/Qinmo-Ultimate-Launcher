using System;
using System.IO;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Configuration;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.IO;
using Qul.Infrastructure.Net;
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
        ErrorCode? dataRootWarning,
        UpdateStore updates,
        bool adoptedPendingUpdate)
    {
        _adoptedPendingUpdate = adoptedPendingUpdate;
        Layout = layout;
        Log = log;
        ConfigResult = configResult;
        ConfigStore = configStore;
        DataRootWarning = dataRootWarning;
        _updates = updates;

        // **代理设置必须在任何请求之前注入。**
        // 配置里选"直连"或填手动代理时，不注入就等于静默忽略用户的选择——
        // 先前正是如此：三态读写都正常，却没有任何调用点把它交给请求。
        HttpTransport.ConfigureProxy(
            configResult.Config.Network.ProxyMode, configResult.Config.Network.ProxyAddress);
    }

    private readonly UpdateStore _updates;
    private readonly bool _adoptedPendingUpdate;

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
        UpdateStore updates = new UpdateStore(layout.UpdatesDirectory, log);
        bool adopted = HandlePendingUpdate(layout, log, updates);

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

        return new BootContext(layout, log, result, store, layoutError?.Code, updates, adopted);
    }

    /// <summary>
    /// 更新标记的状态机。
    ///
    /// 三种情况必须分开：我们就是刚上来的新版本（正常，标记为待确认）；
    /// 上次启动打过待确认标记却没确认健康（回滚）；替换根本没发生（清掉残留标记）。
    /// </summary>
    private static bool HandlePendingUpdate(DataLayout layout, SessionLog log, UpdateStore updates)
    {
        string currentVersion = typeof(BootContext).Assembly.GetName().Version?.ToString() ?? "0.0.0";

        UpdateBootDecision decision = updates.DecideOnBoot(currentVersion, out PendingUpdate? pending);

        switch (decision)
        {
            case UpdateBootDecision.None:
                return false;

            case UpdateBootDecision.Adopt:
                log.Info("update", "adopted pending update " + pending!.TargetVersion + "; awaiting health confirmation");
                return true;

            case UpdateBootDecision.Discard:
                log.Info("update", "pending marker discarded; the swap never happened");
                updates.ClearPending();
                return false;

            default:
                RollBackToPreviousVersion(layout, log, updates, pending!);
                return false;
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

        // **不要把方向说反。** pending.TargetVersion 是那个**没通过健康确认的新版本**，
// 而这里要做的是把它换回上一版——先前写成 "rolling back to <新版本>"，
// 读日志的人会得出完全相反的结论。
                log.Warn(
                    "update",
                    "previous update ("
                    + pending.TargetVersion
                    + ") never confirmed health; rolling back to the previous version",
                    ErrorCode.UpdFailed);

        SelfReplaceOutcome outcome = SelfReplacer.Rollback(selfPath, backupPath, selfPath + ".failed");

        if (outcome.Succeeded)
        {
            log.Info("update", "rolled back; the previous version will run on the next start");

            // **只有真的回退成功才清标记。**
            updates.ClearPending();
            return;
        }

        // **回退失败时绝不能清标记。**
        //
        // 这里原本写的是"无论成败都清掉，留着只会让每次启动都重复同一次失败的尝试"——
        // 那个理由是想当然的：回退失败通常是因为备份被杀软或只读目录挡住，
        // 清掉标记等于**把可恢复变成不可恢复**：用户拿到一个打不开的启动器，
        // 而自愈再也不会发生。重试一次文件移动的代价远小于永久损坏。
        //
        // 下次启动会再试一次；真的试不动时，用户至少还有 backup 文件可手动换回。
        log.Failure(
            "update",
            ErrorCode.UpdFailed,
            new LauncherException(ErrorCode.UpdFailed, outcome.FailureReason ?? "rollback failed"));

        log.Warn("update", "rollback failed; the marker is kept so the next start retries", ErrorCode.UpdFailed);
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

    /// <summary>
    /// 应用真正起来之后调用。**删掉标记才算这次更新活下来了。**
    /// 不调用的话，下次启动会把"已启动但未确认"当成失败并回滚——
    /// 那等于每次更新都会在第二次启动时自己撤销。
    /// </summary>
    public void ConfirmUpdateHealthy()
    {
        // **只有"我们就是被换上来的那个版本"时，确认才有意义。**
        // 执行替换的那个旧进程不能替新版本确认健康——它马上就退出了，
        // 新版本一次都还没跑过。放开这个口子，等于每次更新都跳过试运行。
        if (_adoptedPendingUpdate)
        {
            _updates.ClearPending();
        }
    }

    public void Dispose()
    {
        Log.Info("boot", "session ended");
        Log.Dispose();
    }
}
