using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Diagnostics;
using Qul.Domain.Metadata;
using Qul.Infrastructure.Metadata;

namespace Qul.Tests.Metadata;

/// <summary>
/// 领域层纯单元测试：规则求值、继承合并、坐标解析、解析器边界。
/// 这些用例不需要网络也不需要黄金样本——它们锁的是语义，不是数据。
/// </summary>
[TestClass]
public sealed class MetadataUnitTests
{
    private static readonly EnvironmentProfile WindowsOs =
        new EnvironmentProfile(EnvironmentProfile.OsWindows, EnvironmentProfile.ArchX86_64, "10.0.19045");

    private static EnvironmentProfile Osx => new EnvironmentProfile(EnvironmentProfile.OsOsx, EnvironmentProfile.ArchArm64);

    private static LibraryRef Lib(string coordinate)
    {
        LibraryName.TryParse(coordinate, out LibraryName? name);
        return new LibraryRef { RawName = coordinate, Name = name };
    }

    // ---------- Maven 坐标 ----------

    [TestMethod]
    public void LibraryName_ParsesAllRealCoordinateShapes()
    {
        Assert.IsTrue(LibraryName.TryParse("com.mojang:netty:1.8.8", out LibraryName? plain));
        Assert.AreEqual("com.mojang", plain!.GroupId);
        Assert.AreEqual("netty", plain.ArtifactId);
        Assert.AreEqual("1.8.8", plain.Version);
        Assert.IsNull(plain.Classifier);
        Assert.AreEqual("jar", plain.Extension);
        Assert.AreEqual("com.mojang:netty", plain.Key);

        Assert.IsTrue(LibraryName.TryParse("org.lwjgl:lwjgl:3.2.1:natives-windows", out LibraryName? natives));
        Assert.AreEqual("natives-windows", natives!.Classifier);
        Assert.IsTrue(natives.HasClassifier);
        Assert.AreEqual("org.lwjgl:lwjgl:natives-windows", natives.Key, "带 classifier 的条目必须与主 artifact 区分");

        // 26.x 真实坐标：classifier 里还带架构。
        Assert.IsTrue(LibraryName.TryParse("com.mojang:jtracy:1.14.38:natives-macos-arm64", out LibraryName? arm));
        Assert.AreEqual("natives-macos-arm64", arm!.Classifier);

        Assert.IsTrue(LibraryName.TryParse("foo:bar:1.0@zip", out LibraryName? zipped));
        Assert.AreEqual("zip", zipped!.Extension);
        Assert.AreEqual("foo:bar:1.0@zip", zipped.ToString());
    }

    [TestMethod]
    public void LibraryName_RejectsMalformedCoordinates()
    {
        Assert.IsFalse(LibraryName.TryParse("only:two", out _));
        Assert.IsFalse(LibraryName.TryParse("a:b:c:d:e", out _));
        Assert.IsFalse(LibraryName.TryParse("a::1.0", out _));
        Assert.IsFalse(LibraryName.TryParse("", out _));
        Assert.IsFalse(LibraryName.TryParse(null, out _));
        Assert.IsFalse(LibraryName.TryParse("   ", out _));
    }

    // ---------- 规则求值 ----------

    [TestMethod]
    public void NoRules_MeansAllowed()
    {
        Assert.IsTrue(RuleEvaluator.IsAllowed(Array.Empty<Rule>(), WindowsOs));
        Assert.IsTrue(RuleEvaluator.IsAllowed(null, WindowsOs));
    }

    [TestMethod]
    public void AllowRuleForAnotherPlatform_DeniesOnThisPlatform()
    {
        Rule[] rules = { new Rule { Action = RuleAction.Allow, OsName = "osx" } };

        Assert.IsFalse(RuleEvaluator.IsAllowed(rules, WindowsOs), "有规则但一条都没命中，结果是拒绝");
        Assert.IsTrue(RuleEvaluator.IsAllowed(rules, Osx));
    }

    [TestMethod]
    public void LastMatchingRuleWins()
    {
        Rule[] rules =
        {
            new Rule { Action = RuleAction.Allow },
            new Rule { Action = RuleAction.Disallow, OsName = "osx" },
        };

        Assert.IsTrue(RuleEvaluator.IsAllowed(rules, WindowsOs));
        Assert.IsFalse(RuleEvaluator.IsAllowed(rules, Osx), "后面的 disallow 必须覆盖前面的 allow");
    }

