using System;
using System.Collections.Generic;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using Qul.Domain.Diagnostics;

namespace Qul.Domain.Launch;

/// <summary>
/// 启动计划里出现的占位符名字。
///
/// 这份清单不是拍脑袋来的：它是四个真实版本（1.7.10 / 1.12.2 / 1.16.5 / 26.3）
/// 元数据里全部 <c>${...}</c> 的并集。26.x 相较早期多出了 auth_xuid、clientid
/// 与四个 quickPlay* —— 漏掉任何一个，参数组装都会以未替换占位符告终。
/// </summary>
public static class LaunchPlanPlaceholders
{
    public const string AuthPlayerName = "auth_player_name";
    public const string AuthUuid = "auth_uuid";
    public const string AuthAccessToken = "auth_access_token";
    public const string AuthSession = "auth_session";
    public const string AuthXuid = "auth_xuid";
    public const string ClientId = "clientid";
    public const string UserProperties = "user_properties";
    public const string UserType = "user_type";
    public const string VersionName = "version_name";
    public const string VersionType = "version_type";
    public const string GameDirectory = "game_directory";
    public const string AssetsRoot = "assets_root";
    public const string AssetsIndexName = "assets_index_name";
    public const string NativesDirectory = "natives_directory";
    public const string ClassPath = "classpath";
    public const string ClassPathSeparator = "classpath_separator";
    public const string LibraryDirectory = "library_directory";
    public const string LauncherName = "launcher_name";
    public const string LauncherVersion = "launcher_version";
    public const string LoggingPath = "path";
    public const string ResolutionWidth = "resolution_width";
    public const string ResolutionHeight = "resolution_height";
    public const string QuickPlayPath = "quickPlayPath";
    public const string QuickPlaySingleplayer = "quickPlaySingleplayer";
    public const string QuickPlayMultiplayer = "quickPlayMultiplayer";
    public const string QuickPlayRealms = "quickPlayRealms";

    public const string Marker = "${";

    /// <summary>已知的全部占位符。模板里出现清单之外的名字即视为未替换。</summary>
    public static readonly IReadOnlyList<string> All = new[]
    {
        AuthPlayerName, AuthUuid, AuthAccessToken, AuthSession, AuthXuid, ClientId,
        UserProperties, UserType, VersionName, VersionType,
        GameDirectory, AssetsRoot, AssetsIndexName, NativesDirectory,
        ClassPath, ClassPathSeparator, LibraryDirectory,
        LauncherName, LauncherVersion, LoggingPath,
        ResolutionWidth, ResolutionHeight,
        QuickPlayPath, QuickPlaySingleplayer, QuickPlayMultiplayer, QuickPlayRealms,
    };

    /// <summary>
    /// 可以明文出现在导出里的占位符。**默认拒绝**：不在这个白名单里的一律按敏感处理。
    /// 白名单里只有与具体人、具体机器、具体路径无关的值。
    /// </summary>
    private static readonly HashSet<string> SafeToShow = new HashSet<string>(StringComparer.Ordinal)
    {
        ClientId,
        UserType,
        VersionName,
        VersionType,
        AssetsIndexName,
        ClassPathSeparator,
        LauncherName,
        LauncherVersion,
        ResolutionWidth,
        ResolutionHeight,
    };

    private static readonly HashSet<string> Known = new HashSet<string>(All, StringComparer.Ordinal);

    public static bool IsKnown(string name)
    {
        return Known.Contains(name);
    }

    public static bool IsSensitive(string name)
    {
        return !SafeToShow.Contains(name);
    }
}

/// <summary>
/// 平面 A：**可复现骨架**。
///
/// 只包含稳定的、非秘密的字段。同一份输入两次构建必须逐字节一致，
/// 因此这里没有时间戳、没有绝对路径、没有运行期取值。
/// </summary>
public sealed class LaunchPlanSkeleton
{
    /// <summary>骨架格式版本。字段增删时递增，让旧记录一眼可辨。</summary>
    public const int FormatVersion = 1;

    public string VersionId { get; set; } = string.Empty;

    public string MainClass { get; set; } = string.Empty;

    public int JavaMajorRequirement { get; set; }

    /// <summary>32 / 64，即 ${arch} 的取值来源。</summary>
    public string JavaArchToken { get; set; } = string.Empty;

    /// <summary>x86_64 / x86 / arm64。</summary>
    public string JavaOsArch { get; set; } = string.Empty;

    /// <summary>身份来源种类（offline / microsoft / thirdparty），不是身份本身。</summary>
    public string IdentitySource { get; set; } = string.Empty;

    /// <summary>工作目录的**逻辑标识**。绝对路径在平面 B。</summary>
    public string WorkingDirectoryId { get; set; } = string.Empty;

    /// <summary>natives 目录的**逻辑标识**。</summary>
    public string NativesDirectoryId { get; set; } = string.Empty;

    /// <summary>相对缓存根的类路径条目。顺序即类路径顺序。</summary>
    public IReadOnlyList<string> ClassPathEntries { get; set; } = Array.Empty<string>();

