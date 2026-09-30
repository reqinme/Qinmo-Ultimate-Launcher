using System;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Threading;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Boot;
using Qul.Infrastructure.Diagnostics;

namespace Qul;

/// <summary>
/// 应用入口。只做三件事：引导、接异常、兜底展示。
/// 业务逻辑一律不放这里——WPF 入口类不可测，任何决策写进来就等于放弃测试。
///
/// 基类必须写全限定名：本程序集存在 <c>Qul.Application</c> 命名空间（应用层），
/// 在 <c>Qul</c> 命名空间内它会遮蔽 <c>System.Windows.Application</c>。
/// </summary>
public partial class App : System.Windows.Application
{
    private SessionLog _log = SessionLog.Null;
    private BootContext? _boot;

    protected override void OnStartup(StartupEventArgs e)
    {
        base.OnStartup(e);

        AppDomain.CurrentDomain.UnhandledException += OnAppDomainUnhandledException;
        DispatcherUnhandledException += OnDispatcherUnhandledException;
        TaskScheduler.UnobservedTaskException += OnUnobservedTaskException;

        try
        {
            _boot = BootContext.Start();
            _log = _boot.Log;
            _log.Info("boot", "boot completed placement=" + _boot.Layout.Placement.ToString().ToLowerInvariant());
        }
        catch (Exception ex)
        {
            // 引导失败也要给出错误码，绝不向用户弹裸堆栈。
            ReportAndShutdown("boot", ex);
        }
    }

    protected override void OnExit(ExitEventArgs e)
    {
        _boot?.Dispose();
        _boot = null;
        base.OnExit(e);
    }

    private void OnDispatcherUnhandledException(object sender, DispatcherUnhandledExceptionEventArgs e)
    {
        // 未知异常之后继续跑，UI 状态是未定义的；如实报告并退出，比装作没事更安全。
        e.Handled = true;
        ReportAndShutdown("ui", e.Exception);
    }

    private void OnAppDomainUnhandledException(object sender, UnhandledExceptionEventArgs e)
    {
        _log.Failure("process", ErrorCode.None, e.ExceptionObject as Exception);
    }

    private void OnUnobservedTaskException(object? sender, UnobservedTaskExceptionEventArgs e)
    {
        _log.Failure("process", ErrorCode.None, e.Exception);
        e.SetObserved();
    }

    private void ReportAndShutdown(string phase, Exception? exception)
    {
        ErrorCode code = exception is LauncherException known ? known.Code : ErrorCode.None;

        _log.Failure(phase, code, exception);
        _log.Dispose();

        MessageBox.Show(
            ErrorCodes.Hint(code) + Environment.NewLine + Environment.NewLine + "错误码：" + ErrorCodes.Id(code),
            "Qinmo Ultimate Launcher",
            MessageBoxButton.OK,
            MessageBoxImage.Error);

        Shutdown(1);
    }
}
