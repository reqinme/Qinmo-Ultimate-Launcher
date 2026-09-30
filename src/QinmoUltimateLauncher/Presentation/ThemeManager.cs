using System;
using System.Windows;
using Microsoft.Win32;

namespace Qul.Presentation;

public enum ThemeMode
{
    System = 0,
    Dark = 1,
    Light = 2,
}

/// <summary>
/// 主题切换：深色 / 浅色 / 跟随系统。
///
/// 做法是替换资源字典里的**那一个调色板实例**。这要求全局只有一个调色板字典——
/// 所以任何控件字典都不许自己合并调色板，否则会出现多个实例，
/// 表现就是"一半变了、一半没变"，这是主题切换最典型的 bug。
///
/// 画刷必须用 DynamicResource 才会跟着换；StaticResource 会卡在旧颜色上。
/// </summary>
public static class ThemeManager
{
    private const string PalettePrefix = "Palette.";

    public static ThemeMode Mode { get; private set; } = ThemeMode.System;

    /// <summary>当前实际生效的是不是深色。</summary>
    public static bool IsDarkEffective { get; private set; } = true;

    public static event EventHandler? Changed;

    public static void Initialize(ThemeMode mode)
    {
        Mode = mode;
        Apply();
    }

    public static void Set(ThemeMode mode)
    {
        Mode = mode;
        Apply();
    }

    /// <summary>按 跟随系统 → 深色 → 浅色 → 跟随系统 循环。</summary>
    public static ThemeMode Cycle()
    {
        Set(Next(Mode));
        return Mode;
    }

    public static ThemeMode Next(ThemeMode mode)
    {
        switch (mode)
        {
            case ThemeMode.System:
                return ThemeMode.Dark;
            case ThemeMode.Dark:
                return ThemeMode.Light;
            default:
                return ThemeMode.System;
        }
    }

    public static string Describe(ThemeMode mode)
    {
        switch (mode)
        {
            case ThemeMode.Dark:
                return "深色";
            case ThemeMode.Light:
                return "浅色";
            default:
                return "跟随系统";
        }
    }

    public static string IconKey(ThemeMode mode)
    {
        switch (mode)
        {
            case ThemeMode.Dark:
                return "Qul.Icon.ThemeDark";
            case ThemeMode.Light:
                return "Qul.Icon.ThemeLight";
            default:
                return "Qul.Icon.ThemeSystem";
        }
    }

    private static void Apply()
    {
        // Qul.Application 这个命名空间会遮蔽 System.Windows.Application，必须写全。
        System.Windows.Application? application = System.Windows.Application.Current;
        if (application == null)
        {
            return;
        }

        bool dark = Mode == ThemeMode.Dark || (Mode == ThemeMode.System && IsSystemDark());
        IsDarkEffective = dark;

        ResourceDictionary replacement = new ResourceDictionary
        {
            Source = PaletteUri(dark),
        };

        if (ReplacePalette(application.Resources, replacement))
        {
            Changed?.Invoke(null, EventArgs.Empty);
        }
    }

    private static Uri PaletteUri(bool dark)
    {
        return new Uri(
            "pack://application:,,,/QinmoUltimateLauncher;component/Presentation/Theme/Palette."
            + (dark ? "Dark" : "Light") + ".xaml",
            UriKind.Absolute);
    }

    /// <summary>在合并树里找到调色板槽位并整体替换掉它。</summary>
    private static bool ReplacePalette(ResourceDictionary root, ResourceDictionary replacement)
    {
        for (int i = 0; i < root.MergedDictionaries.Count; i++)
        {
            ResourceDictionary child = root.MergedDictionaries[i];
            string? source = child.Source?.OriginalString;

            if (source != null && source.IndexOf(PalettePrefix, StringComparison.OrdinalIgnoreCase) >= 0)
            {
                root.MergedDictionaries[i] = replacement;
                return true;
            }

            if (ReplacePalette(child, replacement))
            {
                return true;
            }
        }

        return false;
    }

    /// <summary>
    /// 读系统的"应用模式"。读不到就按深色 —— 深色优先是本项目的既定取向。
    /// 不引入任何第三方库，只读一个注册表值。
    /// </summary>
    private static bool IsSystemDark()
    {
        try
        {
            using (RegistryKey? key = Registry.CurrentUser.OpenSubKey(
                @"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"))
            {
                object? value = key?.GetValue("AppsUseLightTheme");

                if (value is int light)
                {
                    return light == 0;
                }
            }
        }
        catch (Exception ex) when (ex is System.Security.SecurityException || ex is UnauthorizedAccessException)
        {
            // 读不到不是错误，按深色处理。
        }

        return true;
    }
}
