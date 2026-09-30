using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Launch;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;
using Qul.Domain.Launch;
using Qul.Domain.Metadata;
using Qul.Domain.Runtime;
using Qul.Infrastructure.Metadata;

namespace Qul.Tests.Launch;

/// <summary>
/// P4.0 离线身份 + 启动计划双平面模型。
///
/// 这一组用例里最重要的两条：
///   1) 骨架必须**不随账户与路径变化**——这是"可复现骨架"这个说法的可执行形式；
///   2) 四个真实版本的参数必须能**全部解析完**，不留任何 ${...}——这是占位符清单完整的证明。
/// </summary>
[TestClass]
public sealed class LaunchPlanTests
{
    private static readonly EnvironmentProfile WindowsX64 =
        new EnvironmentProfile(EnvironmentProfile.OsWindows, EnvironmentProfile.ArchX86_64, "10.0.19045");

    private const string CacheRoot = @"C:\qul\data\cache";
    private const string GameDir = @"C:\qul\game";

    private static readonly string[] AllVersions = { "1.7.10", "1.12.2", "1.16.5", "26.3" };

    // ---------- P4.0 离线身份 ----------

    [TestMethod]
    public void OfflineIdentity_IsLocalOnlyAndSaysSo()
    {
        PlayerIdentity identity = OfflineIdentityFactory.Create("Steve");

        Assert.AreEqual(IdentitySource.Offline, identity.Source);
        Assert.AreEqual("Steve", identity.UserName);
        Assert.AreEqual(32, identity.Uuid.Length);
        Assert.IsFalse(identity.IsOnlineVerified, "离线身份不可能是已验证状态");
        Assert.AreEqual(OfflineIdentityFactory.PlaceholderAccessToken, identity.AccessToken);
        Assert.IsTrue(identity.CapabilityNotices.Count >= 3, "必须把能力限制说清楚");
        Assert.IsTrue(
            identity.CapabilityNotices.Any(n => n.Contains("无法进入正版验证")),
            "最要紧的那条告知必须在");
        Assert.IsTrue(identity.CapabilityNotices.Any(n => n.Contains("与 Mojang 官方发放的账号标识无关")));
    }

    [TestMethod]
    public void OfflineIdentity_IsDeterministicAndDistinctPerName()
    {
        Assert.AreEqual(
            OfflineIdentityFactory.ComputeUuid("Steve"),
            OfflineIdentityFactory.ComputeUuid("Steve"),
            "同一个名字必须恒得同一个标识");

        Assert.AreNotEqual(
            OfflineIdentityFactory.ComputeUuid("Steve"),
            OfflineIdentityFactory.ComputeUuid("Alex"));
    }

    [TestMethod]
    public void OfflineIdentity_IsStructurallyDistinguishableFromOfficialIdentifiers()
    {
        string derived = OfflineIdentityFactory.ComputeUuid("Steve");

        Assert.IsTrue(OfflineIdentityFactory.IsDerivedUuid(derived));

        // 官方发放的是 UUID v4，版本位是 4 —— 结构上就不会被误判成本地派生标识。
        const string OfficialStyleV4 = "069a79f444e94726a5befca90e38aaf5";
        Assert.AreEqual('4', OfficialStyleV4[12]);
        Assert.IsFalse(OfflineIdentityFactory.IsDerivedUuid(OfficialStyleV4));
        Assert.IsFalse(OfflineIdentityFactory.IsDerivedUuid(null));
        Assert.IsFalse(OfflineIdentityFactory.IsDerivedUuid("too-short"));
    }

    [TestMethod]
    public void OfflineIdentity_RejectsNamesTheGameWouldNotAccept()
    {
        foreach (string? bad in new[] { null, string.Empty, "   ", "has space", "a-b", "名字", new string('a', 17) })
        {
            LauncherException error = Assert.ThrowsException<LauncherException>(
                () => OfflineIdentityFactory.Create(bad!));
            Assert.AreEqual(ErrorCode.AuthOfflineNameInvalid, error.Code, "非法名字：" + (bad ?? "<null>"));
        }

        Assert.AreEqual(16, OfflineIdentityFactory.Create(new string('a', 16)).UserName.Length);
        Assert.AreEqual("Player_1", OfflineIdentityFactory.Create("  Player_1  ").UserName, "两端空白应被裁掉");
    }