    [TestMethod]
    public void FeatureRules_RequireExactFeatureState()
    {
        Rule[] requiresDemo =
        {
            new Rule
            {
                Action = RuleAction.Allow,
                Features = new Dictionary<string, bool>(StringComparer.Ordinal) { { "is_demo_user", true } },
            },
        };

        Assert.IsFalse(RuleEvaluator.IsAllowed(requiresDemo, WindowsOs), "环境未声明该特性时不得命中");

        EnvironmentProfile demo = new EnvironmentProfile(
            EnvironmentProfile.OsWindows,
            EnvironmentProfile.ArchX86_64,
            null,
            new Dictionary<string, bool>(StringComparer.Ordinal) { { "is_demo_user", true } });
        Assert.IsTrue(RuleEvaluator.IsAllowed(requiresDemo, demo));

        Rule[] requiresNoCustomResolution =
        {
            new Rule
            {
                Action = RuleAction.Allow,
                Features = new Dictionary<string, bool>(StringComparer.Ordinal) { { "has_custom_resolution", false } },
            },
        };

        Assert.IsTrue(
            RuleEvaluator.IsAllowed(requiresNoCustomResolution, WindowsOs),
            "要求特性为 false 时，环境未声明即算满足");
    }

    [TestMethod]
    public void ArchMatching_AcceptsX86OnX86_64Only()
    {
        Assert.IsTrue(RuleEvaluator.ArchMatches("x86", "x86_64"), "32 位规则在 64 位环境下应生效");
        Assert.IsTrue(RuleEvaluator.ArchMatches("X86_64", "x86_64"));
        Assert.IsFalse(RuleEvaluator.ArchMatches("arm64", "x86_64"));
        Assert.IsFalse(RuleEvaluator.ArchMatches("x86_64", "x86"), "反向不成立：64 位规则不得在 32 位环境生效");
    }

    [TestMethod]
    public void OsVersionRule_IsRegexAndNeverThrows()
    {
        Assert.IsTrue(RuleEvaluator.VersionMatches("^10\\.", "10.0.19045"));
        Assert.IsFalse(RuleEvaluator.VersionMatches("^11\\.", "10.0.19045"));
        Assert.IsFalse(RuleEvaluator.VersionMatches("[unclosed", "10.0.19045"), "畸形正则必须判为不命中，而不是抛穿");
        Assert.IsFalse(RuleEvaluator.VersionMatches("^10\\.", null), "环境版本未知时判为不命中");
    }

    // ---------- 继承合并 ----------

    [TestMethod]
    public void Merge_TakesMissingFieldsFromParent()
    {
        VersionDetail parent = new VersionDetail
        {
            Id = "base",
            MainClass = "BaseMain",
            Assets = "1.16",
            JavaVersion = new JavaVersionRequirement { MajorVersion = 8 },
            ClientDownload = new DownloadRef("https://example.invalid/base.jar"),
        };

        VersionDetail child = new VersionDetail { Id = "child", InheritsFrom = "base" };

        VersionDetail merged = VersionResolver.Merge(child, parent);

        Assert.AreEqual("child", merged.Id);
        Assert.AreEqual("BaseMain", merged.MainClass);
        Assert.AreEqual("1.16", merged.Assets);
        Assert.AreEqual(8, merged.JavaVersion?.MajorVersion);
        Assert.IsNull(merged.InheritsFrom, "合并后不应再保留继承指向");
    }

    [TestMethod]
    public void Merge_ChildOverridesParentLibraryAndChildEntriesComeLast()
    {
        VersionDetail parent = new VersionDetail
        {
            Id = "p",
            Libraries = new[] { Lib("a:a:1.0"), Lib("b:b:1.0"), Lib("c:c:1.0") },
        };

        VersionDetail child = new VersionDetail
        {
            Id = "c",
            InheritsFrom = "p",
            Libraries = new[] { Lib("b:b:2.0") },
        };

        IReadOnlyList<LibraryRef> libraries = VersionResolver.Merge(child, parent).Libraries;

        // 子列表原样保留并排在最后；父列表里被覆盖的那条消失，其余保持原顺序。
        Assert.AreEqual(3, libraries.Count);
        Assert.AreEqual("a:a:1.0", libraries[0].RawName);
        Assert.AreEqual("c:c:1.0", libraries[1].RawName);
        Assert.AreEqual("b:b:2.0", libraries[2].RawName, "子版本必须覆盖同身份的父库");
    }

    [TestMethod]
    public void MergeLibraries_NeverDeduplicatesInsideASingleList()
    {
        // 1.16.5 里 org.lwjgl:lwjgl:3.2.1（osx 专用）与 3.2.2（win/linux 专用）在同一份列表里合法共存。
        // 单份列表内部去重会把它们误杀，症状是运行时缺本地库。
        LibraryRef osxOnly = Lib("org.lwjgl:lwjgl:3.2.1");
        LibraryRef nonOsx = Lib("org.lwjgl:lwjgl:3.2.2");

        IReadOnlyList<LibraryRef> kept =
            VersionResolver.MergeLibraries(Array.Empty<LibraryRef>(), new[] { osxOnly, nonOsx });

        Assert.AreEqual(2, kept.Count, "单份列表内部绝不去重");
        Assert.AreEqual(
            osxOnly.IdentityKey,
            nonOsx.IdentityKey,
            "两者身份相同（版本号不参与身份）——正因如此才必须禁止列表内去重");
    }