    /// <summary>JVM 参数模板（扁平 token 流，含 ${...} 占位符，不含任何取值）。</summary>
    public IReadOnlyList<string> JvmArgumentTemplate { get; set; } = Array.Empty<string>();

    /// <summary>游戏参数模板（扁平 token 流）。</summary>
    public IReadOnlyList<string> GameArgumentTemplate { get; set; } = Array.Empty<string>();

    /// <summary>环境变量**键名**，不含取值。</summary>
    public IReadOnlyList<string> EnvironmentKeys { get; set; } = Array.Empty<string>();

    /// <summary>身份注入项的键名，不含取值。</summary>
    public IReadOnlyList<string> InjectionKeys { get; set; } = Array.Empty<string>();

    /// <summary>
    /// 规范化文本。逐字节比对与哈希都以它为准。
    /// 换行固定用 \n、数字用不变文化——否则跨机器、跨区域设置就比不起来了。
    /// </summary>
    public string ToCanonicalText()
    {
        StringBuilder builder = new StringBuilder(2048);

        Append(builder, "planFormat", FormatVersion.ToString(CultureInfo.InvariantCulture));
        Append(builder, "version", VersionId);
        Append(builder, "mainClass", MainClass);
        Append(builder, "javaMajor", JavaMajorRequirement.ToString(CultureInfo.InvariantCulture));
        Append(builder, "javaArchToken", JavaArchToken);
        Append(builder, "javaOsArch", JavaOsArch);
        Append(builder, "identitySource", IdentitySource);
        Append(builder, "workingDirectoryId", WorkingDirectoryId);
        Append(builder, "nativesDirectoryId", NativesDirectoryId);

        AppendIndexed(builder, "classpath", ClassPathEntries);
        AppendIndexed(builder, "jvm", JvmArgumentTemplate);
        AppendIndexed(builder, "game", GameArgumentTemplate);
        AppendIndexed(builder, "env", EnvironmentKeys);
        AppendIndexed(builder, "inject", InjectionKeys);

        return builder.ToString();
    }

    /// <summary>骨架指纹。日志里只记这一串，避免把整份计划灌进日志。</summary>
    public string ComputeHash()
    {
        byte[] bytes = Encoding.UTF8.GetBytes(ToCanonicalText());

        using (SHA1 sha1 = SHA1.Create())
        {
            byte[] hash = sha1.ComputeHash(bytes);
            StringBuilder builder = new StringBuilder(hash.Length * 2);
            for (int i = 0; i < hash.Length; i++)
            {
                builder.Append(hash[i].ToString("x2", CultureInfo.InvariantCulture));
            }

            return builder.ToString();
        }
    }

    private static void AppendIndexed(StringBuilder builder, string prefix, IReadOnlyList<string> values)
    {
        for (int i = 0; i < values.Count; i++)
        {
            Append(builder, prefix + "[" + i.ToString(CultureInfo.InvariantCulture) + "]", values[i]);
        }
    }

    private static void Append(StringBuilder builder, string key, string value)
    {
        builder.Append(key).Append('=').Append(value ?? string.Empty).Append('\n');
    }
}

/// <summary>
/// 平面 B：**运行时秘密引用**。
///
/// 这些值只在启动瞬间使用，**绝不参与骨架比对，绝不出现在任何导出里**。
/// </summary>
public sealed class LaunchPlanSecrets
{
    public IReadOnlyDictionary<string, string> Values { get; set; } =
        new Dictionary<string, string>(0, StringComparer.Ordinal);

    public string WorkingDirectory { get; set; } = string.Empty;

    public string NativesDirectory { get; set; } = string.Empty;

    public string JavaExecutablePath { get; set; } = string.Empty;

    public DateTimeOffset BuiltAt { get; set; }

    public string? Get(string name)
    {
        return Values.TryGetValue(name, out string? value) ? value : null;
    }
}

/// <summary>
/// 一份完整的启动计划：可复现的骨架 + 只在启动瞬间解析的秘密。
/// </summary>
public sealed class LaunchPlan
{
    public LaunchPlan(LaunchPlanSkeleton skeleton, LaunchPlanSecrets secrets)
    {
        Skeleton = skeleton ?? throw new ArgumentNullException(nameof(skeleton));
        Secrets = secrets ?? throw new ArgumentNullException(nameof(secrets));
    }

    public LaunchPlanSkeleton Skeleton { get; }

    public LaunchPlanSecrets Secrets { get; }

    /// <summary>解析出真正的 JVM 参数。未替换的占位符会抛 QUL-PLAN-0001，而不是把 ${...} 喂给 JVM。</summary>
    public IReadOnlyList<string> ResolveJvmArguments()
    {
        List<string> unresolved = new List<string>();
        List<string> arguments = ResolveStream(Skeleton.JvmArgumentTemplate, masked: false, unresolved);

        ThrowIfUnresolved(unresolved);
        return arguments;
    }

    public IReadOnlyList<string> ResolveGameArguments()
    {
        List<string> unresolved = new List<string>();
        List<string> arguments = ResolveStream(Skeleton.GameArgumentTemplate, masked: false, unresolved);

        ThrowIfUnresolved(unresolved);
        return arguments;
    }