    // ---------- 骨架：可复现 ----------

    [TestMethod]
    public void Skeleton_DoesNotChangeWithAccountOrPaths()
    {
        LaunchPlan first = BuildPlan("1.16.5", OfflineIdentityFactory.Create("Alice"), @"C:\a\cache", @"C:\a\game");
        LaunchPlan second = BuildPlan("1.16.5", OfflineIdentityFactory.Create("Bob"), @"D:\b\cache", @"D:\b\game");

        Assert.AreEqual(
            first.Skeleton.ToCanonicalText(),
            second.Skeleton.ToCanonicalText(),
            "骨架绝不该随账户、令牌或路径变化——它只由版本与平台决定");
        Assert.AreEqual(first.Skeleton.ComputeHash(), second.Skeleton.ComputeHash());

        // 而秘密平面必须真的不同，否则上面那条断言就是在测空气。
        Assert.AreNotEqual(
            first.Secrets.Values[LaunchPlanPlaceholders.AuthPlayerName],
            second.Secrets.Values[LaunchPlanPlaceholders.AuthPlayerName]);
        Assert.AreNotEqual(
            first.Secrets.Values[LaunchPlanPlaceholders.ClassPath],
            second.Secrets.Values[LaunchPlanPlaceholders.ClassPath]);
    }

    [TestMethod]
    public void Skeleton_IsByteIdenticalAcrossRepeatedBuilds()
    {
        LaunchPlan first = BuildPlan("26.3");
        LaunchPlan second = BuildPlan("26.3");

        Assert.AreEqual(first.Skeleton.ToCanonicalText(), second.Skeleton.ToCanonicalText());
        Assert.AreEqual(first.Skeleton.ComputeHash(), second.Skeleton.ComputeHash());
    }

    [TestMethod]
    public void Skeleton_ContainsNoSecretsAndNoAbsolutePaths()
    {
        LaunchPlan plan = BuildPlan("1.16.5", OfflineIdentityFactory.Create("SecretPlayer"));

        string canonical = plan.Skeleton.ToCanonicalText();

        Assert.IsFalse(canonical.Contains("SecretPlayer"), "骨架里不得出现用户名");
        Assert.IsFalse(canonical.Contains(CacheRoot), "骨架里不得出现绝对路径");
        Assert.IsFalse(canonical.Contains("\\"), "骨架里的路径必须是相对形式");
        Assert.IsTrue(canonical.Contains("${" + LaunchPlanPlaceholders.AuthAccessToken + "}"), "令牌只能是占位符");

        // 骨架里的类路径项应当是相对路径。
        Assert.IsTrue(plan.Skeleton.ClassPathEntries.All(e => !e.Contains(":")));
    }

    [TestMethod]
    public void Skeleton_RecordsWhatWasInjectedWithoutRecordingTheValues()
    {
        LaunchPlan plan = BuildPlan("1.16.5", OfflineIdentityFactory.Create("Steve"));

        Assert.IsTrue(plan.Skeleton.InjectionKeys.Contains(LaunchPlanPlaceholders.AuthAccessToken));
        Assert.IsTrue(plan.Skeleton.InjectionKeys.Contains(LaunchPlanPlaceholders.AuthUuid));
        Assert.IsFalse(
            plan.Skeleton.InjectionKeys.Contains(LaunchPlanPlaceholders.AuthXuid),
            "离线账户不注入 xuid，骨架也不该声称注入了");
    }

    // ---------- 解析与丢弃规则 ----------

