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

        // 第一个参数是命令行动词就进命令行模式：不显示窗口，跑完带退出码结束。
        // 这既是架构要求（UI 挂了不影响命令行启动），也是真实验收的驱动方式。
        // 不是动词则当作界面参数——例如从快捷方式直接启动某个版本。
        string? initialVersion = null;

        if (e.Args != null && e.Args.Length > 0)
        {
            if (IsCliVerb(e.Args[0]))
            {
                int exitCode = CliRunner.Run(_boot, e.Args);

                // 命令行跑完且没有失败，同样算这次更新活下来了。
                if (exitCode == 0)
                {
                    _boot.ConfirmUpdateHealthy();
                }

                Shutdown(exitCode);
                return;
            }

            // **不认识的第一个参数必须报错退出，绝不能静默开界面。**
            //
            // 实测踩到过：脚本里把动词写错（`--version-check`），程序把它当成
            // 界面参数、开出一个窗口并一直挂着——对脚本化使用来说，
            // **卡死比报错难查得多**（要等到超时才发现，而且看不出原因）。
            //
            // 双击启动时没有参数，走不到这里；`--version <id>` 是唯一认识的界面参数。
            if (!IsGuiOption(e.Args[0]))
            {
                Console.Error.WriteLine("未知参数：" + e.Args[0]);
                Console.Error.WriteLine(
                    "用法：QinmoUltimateLauncher.exe [plan|install|launch|preflight|update|account] "
                    + "或 --version <版本号>");

                Shutdown(2);
                return;
            }

            initialVersion = OptionValue(e.Args, "--version");
        }

        // 渲染层级决定了界面是否走硬件加速。Tier 0 = 软件渲染，
        // 内存与流畅度都会明显变差——用户报"界面卡"时这是第一个要看的值。
        _log.Info("ui", string.Format(
            System.Globalization.CultureInfo.InvariantCulture,
            "render tier={0} animations={1} dpi={2}x{3}",
            System.Windows.Media.RenderCapability.Tier >> 16,
            System.Windows.SystemParameters.ClientAreaAnimation,
            System.Windows.SystemParameters.PrimaryScreenWidth,
            System.Windows.SystemParameters.PrimaryScreenHeight));

        // 深色优先：默认就是深色，用户可切浅色或跟随系统。
        ThemeManager.Initialize(ThemeMode.Dark);

        ShellWindow window = new ShellWindow
        {
            DataContext = new MainViewModel(_boot, initialVersion),
        };

        window.Closed += OnMainWindowClosed;

        // 窗口真正显示出来，才算这次更新活下来了。
        window.Loaded += (_, __) => _boot?.ConfirmUpdateHealthy();

        window.Show();
    }

    /// <summary>界面模式认识的参数。目前只有 <c>--version &lt;id&gt;</c>。</summary>
    private static bool IsGuiOption(string arg)
    {
        return string.Equals(arg, "--version", StringComparison.OrdinalIgnoreCase);
    }

    private static bool IsCliVerb(string arg)
    {
        switch (arg.Trim().ToLowerInvariant())
        {
            case "plan":
            case "install":
            case "launch":
            case "preflight":
            case "update":
            case "account":
                return true;
            default:
                return false;
        }
    }

    private static string? OptionValue(string[] args, string name)
    {
        for (int i = 0; i < args.Length - 1; i++)
        {
            if (string.Equals(args[i], name, StringComparison.OrdinalIgnoreCase))
            {
                return args[i + 1];
            }
        }

        return null;
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
