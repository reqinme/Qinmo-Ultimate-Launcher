using System.Windows;
using System.Windows.Controls;

namespace Qul.Presentation;

public partial class ShellWindow : Window
{
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

    private void OnLogTextChanged(object sender, TextChangedEventArgs e)
    {
        // 日志总是滚到最新一行——排查时最关心的就是最后那几行。
        ((TextBox)sender).ScrollToEnd();
    }
}