    [TestMethod]
    public void Resolve_DropsTheWholeEntryWhenAValueIsEmpty()
    {
        // 真实元数据里 --xuid 与 ${auth_xuid} 是**两个独立条目**；值解析为空时若只丢值，
        // 开关会孤零零留下，游戏就会把下一个参数当成它的值。
        LaunchPlanSkeleton skeleton = new LaunchPlanSkeleton
        {
            VersionId = "x",
            GameArgumentTemplate = new[]
            {
                "--username", "${auth_player_name}",
                "--xuid", "${auth_xuid}",
                "--quickPlayPath", "${quickPlayPath}",
            },
        };

        LaunchPlanSecrets secrets = new LaunchPlanSecrets
        {
            Values = new Dictionary<string, string>(StringComparer.Ordinal)
            {
                [LaunchPlanPlaceholders.AuthPlayerName] = "Player",
                [LaunchPlanPlaceholders.AuthXuid] = string.Empty,
                [LaunchPlanPlaceholders.QuickPlayPath] = string.Empty,
            },
        };

        List<string> arguments = new LaunchPlan(skeleton, secrets).ResolveGameArguments().ToList();

        CollectionAssert.AreEqual(
            new[] { "--username", "Player" },
            arguments,
            "取值为空的参数必须整条丢弃");
    }

    [TestMethod]
    public void Resolve_ThrowsOnUnresolvedOrUnknownPlaceholders()
    {
        LaunchPlanSkeleton missingValue = new LaunchPlanSkeleton
        {
            GameArgumentTemplate = new[] { "--uuid", "${auth_uuid}" },
        };

        LauncherException unresolved = Assert.ThrowsException<LauncherException>(
            () => new LaunchPlan(missingValue, new LaunchPlanSecrets()).ResolveGameArguments());
        Assert.AreEqual(ErrorCode.PlanUnresolvedPlaceholder, unresolved.Code);

        LaunchPlanSkeleton unknownName = new LaunchPlanSkeleton
        {
            GameArgumentTemplate = new[] { "${someFutureThing}" },
        };

        LauncherException unknown = Assert.ThrowsException<LauncherException>(
            () => new LaunchPlan(
                unknownName,
                new LaunchPlanSecrets
                {
                    Values = new Dictionary<string, string>(StringComparer.Ordinal) { ["someFutureThing"] = "x" },
                }).ResolveGameArguments());
        Assert.AreEqual(ErrorCode.PlanUnresolvedPlaceholder, unknown.Code);
    }

    [TestMethod]
    public void RedactedExport_LeaksNothingSensitive()
    {
        // 必须用一个**可辨识**的令牌：离线占位令牌是 "0"，拿它断言"导出里不含令牌"
        // 会因为文本里到处是 0 而永远不成立——那种断言等于没写。
        PlayerIdentity identity = new PlayerIdentity(
            IdentitySource.Microsoft, "SecretPlayer", "069a79f444e94726a5befca90e38aaf5")
        {
            AccessToken = "SECRET-TOKEN-9f3a1c",
            UserType = "msa",
            IsOnlineVerified = true,
        };

        LaunchPlan plan = BuildPlan("1.16.5", identity);

        string text = plan.ToRedactedText();

        Assert.IsFalse(text.Contains("SecretPlayer"), "导出里不得出现用户名");
        Assert.IsFalse(text.Contains(CacheRoot), "导出里不得出现绝对路径");
        Assert.IsFalse(text.Contains(GameDir), "导出里不得出现游戏目录");
        Assert.IsFalse(text.Contains("SECRET-TOKEN-9f3a1c"), "导出里不得出现访问令牌");
        Assert.IsFalse(text.Contains("069a79f444e94726a5befca90e38aaf5"), "导出里不得出现账号标识");
        Assert.IsFalse(text.Contains(plan.Secrets.Values[LaunchPlanPlaceholders.ClassPath]), "导出里不得出现类路径");

        // 但结构信息要留着，否则导出就没法用来排障了。
        StringAssert.Contains(text, "<" + LaunchPlanPlaceholders.AuthPlayerName + ">");
        StringAssert.Contains(text, "${" + LaunchPlanPlaceholders.AuthAccessToken + "}");
        StringAssert.Contains(text, "version=1.16.5");
    }

    // ---------- 四个真实版本的端到端组装 ----------

