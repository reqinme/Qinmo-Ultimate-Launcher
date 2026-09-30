using System;
using System.Collections.Generic;
using System.Globalization;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Domain.Identity;
using Qul.Domain.Launch;
using Qul.Domain.Metadata;
using Qul.Domain.Runtime;

namespace Qul.Application.Launch;

public sealed class LaunchPlanRequest
{
    public VersionDetail Version { get; set; } = new VersionDetail();

    /// <summary>已按**选定 JVM** 构造的环境画像（natives 与 os.arch 规则要以它为准）。</summary>
    public EnvironmentProfile Environment { get; set; } = new EnvironmentProfile(
        EnvironmentProfile.OsWindows, EnvironmentProfile.ArchX86_64);

    public JavaRuntimeCandidate Java { get; set; } = new JavaRuntimeCandidate();

    public PlayerIdentity Identity { get; set; } = OfflineIdentityFactory.Create("Player");

    /// <summary>缓存根绝对路径。</summary>
    public string CacheRoot { get; set; } = string.Empty;

    public string GameDirectory { get; set; } = string.Empty;

    public string NativesDirectory { get; set; } = string.Empty;

    /// <summary>资源根绝对路径（游戏把它当 --assetsDir）。为空时按缓存根下的 assets 推导。</summary>
    public string? AssetsDirectory { get; set; }

    /// <summary>库目录绝对路径。为空时按缓存根下的 libraries 推导。</summary>
    public string? LibraryDirectory { get; set; }

    /// <summary>日志配置文件的绝对路径。没有就不加那条 JVM 参数——宁可没有，也不要一个指向空路径的属性。</summary>
    public string? LoggingConfigPath { get; set; }

    public int? MaxMemoryMb { get; set; }

    public IReadOnlyList<string> ExtraJvmArgs { get; set; } = Array.Empty<string>();

    public IReadOnlyDictionary<string, string> EnvironmentVariables { get; set; } =
        new Dictionary<string, string>(0, StringComparer.Ordinal);

    public int? ResolutionWidth { get; set; }

    public int? ResolutionHeight { get; set; }

    public string LauncherName { get; set; } = "QinmoUltimateLauncher";

    public string LauncherVersion { get; set; } = "0.1.0";

    public string? ClientId { get; set; }
}

/// <summary>
/// 把"已合并的版本 + 选定的 Java + 身份 + 配置"组装成一份启动计划。
///
/// 关键分工：**模板进骨架，取值进秘密**。
/// 骨架里只有 ${...} 占位符，没有任何令牌、UUID、用户名或绝对路径——
/// 因此换一个账户、换一台机器，骨架依然逐字节一致。
/// </summary>
public static class LaunchPlanBuilder
{
    private const string DefaultClientId = "qinmo-ultimate-launcher";

    private static readonly CachePathConventions Paths = new CachePathConventions();