    [TestMethod]
    public void Merge_KeepsVariantsThatShareACoordinate()
    {
        // 同一个坐标、只有 natives 不同的两个条目是不同实体，不得互相顶掉。
        LibraryRef plain = Lib("com.mojang:text2speech:1.10.3");
        LibraryRef withNatives = Lib("com.mojang:text2speech:1.10.3");
        withNatives.Natives = new Dictionary<string, string>(StringComparer.Ordinal) { { "windows", "natives-windows" } };

        IReadOnlyList<LibraryRef> libraries =
            VersionResolver.MergeLibraries(Array.Empty<LibraryRef>(), new[] { plain, withNatives });

        Assert.AreEqual(2, libraries.Count, "同坐标的 natives 变体必须保留");
        Assert.AreNotEqual(plain.IdentityKey, withNatives.IdentityKey);
    }

    [TestMethod]
    public void Merge_ConcatenatesArgumentsParentFirst()
    {
        VersionDetail parent = new VersionDetail
        {
            Id = "p",
            JvmArguments = new[] { new ArgumentEntry { Values = new[] { "-Xmx1G" } } },
        };

        VersionDetail child = new VersionDetail
        {
            Id = "c",
            InheritsFrom = "p",
            JvmArguments = new[] { new ArgumentEntry { Values = new[] { "-Dfoo=bar" } } },
        };

        List<string> values = VersionResolver.Merge(child, parent).JvmArguments
            .SelectMany(a => a.Values)
            .ToList();

        CollectionAssert.AreEqual(new[] { "-Xmx1G", "-Dfoo=bar" }, values, "父参数在前、子参数在后");
    }

    [TestMethod]
    public void Resolve_FoldsInheritChainFromRootToLeaf()
    {
        Dictionary<string, VersionDetail> byId = new Dictionary<string, VersionDetail>(StringComparer.Ordinal)
        {
            ["root"] = new VersionDetail { Id = "root", MainClass = "RootMain", Assets = "1.0" },
            ["mid"] = new VersionDetail { Id = "mid", InheritsFrom = "root", MainClass = "MidMain" },
            ["leaf"] = new VersionDetail { Id = "leaf", InheritsFrom = "mid" },
        };

        VersionDetail resolved = VersionResolver.Resolve(
            byId["leaf"],
            id => byId.TryGetValue(id, out VersionDetail? found) ? found : null);

        Assert.AreEqual("leaf", resolved.Id);
        Assert.AreEqual("MidMain", resolved.MainClass, "中间层应覆盖根层");
        Assert.AreEqual("1.0", resolved.Assets, "叶层缺失时回落到根层");
        Assert.IsNull(resolved.InheritsFrom);
    }

    [TestMethod]
    public void Resolve_RejectsCyclesAndMissingParents()
    {
        Dictionary<string, VersionDetail> cyclic = new Dictionary<string, VersionDetail>(StringComparer.Ordinal)
        {
            ["a"] = new VersionDetail { Id = "a", InheritsFrom = "b" },
            ["b"] = new VersionDetail { Id = "b", InheritsFrom = "a" },
        };

        LauncherException cycle = Assert.ThrowsException<LauncherException>(
            () => VersionResolver.Resolve(cyclic["a"], id => cyclic.TryGetValue(id, out VersionDetail? v) ? v : null));
        Assert.AreEqual(ErrorCode.MetaInheritBroken, cycle.Code);

        Dictionary<string, VersionDetail> orphan = new Dictionary<string, VersionDetail>(StringComparer.Ordinal)
        {
            ["x"] = new VersionDetail { Id = "x", InheritsFrom = "nope" },
        };

        LauncherException missing = Assert.ThrowsException<LauncherException>(
            () => VersionResolver.Resolve(orphan["x"], id => orphan.TryGetValue(id, out VersionDetail? v) ? v : null));
        Assert.AreEqual(ErrorCode.MetaInheritBroken, missing.Code);
    }

    // ---------- 校验 ----------

    [TestMethod]
    public void Validate_RequiresIdMainClassAndClientDownload()
    {
        Assert.AreEqual(ErrorCode.MetaVersionInvalid, VersionResolver.Validate(new VersionDetail())!.Code);
        Assert.AreEqual(
            ErrorCode.MetaVersionInvalid,
            VersionResolver.Validate(new VersionDetail { Id = "x" })!.Code);

        Assert.IsNull(VersionResolver.Validate(new VersionDetail
        {
            Id = "x",
            MainClass = "M",
            ClientDownload = new DownloadRef("https://example.invalid/client.jar"),
        }));
    }