    [TestMethod]
    public void EveryRealVersion_ResolvesEveryArgumentWithoutLeftoverPlaceholders()
    {
        foreach (string id in AllVersions)
        {
            LaunchPlan plan = BuildPlan(id);

            IReadOnlyList<string> jvm = plan.ResolveJvmArguments();
            IReadOnlyList<string> game = plan.ResolveGameArguments();

            Assert.IsTrue(jvm.Count > 0, id + "：JVM 参数不该为空");
            Assert.IsTrue(game.Count > 0, id + "：游戏参数不该为空");
            Assert.IsFalse(jvm.Any(a => a.Contains("${")), id + "：JVM 参数残留占位符");
            Assert.IsFalse(game.Any(a => a.Contains("${")), id + "：游戏参数残留占位符");

            Assert.IsTrue(plan.Skeleton.ClassPathEntries.Count > 1, id + "：类路径应包含库与客户端 jar");
            StringAssert.EndsWith(
                plan.Skeleton.ClassPathEntries[plan.Skeleton.ClassPathEntries.Count - 1],
                "client-" + id + ".jar",
                id + "：客户端 jar 必须排在类路径最后");
        }
    }

    [TestMethod]
    public void Version1_7_10_UsesLegacyArgumentsAndSynthesizedJvmArguments()
    {
        LaunchPlan plan = BuildPlan("1.7.10");
        IReadOnlyList<string> jvm = plan.ResolveJvmArguments();
        IReadOnlyList<string> game = plan.ResolveGameArguments();

        // 旧式元数据没有 arguments.jvm，必须自己补上这三条。
        Assert.IsTrue(jvm.Contains("-cp"), "旧式版本要自己补 -cp");
        Assert.IsTrue(jvm.Any(a => a.StartsWith("-Djava.library.path=", StringComparison.Ordinal)));

        Assert.IsTrue(game.Contains("--username"));
        Assert.IsTrue(game.Contains("--accessToken"));
        Assert.IsTrue(game.Contains("--userProperties"));
        Assert.AreEqual("{}", game[game.ToList().IndexOf("--userProperties") + 1]);

        // 1.7.10 的 natives 容器没有主 artifact，不该出现在类路径里。
        Assert.IsFalse(
            plan.Skeleton.ClassPathEntries.Any(e => e.Contains("lwjgl-platform") && e.EndsWith(".jar")),
            "只有 classifier 内容的 natives 容器不参与类路径");
    }

    [TestMethod]
    public void Version26_3_OmitsXuidAndNeverLeavesAnOrphanFlag()
    {
        LaunchPlan plan = BuildPlan("26.3");
        List<string> game = plan.ResolveGameArguments().ToList();

        Assert.IsTrue(game.Contains("--clientId"), "启动器自己的 clientId 是真实的，应当保留");
        Assert.IsTrue(game.Contains("qinmo-ultimate-launcher"));

        // --xuid 与 ${auth_xuid} 在元数据里是**两个独立条目**。离线账户没有 Xbox 标识，
        // 值解析为空时必须连同开关一起消失，否则游戏会把下一个参数当成 xuid。
        Assert.IsFalse(game.Contains("--xuid"), "离线账户没有 Xbox 标识，该参数应连同开关一起省略");

        // 快速进入四项与分辨率由 feature 门控（见下一条用例），未声明时本就不该出现。
        Assert.IsFalse(game.Contains("--quickPlayPath"));
        Assert.IsFalse(game.Contains("--quickPlaySingleplayer"));
        Assert.IsFalse(game.Contains("--width"));
        Assert.IsFalse(game.Contains("--height"));
    }

    [TestMethod]
    public void FeatureGatedArguments_RequireTheFeatureToBeDeclared()
    {
        // 实测：--demo 的规则是 features.is_demo_user = true。
        // MVP 不声明试玩特性，所以这条参数根本进不了计划——不是"取值为空被省略"。
        Assert.IsFalse(BuildPlan("26.3").ResolveGameArguments().Contains("--demo"), "MVP 不声明试玩特性");
        Assert.IsFalse(BuildPlan("1.16.5").ResolveGameArguments().Contains("--demo"));

        // 而分辨率一旦真的设置，组装器就必须把 has_custom_resolution 翻成 true，
        // 否则 --width/--height 会被规则挡在门外，用户设了分辨率却没有任何效果。
        Assert.IsTrue(BuildPlan("26.3", width: 1280, height: 720).ResolveGameArguments().Contains("--width"));
        Assert.IsTrue(BuildPlan("1.16.5", width: 1280, height: 720).ResolveGameArguments().Contains("--width"));
    }