    public static LaunchPlan Build(LaunchPlanRequest request)
    {
        if (request == null)
        {
            throw new ArgumentNullException(nameof(request));
        }

        LauncherException? invalid = VersionResolver.Validate(request.Version);
        if (invalid != null)
        {
            throw invalid;
        }

        if (string.IsNullOrWhiteSpace(request.CacheRoot))
        {
            throw new LauncherException(ErrorCode.PlanUnresolvedPlaceholder, "cache root is required");
        }

        if (string.IsNullOrWhiteSpace(request.GameDirectory))
        {
            throw new LauncherException(ErrorCode.PlanUnresolvedPlaceholder, "game directory is required");
        }

        string versionId = request.Version.Id;
        string assetsDirectory = request.AssetsDirectory ?? Combine(request.CacheRoot, Paths.AssetsRoot);
        string libraryDirectory = request.LibraryDirectory ?? Combine(request.CacheRoot, Paths.LibrariesPrefix);
        const string ClassPathSeparator = ";";

        // 一部分参数条目由 feature 门控而不是 os 门控（实测 --width/--height 要求
        // has_custom_resolution=true，快速进入四项各要求自己的 feature）。
        // 不声明 feature，这些参数根本进不了计划——把请求里的设置翻译成 feature 是组装器的职责。
        EnvironmentProfile environment = WithLaunchFeatures(request);

        // ---------- 类路径：库在前，客户端 jar 在最后 ----------

        List<string> relativeClassPath = new List<string>();
        List<string> absoluteClassPath = new List<string>();

        for (int i = 0; i < request.Version.Libraries.Count; i++)
        {
            LibraryRef library = request.Version.Libraries[i];

            if (!RuleEvaluator.IsAllowed(library.Rules, environment))
            {
                continue;
            }

            // 旧式 natives 容器没有主 artifact：它只有 classifier 内容，会被解压到 natives 目录，
            // 不参与类路径。26.x 的 natives 是坐标自带 classifier 的独立条目，有 artifact，会正常进来。
            if (library.Artifact == null)
            {
                continue;
            }

            string relative = Paths.LibraryFile(library.Artifact.Path, library.Name, library.RawName);
            relativeClassPath.Add(relative);
            absoluteClassPath.Add(Combine(request.CacheRoot, relative));
        }

        string clientRelative = Paths.ClientJarFile(versionId);
        relativeClassPath.Add(clientRelative);
        absoluteClassPath.Add(Combine(request.CacheRoot, clientRelative));

        // ---------- JVM 参数（扁平 token 流） ----------

        List<string> jvm = new List<string>();

        if (request.MaxMemoryMb.HasValue && request.MaxMemoryMb.Value > 0)
        {
            jvm.Add("-Xmx" + request.MaxMemoryMb.Value.ToString(CultureInfo.InvariantCulture) + "M");
        }

        int jvmFromMetadata = AppendFiltered(jvm, request.Version.JvmArguments, environment);

        if (jvmFromMetadata == 0)
        {
            // 1.7 / 1.12 时代的元数据没有 arguments.jvm，这几条得自己补。
            jvm.Add("-Djava.library.path=${" + LaunchPlanPlaceholders.NativesDirectory + "}");
            jvm.Add("-cp");
            jvm.Add("${" + LaunchPlanPlaceholders.ClassPath + "}");
        }

        for (int i = 0; i < request.ExtraJvmArgs.Count; i++)
        {
            if (!string.IsNullOrWhiteSpace(request.ExtraJvmArgs[i]))
            {
                jvm.Add(request.ExtraJvmArgs[i]);
            }
        }

        // 只在真的拿到日志配置文件时才加这条；否则会产生一个指向空路径的属性。
        string loggingArgument = request.Version.Logging?.Argument ?? string.Empty;
        if (!string.IsNullOrEmpty(request.LoggingConfigPath) && loggingArgument.Length > 0)
        {
            jvm.Add(loggingArgument);
        }

        // ---------- 游戏参数（扁平 token 流） ----------

        List<string> game = new List<string>();
        int gameFromMetadata = AppendFiltered(game, request.Version.GameArguments, environment);

        if (gameFromMetadata == 0 && !string.IsNullOrEmpty(request.Version.MinecraftArguments))
        {
            // 旧式：整串按空白切分。
            string[] tokens = request.Version.MinecraftArguments!.Split(new[] { ' ' }, StringSplitOptions.RemoveEmptyEntries);
            for (int i = 0; i < tokens.Length; i++)
            {
                game.Add(tokens[i]);
            }
        }

        // ---------- 秘密取值 ----------

        string accessToken = request.Identity.AccessToken ?? string.Empty;

        Dictionary<string, string> values = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            [LaunchPlanPlaceholders.AuthPlayerName] = request.Identity.UserName,
            [LaunchPlanPlaceholders.AuthUuid] = request.Identity.Uuid,
            [LaunchPlanPlaceholders.AuthAccessToken] = accessToken,
            [LaunchPlanPlaceholders.AuthSession] = accessToken,

            // 离线账户没有 Xbox 身份标识，如实给空串——该参数会连同它的开关一起省略，
            // 而不是编一个假的标识。
            [LaunchPlanPlaceholders.AuthXuid] = string.Empty,

            [LaunchPlanPlaceholders.ClientId] = string.IsNullOrEmpty(request.ClientId) ? DefaultClientId : request.ClientId!,
            [LaunchPlanPlaceholders.UserProperties] = "{}",
            [LaunchPlanPlaceholders.UserType] = request.Identity.UserType,
            [LaunchPlanPlaceholders.VersionName] = versionId,
            [LaunchPlanPlaceholders.VersionType] = VersionTypeText(request.Version.Type),
            [LaunchPlanPlaceholders.GameDirectory] = request.GameDirectory,
            [LaunchPlanPlaceholders.AssetsRoot] = assetsDirectory,
            [LaunchPlanPlaceholders.AssetsIndexName] = request.Version.Assets ?? request.Version.AssetIndex?.Id ?? string.Empty,
            [LaunchPlanPlaceholders.NativesDirectory] = request.NativesDirectory,
            [LaunchPlanPlaceholders.ClassPath] = string.Join(ClassPathSeparator, absoluteClassPath),
            [LaunchPlanPlaceholders.ClassPathSeparator] = ClassPathSeparator,
            [LaunchPlanPlaceholders.LibraryDirectory] = libraryDirectory,
            [LaunchPlanPlaceholders.LauncherName] = request.LauncherName,
            [LaunchPlanPlaceholders.LauncherVersion] = request.LauncherVersion,
            [LaunchPlanPlaceholders.LoggingPath] = request.LoggingConfigPath ?? string.Empty,
            [LaunchPlanPlaceholders.ResolutionWidth] = Text(request.ResolutionWidth),
            [LaunchPlanPlaceholders.ResolutionHeight] = Text(request.ResolutionHeight),

            // 快速进入（quick play）四项：MVP 不提供该功能，如实给空串让相关开关一并省略。
            [LaunchPlanPlaceholders.QuickPlayPath] = string.Empty,
            [LaunchPlanPlaceholders.QuickPlaySingleplayer] = string.Empty,
            [LaunchPlanPlaceholders.QuickPlayMultiplayer] = string.Empty,
            [LaunchPlanPlaceholders.QuickPlayRealms] = string.Empty,
        };

