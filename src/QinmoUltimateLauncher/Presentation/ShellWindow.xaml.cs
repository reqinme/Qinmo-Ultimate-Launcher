using System.Collections.Specialized;
using System.Windows;
using System.Windows.Controls;

namespace Qul.Presentation;

public partial class ShellWindow : Window
{
    private bool _logPinned = true;

    public ShellWindow()
    {
        InitializeComponent();
        Loaded += OnLoaded;
    }

    private void OnLoaded(object sender, RoutedEventArgs e)
    {
        if (DataContext is MainViewModel viewModel)
        {
            viewModel.RefreshCommand.Execute(null);
        }
    }

    /// <summary>
    /// 日志吸附：只有本来就贴着底部时才自动跟随。
    /// 用户往上翻看历史时不该被新行拽回去。
    /// </summary>
    private void OnLogScrollChanged(object sender, ScrollChangedEventArgs e)
    {
        if (e.ExtentHeightChange == 0 && e.VerticalChange != 0)
        {
            // 用户自己滚了：到底部才重新吸附。
            _logPinned = e.VerticalOffset >= e.ExtentHeight - e.ViewportHeight - 1;
            return;
        }

        if (_logPinned && e.ExtentHeightChange > 0 && LogList.Items.Count > 0)
        {
            LogList.ScrollIntoView(LogList.Items[LogList.Items.Count - 1]);
        }
    }
}
