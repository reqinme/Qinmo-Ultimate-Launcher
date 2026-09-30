using System;
using System.Collections.Generic;
using System.Text.RegularExpressions;

namespace Qul.Domain.Metadata;

/// <summary>
/// 规则求值所依赖的运行环境画像。
/// 刻意做成纯数据：探测真实环境属于基础设施职责（Domain 层被禁止引用进程与系统 API），
/// 这样规则求值本身可以在任意平台上被完整单测。
/// </summary>
public sealed class EnvironmentProfile
{
    public const string OsWindows = "windows";
    public const string OsLinux = "linux";
    public const string OsOsx = "osx";

    public const string ArchX86 = "x86";
    public const string ArchX86_64 = "x86_64";
    public const string ArchArm64 = "arm64";

    private static readonly IReadOnlyDictionary<string, bool> NoFeatures =
        new Dictionary<string, bool>(0, StringComparer.Ordinal);

    public EnvironmentProfile(
        string osName,
        string osArch,
        string? osVersion = null,
        IReadOnlyDictionary<string, bool>? features = null)
    {
        OsName = osName ?? throw new ArgumentNullException(nameof(osName));
        OsArch = osArch ?? throw new ArgumentNullException(nameof(osArch));
        OsVersion = osVersion;
        Features = features ?? NoFeatures;
    }

    public string OsName { get; }

    public string OsArch { get; }

    public string? OsVersion { get; }

    public IReadOnlyDictionary<string, bool> Features { get; }
}

/// <summary>
/// 元数据规则求值。规则书见 docs/P1-版本元数据规范.md。
/// </summary>
public static class RuleEvaluator
{
    /// <summary>
    /// 规则列表求值。语义与官方启动器一致：
    /// 无规则 → 允许；有规则 → 初始为拒绝，按顺序求值，命中的规则覆盖结论，最终以最后一条命中的规则为准。
    /// 也就是说"有规则但一条都没命中"的结果是拒绝。
    /// </summary>
    public static bool IsAllowed(IReadOnlyList<Rule>? rules, EnvironmentProfile environment)
    {
        if (environment == null)
        {
            throw new ArgumentNullException(nameof(environment));
        }

        if (rules == null || rules.Count == 0)
        {
            return true;
        }

        bool allowed = false;

        for (int i = 0; i < rules.Count; i++)
        {
            Rule rule = rules[i];
            if (Matches(rule, environment))
            {
                allowed = rule.Action == RuleAction.Allow;
            }
        }

        return allowed;
    }

    public static bool Matches(Rule rule, EnvironmentProfile environment)
    {
        if (rule == null)
        {
            throw new ArgumentNullException(nameof(rule));
        }

        if (environment == null)
        {
            throw new ArgumentNullException(nameof(environment));
        }

        if (!string.IsNullOrEmpty(rule.OsName) && !Equals2(rule.OsName, environment.OsName))
        {
            return false;
        }

        if (!string.IsNullOrEmpty(rule.OsArch) && !ArchMatches(rule.OsArch!, environment.OsArch))
        {
            return false;
        }

        if (!string.IsNullOrEmpty(rule.OsVersion) && !VersionMatches(rule.OsVersion!, environment.OsVersion))
        {
            return false;
        }

        foreach (KeyValuePair<string, bool> feature in rule.Features)
        {
            bool actual = environment.Features.TryGetValue(feature.Key, out bool value) && value;
            if (actual != feature.Value)
            {
                return false;
            }
        }

        return true;
    }

    /// <summary>
    /// 架构匹配。
    /// 除严格相等外，额外接受一条兼容规则：元数据要求 x86 时，x86_64 环境也算命中。
    /// 这是官方启动器长期存在的行为（32 位规则在 64 位 JVM 上仍应生效），
    /// 保留它是为了不与既有元数据为敌；四个黄金样本均未使用 os.arch，因此该分支由单测单独锁定。
    /// </summary>
    public static bool ArchMatches(string required, string actual)
    {
        if (Equals2(required, actual))
        {
            return true;
        }

        return Equals2(required, EnvironmentProfile.ArchX86)
               && Equals2(actual, EnvironmentProfile.ArchX86_64);
    }

    /// <summary>os.version 是正则匹配。模式非法或环境版本未知时按不命中处理，绝不让畸形元数据抛穿。</summary>
    public static bool VersionMatches(string pattern, string? environmentVersion)
    {
        if (string.IsNullOrEmpty(environmentVersion))
        {
            return false;
        }

        try
        {
            return Regex.IsMatch(environmentVersion!, pattern, RegexOptions.CultureInvariant);
        }
        catch (ArgumentException)
        {
            return false;
        }
    }

    private static bool Equals2(string? left, string? right)
    {
        return string.Equals(left, right, StringComparison.OrdinalIgnoreCase);
    }
}