    /// <summary>
    /// 人类可读、**已脱敏**的导出。
    /// 骨架部分原样（它本来就不含秘密）；解析后的命令行里敏感值以 &lt;名字&gt; 呈现。
    /// </summary>
    public string ToRedactedText()
    {
        StringBuilder builder = new StringBuilder(4096);
        builder.Append("# Qinmo Ultimate Launcher 启动计划（已脱敏）\n");
        builder.Append(Skeleton.ToCanonicalText());
        builder.Append("# --- 解析后的命令行；敏感值以占位符呈现 ---\n");

        AppendResolved(builder, "jvm-resolved", ResolveStream(Skeleton.JvmArgumentTemplate, masked: true, null));
        AppendResolved(builder, "game-resolved", ResolveStream(Skeleton.GameArgumentTemplate, masked: true, null));

        return builder.ToString();
    }

    /// <summary>
    /// 扁平 token 流上的替换与"孤儿开关"修复。
    ///
    /// 为什么必须修复孤儿开关：真实元数据里 <c>--xuid</c> 与 <c>${auth_xuid}</c> 是**两个独立条目**
    /// （不是"一个参数两个 token"）。值解析为空时若只丢值，开关会孤零零留下，
    /// 游戏就会把下一个参数当成它的值——参数会整体错位。
    ///
    /// 因此：**值为空的 token 连同紧邻的前一个以 <c>-</c> 开头的 token 一起丢弃。**
    /// 这对"独立条目"和"数组 value 的复合条目"两种形态都成立。
    /// </summary>
    private List<string> ResolveStream(IReadOnlyList<string> template, bool masked, List<string>? unresolved)
    {
        List<string> substituted = new List<string>(template.Count);

        for (int i = 0; i < template.Count; i++)
        {
            substituted.Add(Substitute(template[i], masked, unresolved));
        }

        List<string> result = new List<string>(substituted.Count);

        for (int i = 0; i < substituted.Count; i++)
        {
            if (substituted[i].Length == 0)
            {
                if (result.Count > 0 && result[result.Count - 1].StartsWith("-", StringComparison.Ordinal))
                {
                    result.RemoveAt(result.Count - 1);
                }

                continue;
            }

            result.Add(substituted[i]);
        }

        return result;
    }

    private static void ThrowIfUnresolved(List<string> unresolved)
    {
        if (unresolved.Count == 0)
        {
            return;
        }

        throw new LauncherException(
            ErrorCode.PlanUnresolvedPlaceholder,
            "unresolved placeholders: " + string.Join(", ", Distinct(unresolved)));
    }

    private static void AppendResolved(StringBuilder builder, string prefix, List<string> arguments)
    {
        for (int i = 0; i < arguments.Count; i++)
        {
            builder.Append(prefix).Append('=').Append(arguments[i]).Append('\n');
        }
    }

    /// <summary>
    /// 替换一个 token 里的全部占位符。
    /// 取值为空串时返回空串——调用方据此执行孤儿开关修复。
    /// </summary>
    private string Substitute(string token, bool masked, List<string>? unresolved)
    {
        if (token == null)
        {
            return string.Empty;
        }

        if (token.IndexOf(LaunchPlanPlaceholders.Marker, StringComparison.Ordinal) < 0)
        {
            return token;
        }

        StringBuilder builder = new StringBuilder(token.Length + 16);
        int cursor = 0;

        while (cursor < token.Length)
        {
            int start = token.IndexOf(LaunchPlanPlaceholders.Marker, cursor, StringComparison.Ordinal);
            if (start < 0)
            {
                builder.Append(token, cursor, token.Length - cursor);
                break;
            }

            builder.Append(token, cursor, start - cursor);

            int nameStart = start + LaunchPlanPlaceholders.Marker.Length;
            int end = token.IndexOf('}', nameStart);
            if (end < 0)
            {
                builder.Append(token, start, token.Length - start);
                unresolved?.Add(token.Substring(start));
                break;
            }

            string name = token.Substring(nameStart, end - nameStart);

            if (!LaunchPlanPlaceholders.IsKnown(name))
            {
                unresolved?.Add(name);
                builder.Append(masked ? "<unknown:" + name + ">" : token.Substring(start, end - start + 1));
                cursor = end + 1;
                continue;
            }

            string? value = Secrets.Get(name);
            if (value == null)
            {
                unresolved?.Add(name);
                builder.Append(masked ? "<missing:" + name + ">" : token.Substring(start, end - start + 1));
                cursor = end + 1;
                continue;
            }

            if (masked && LaunchPlanPlaceholders.IsSensitive(name))
            {
                builder.Append('<').Append(name).Append('>');
            }
            else
            {
                builder.Append(value);
            }

            cursor = end + 1;
        }

        return builder.ToString();
    }

    private static List<string> Distinct(List<string> values)
    {
        List<string> result = new List<string>();
        HashSet<string> seen = new HashSet<string>(StringComparer.Ordinal);

        for (int i = 0; i < values.Count; i++)
        {
            if (seen.Add(values[i]))
            {
                result.Add(values[i]);
            }
        }

        return result;
    }
}