        List<string> environmentKeys = new List<string>();
        foreach (KeyValuePair<string, string> pair in request.EnvironmentVariables)
        {
            environmentKeys.Add(pair.Key);
            values[pair.Key] = pair.Value;
        }

        environmentKeys.Sort(StringComparer.Ordinal);

        LaunchPlanSkeleton skeleton = new LaunchPlanSkeleton
        {
            VersionId = versionId,
            MainClass = request.Version.MainClass ?? string.Empty,
            JavaMajorRequirement = request.Version.JavaVersion?.MajorVersion ?? 8,
            JavaArchToken = request.Java.ArchToken,
            JavaOsArch = request.Java.OsArch ?? string.Empty,
            IdentitySource = request.Identity.Source.ToString().ToLowerInvariant(),
            WorkingDirectoryId = "game",
            NativesDirectoryId = "natives-" + versionId,
            ClassPathEntries = relativeClassPath,
            JvmArgumentTemplate = jvm,
            GameArgumentTemplate = game,
            EnvironmentKeys = environmentKeys,
            InjectionKeys = CollectInjectionKeys(jvm, game),
        };

        LaunchPlanSecrets secrets = new LaunchPlanSecrets
        {
            Values = values,
            WorkingDirectory = request.GameDirectory,
            NativesDirectory = request.NativesDirectory,
            JavaExecutablePath = request.Java.ExecutablePath,
            BuiltAt = DateTimeOffset.Now,
        };

