using System;
using System.Globalization;
using System.Windows.Data;

namespace Qul.Presentation;

/// <summary>取反的布尔转换器。用于"忙碌时禁用控件"这类绑定。</summary>
public sealed class InverseBooleanConverter : IValueConverter
{
    public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
    {
        return !(value is bool flag && flag);
    }

    public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture)
    {
        return !(value is bool flag && flag);
    }
}