    [TestMethod]
    public void Validate_RejectsDeclaringBothArgumentForms()
    {
        VersionDetail both = new VersionDetail
        {
            Id = "x",
            MainClass = "M",
            ClientDownload = new DownloadRef("https://example.invalid/client.jar"),
            MinecraftArguments = "--username ${auth_player_name}",
            GameArguments = new[] { new ArgumentEntry { Values = new[] { "--demo" } } },
        };

        LauncherException? error = VersionResolver.Validate(both);

        Assert.IsNotNull(error);
        Assert.AreEqual(ErrorCode.MetaVersionInvalid, error!.Code);
    }

    // ---------- 解析器边界 ----------

    [TestMethod]
    public void Parser_RejectsMalformedMismatchedAndEmptyInput()
    {
        Assert.ThrowsException<LauncherException>(() => VersionMetadataParser.ParseVersion("{ not json"));
        Assert.ThrowsException<LauncherException>(() => VersionMetadataParser.ParseVersion("[1,2]"));
        Assert.ThrowsException<LauncherException>(() => VersionMetadataParser.ParseVersion("{}"));
        Assert.ThrowsException<LauncherException>(
            () => VersionMetadataParser.ParseVersion("{\"id\":\"1.16.5\"}", "1.7.10"));

        Assert.ThrowsException<LauncherException>(() => VersionMetadataParser.ParseManifest("{}"));
        Assert.ThrowsException<LauncherException>(() => VersionMetadataParser.ParseManifest("{\"versions\":[]}"));
    }

    [TestMethod]
    public void Parser_CollectsUnknownTopLevelFieldsWithoutFailing()
    {
        List<string> unknown = new List<string>();

        VersionDetail detail = VersionMetadataParser.ParseVersion(
            "{\"id\":\"x\",\"mainClass\":\"M\",\"futureThing\":{\"a\":1},\"anotherNew\":[1]}",
            null,
            unknown);

        Assert.AreEqual("x", detail.Id);
        CollectionAssert.AreEquivalent(new[] { "futureThing", "anotherNew" }, unknown);
    }

    [TestMethod]
    public void Parser_SkipsMalformedEntriesInsteadOfFailing()
    {
        // 单条坏数据不该毁掉整份清单。
        VersionManifest manifest = VersionMetadataParser.ParseManifest(
            "{\"latest\":{\"release\":\"r\",\"snapshot\":\"s\"},\"versions\":[" +
            "{\"id\":\"good\",\"url\":\"https://example.invalid/g.json\",\"type\":\"release\"}," +
            "{\"id\":\"no-url\"}," +
            "{\"url\":\"https://example.invalid/no-id.json\"}," +
            "\"not-an-object\"]}");

        Assert.AreEqual(1, manifest.Versions.Count);
        Assert.AreEqual("good", manifest.Versions[0].Id);
        Assert.AreEqual(VersionType.Release, manifest.Versions[0].Type);
    }

    [TestMethod]
    public void Parser_SkipsRulesWithUnrecognizedAction()
    {
        // 动作无法识别时整条规则跳过：既不能当 allow 放行，也不能当 disallow 拦截。
        VersionDetail detail = VersionMetadataParser.ParseVersion(
            "{\"id\":\"x\",\"libraries\":[{\"name\":\"a:a:1.0\",\"rules\":[" +
            "{\"action\":\"maybe\",\"os\":{\"name\":\"windows\"}}," +
            "{\"action\":\"allow\"}]}]}");

        Assert.AreEqual(1, detail.Libraries.Count);
        Assert.AreEqual(1, detail.Libraries[0].Rules.Count, "无法识别的动作必须被丢掉");
        Assert.AreEqual(RuleAction.Allow, detail.Libraries[0].Rules[0].Action);
    }

    [TestMethod]
    public void Parser_ExpandsStringAndArrayArgumentValues()
    {
        VersionDetail detail = VersionMetadataParser.ParseVersion(
            "{\"id\":\"x\",\"arguments\":{\"jvm\":[" +
            "\"-Djava.library.path=${natives_directory}\"," +
            "{\"rules\":[{\"action\":\"allow\",\"os\":{\"name\":\"osx\"}}],\"value\":[\"-XstartOnFirstThread\",\"-Xdock:name=Minecraft\"]}" +
            "]}}");

        Assert.AreEqual(2, detail.JvmArguments.Count);
        Assert.AreEqual("-Djava.library.path=${natives_directory}", detail.JvmArguments[0].Values[0]);
        Assert.AreEqual(2, detail.JvmArguments[1].Values.Count, "数组形态的 value 必须被摊平成多项");
        Assert.IsTrue(detail.JvmArguments[1].HasRules);
    }
}