        return new LaunchPlan(skeleton, secrets);
    }

    /// <summary>
    /// 把请求里的设置翻译成元数据认识的特性开关。
    /// 只有 <c>has_custom_resolution</c> 会被置真；其余显式置假，让规则求值的输入是完整的而非缺省的。
    /// </summary>
    private static EnvironmentProfile WithLaunchFeatures(LaunchPlanRequest request)
    {
        Dictionary<string, bool> features = new Dictionary<string, bool>(StringComparer.Ordinal);

        foreach (KeyValuePair<string, bool> pair in request.Environment.Features)
        {
            features[pair.Key] = pair.Value;
        }

        features[LaunchFeatures.HasCustomResolution] = request.ResolutionWidth.HasValue || request.ResolutionHeight.HasValue;
        features[LaunchFeatures.IsDemoUser] = false;
        features[LaunchFeatures.HasQuickPlaysSupport] = false;
        features[LaunchFeatures.IsQuickPlaySingleplayer] = false;
        features[LaunchFeatures.IsQuickPlayMultiplayer] = false;
        features[LaunchFeatures.IsQuickPlayRealms] = false;

        return new EnvironmentProfile(
            request.Environment.OsName,
            request.Environment.OsArch,
            request.Environment.OsVersion,
            features);
    }
    /// <summary>
    /// 按规则过滤参数条目并摊平成 token 流，返回追加的 token 数。
    ///
    /// 摊平是安全的：真实元数据里 <c>--xuid</c> 与 <c>${auth_xuid}</c> 本来就是两个独立条目，
    /// 所以取值与交付不依赖条目边界；孤儿开关的修复在解析阶段统一处理。
    /// </summary>
    private static int AppendFiltered(
        List<string> target,
        IReadOnlyList<ArgumentEntry> entries,
        EnvironmentProfile environment)
    {
        int appended = 0;

        for (int i = 0; i < entries.Count; i++)
        {
            ArgumentEntry entry = entries[i];

            if (!RuleEvaluator.IsAllowed(entry.Rules, environment))
            {
                continue;
            }

            for (int j = 0; j < entry.Values.Count; j++)
            {
                target.Add(entry.Values[j]);
                appended++;
            }
        }

        return appended;
    }

    /// <summary>收集真正注入的身份相关占位符键，供骨架记录"注入了什么"（只记键，不记值）。</summary>
    private static IReadOnlyList<string> CollectInjectionKeys(
        IReadOnlyList<string> jvm,
        IReadOnlyList<string> game)
    {
        HashSet<string> identityKeys = new HashSet<string>(StringComparer.Ordinal)
        {
            LaunchPlanPlaceholders.AuthPlayerName,
            LaunchPlanPlaceholders.AuthUuid,
            LaunchPlanPlaceholders.AuthAccessToken,
            LaunchPlanPlaceholders.AuthSession,
            LaunchPlanPlaceholders.AuthXuid,
            LaunchPlanPlaceholders.UserProperties,
            LaunchPlanPlaceholders.UserType,
        };

        List<string> found = new List<string>();
        HashSet<string> seen = new HashSet<string>(StringComparer.Ordinal);

        Collect(jvm);
        Collect(game);

        return found;

        void Collect(IReadOnlyList<string> tokens)
        {
            for (int i = 0; i < tokens.Count; i++)
            {
                foreach (string name in FindPlaceholders(tokens[i]))
                {
                    if (identityKeys.Contains(name) && seen.Add(name))
                    {
                        found.Add(name);
                    }
                }
            }
        }
    }

    private static List<string> FindPlaceholders(string token)
    {
        List<string> found = new List<string>();

        if (string.IsNullOrEmpty(token))
        {
            return found;
        }

        int cursor = 0;
        while (cursor < token.Length)
        {
            int start = token.IndexOf(LaunchPlanPlaceholders.Marker, cursor, StringComparison.Ordinal);
            if (start < 0)
            {
                break;
            }

            int nameStart = start + LaunchPlanPlaceholders.Marker.Length;
            int end = token.IndexOf('}', nameStart);
            if (end < 0)
            {
                break;
            }

            found.Add(token.Substring(nameStart, end - nameStart));
            cursor = end + 1;
        }

        return found;
    }

    private static string VersionTypeText(VersionType type)
    {
        switch (type)
        {
            case VersionType.Release: return "release";
            case VersionType.Snapshot: return "snapshot";
            case VersionType.OldBeta: return "old_beta";
            case VersionType.OldAlpha: return "old_alpha";
            default: return "release";
        }
    }

    private static string Text(int? value)
    {
        return value.HasValue ? value.Value.ToString(CultureInfo.InvariantCulture) : string.Empty;
    }

    private static string Combine(string root, string relative)
    {
        string left = root.Replace('/', '\\').TrimEnd('\\');
        string right = relative.Replace('/', '\\').TrimStart('\\');
        return left + "\\" + right;
    }
}
