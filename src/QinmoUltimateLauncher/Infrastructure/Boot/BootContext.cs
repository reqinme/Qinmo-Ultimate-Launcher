using System;
using System.IO;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Configuration;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.IO;

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

    public void Dispose()
    {
        Log.Info("boot", "session ended");
        Log.Dispose();
    }
}