    [TestMethod]
    public void Version26_3_KeepsResolutionArgumentsWhenTheyAreActuallySet()
    {
        LaunchPlan plan = BuildPlan("26.3", width: 854, height: 480);
        List<string> game = plan.ResolveGameArguments().ToList();

        Assert.IsTrue(game.Contains("--width"));
        Assert.IsTrue(game.Contains("--height"));
        Assert.AreEqual("854", game[game.IndexOf("--width") + 1]);
        Assert.AreEqual("480", game[game.IndexOf("--height") + 1]);
    }

    [TestMethod]
    public void Build_IncludesMemoryLoggingAndUserArgumentsInTheRightPlaces()
    {
        LaunchPlan plan = BuildPlan("1.16.5", memory: 4096, loggingPath: @"C:\qul\data\cache\logging\client-1.16.5.xml");
        IReadOnlyList<string> jvm = plan.ResolveJvmArguments();

        Assert.AreEqual("-Xmx4096M", jvm[0], "内存参数应排在最前，便于一眼看到");
        Assert.IsTrue(jvm.Any(a => a.StartsWith("-Dlog4j.configurationFile=", StringComparison.Ordinal)));

        LaunchPlan withoutLogging = BuildPlan("1.16.5");
        Assert.IsFalse(
            withoutLogging.ResolveJvmArguments().Any(a => a.StartsWith("-Dlog4j.configurationFile=", StringComparison.Ordinal)),
            "没有日志配置文件时不该产生一个指向空路径的属性");
    }

    [TestMethod]
    public void Build_RejectsAnInvalidVersionBeforeProducingAPlan()
    {
        LauncherException error = Assert.ThrowsException<LauncherException>(
            () => LaunchPlanBuilder.Build(new LaunchPlanRequest
            {
                Version = new VersionDetail { Id = "broken" },
                CacheRoot = CacheRoot,
                GameDirectory = GameDir,
            }));

        Assert.AreEqual(ErrorCode.MetaVersionInvalid, error.Code);
    }

    // ---------- 工具 ----------

    private static string ReadGolden(string fileName)
    {
        string path = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "Golden", fileName);
        Assert.IsTrue(File.Exists(path), "缺少黄金样本：" + path);
        return File.ReadAllText(path, Encoding.UTF8);
    }

    private static LaunchPlan BuildPlan(
        string versionId,
        PlayerIdentity? identity = null,
        string cacheRoot = CacheRoot,
        string gameDirectory = GameDir,
        int? memory = null,
        string? loggingPath = null,
        int? width = null,
        int? height = null)
    {
        VersionDetail version = VersionMetadataParser.ParseVersion(
            ReadGolden("version-" + versionId + ".json"), versionId);

        return LaunchPlanBuilder.Build(new LaunchPlanRequest
        {
            Version = version,
            Environment = WindowsX64,
            Java = Java(@"C:\Program Files\Java\jdk-17.0.20\bin\java.exe", "17.0.20", 64),
            Identity = identity ?? OfflineIdentityFactory.Create("Player"),
            CacheRoot = cacheRoot,
            GameDirectory = gameDirectory,
            NativesDirectory = cacheRoot + @"\natives",
            LoggingConfigPath = loggingPath,
            MaxMemoryMb = memory,
            ResolutionWidth = width,
            ResolutionHeight = height,
        });
    }

    private static JavaRuntimeCandidate Java(string path, string version, int bitness)
    {
        Assert.IsTrue(JavaVersion.TryParse(version, out JavaVersion parsed));

        return new JavaRuntimeCandidate
        {
            ExecutablePath = path,
            Version = parsed,
            OsArch = "amd64",
            Bitness = bitness,
        };
    }
}
