using System;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Threading;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Boot;
using Qul.Infrastructure.Cli;
using Qul.Presentation;
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
            return;
        }

        // 带参数即进入命令行模式：不显示窗口，跑完直接带退出码结束。
        // 这既是架构要求（UI 挂了不影响命令行启动），也是真实验收的驱动方式。
        if (e.Args != null && e.Args.Length > 0)
        {
            Shutdown(CliRunner.Run(_boot, e.Args));
            return;
        }

        // 深色优先：默认就是深色，用户可切浅色或跟随系统。
        ThemeManager.Initialize(ThemeMode.Dark);

        ShellWindow window = new ShellWindow
        {
            DataContext = new MainViewModel(_boot),
        };

        window.Closed += OnMainWindowClosed;
        window.Show();
    }

    private void OnMainWindowClosed(object? sender, EventArgs e)
    {
        // 显式关闭模式：主窗口关掉就退出，不留一个没有界面的进程在后台。
        Shutdown();
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

        // 异常的 message 往往就是唯一能定位问题的线索——
        // 例如 XamlParseException 会直接指出缺哪个资源、在哪一行。
        // 只记 hresult 等于把最有用的信息丢掉。
        for (Exception? current = exception; current != null; current = current.InnerException)
        {
            string message = (current.Message ?? string.Empty).Replace("\r", " ").Replace("\n", " ");
            _log.Info(phase, "exception detail: " + current.GetType().Name + ": " + message);
        }

        // 完整堆栈：XamlParseException 的堆栈里带着出问题的 XAML 元素与行号。
        _log.Info(phase, "exception stack: " + (exception?.ToString() ?? "(none)")
            .Replace("\r", " ").Replace("\n", " | ").Replace("\"", "'"));

        _log.Dispose();

        MessageBox.Show(
            ErrorCodes.Hint(code) + Environment.NewLine + Environment.NewLine + "错误码：" + ErrorCodes.Id(code),
            "Qinmo Ultimate Launcher",
            MessageBoxButton.OK,
            MessageBoxImage.Error);

        Shutdown(1);
    }
}
