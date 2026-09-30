using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Assets;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Domain.Metadata;
using Qul.Domain.Security;
using Qul.Infrastructure.Metadata;
using Qul.Infrastructure.Platform;

namespace Qul.Tests.Downloads;

/// <summary>
/// 下载计划组装测试。
///
/// 这一组用例的价值在于**把 P1 与 P2 钉在一起**：
/// 计划里的每个条目都必须带校验值，否则下载引擎会按设计拒绝它（QUL-DL-0003）。
/// 换句话说，"元数据解析得对不对"在这里被"计划能不能真的被执行"再验证了一次。
/// </summary>
[TestClass]
public sealed class DownloadPlanBuilderTests
{
    private static readonly EnvironmentProfile WindowsX64 =
        new EnvironmentProfile(EnvironmentProfile.OsWindows, EnvironmentProfile.ArchX86_64, "10.0.19045");

    private static readonly EnvironmentProfile WindowsX86 =
        new EnvironmentProfile(EnvironmentProfile.OsWindows, EnvironmentProfile.ArchX86, "10.0.19045");

    private static readonly EnvironmentProfile LinuxX64 =
        new EnvironmentProfile(EnvironmentProfile.OsLinux, EnvironmentProfile.ArchX86_64);

    private static readonly string[] AllVersions = { "1.7.10", "1.12.2", "1.16.5", "26.3" };

    private static string ReadGolden(string fileName)
    {
        string path = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "Golden", fileName);
        Assert.IsTrue(File.Exists(path), "缺少黄金样本：" + path);
        return File.ReadAllText(path, Encoding.UTF8);
    }

    private static VersionDetail LoadVersion(string id)
    {
        return VersionMetadataParser.ParseVersion(ReadGolden("version-" + id + ".json"), id);
    }

    private static List<string> ClassifierPaths(
        DownloadPlanBuilder builder,
        VersionDetail version,
        EnvironmentProfile environment)
    {
        return builder.Build(version, environment).Items
            .Where(i => i.Kind == DownloadItemKind.LibraryClassifier)
            .Select(i => i.RelativePath)
            .ToList();
    }

    // ---------- 跨阶段交叉校验 ----------

    [TestMethod]
    public void EveryRealVersion_ProducesAPlanWhereEveryItemCanActuallyBeDownloaded()
    {
        DownloadPlanBuilder builder = new DownloadPlanBuilder();

        foreach (string id in AllVersions)
        {
            DownloadPlan plan = builder.Build(LoadVersion(id), WindowsX64);

            Assert.IsTrue(plan.Items.Count > 0, id + "：计划不该是空的");

            foreach (DownloadItem item in plan.Items)
            {
                // 没有校验值的条目在引擎里会被直接拒绝，所以计划本身就不该产出这种条目。
                Assert.IsTrue(
                    Sha1Hex.TryParse(item.Sha1, out _),
                    id + "：" + item.Describe() + " 缺少可用的 SHA-1，下载时会被拒绝");

                Assert.IsFalse(string.IsNullOrWhiteSpace(item.Url), id + "：" + item.Describe() + " 缺少 URL");
                StringAssert.StartsWith(item.Url, "https://", id + "：" + item.Describe());
                Assert.IsFalse(string.IsNullOrWhiteSpace(item.RelativePath), id + "：" + item.Describe() + " 缺少相对路径");
                Assert.IsFalse(
                    item.RelativePath.StartsWith("/", StringComparison.Ordinal)
                    || item.RelativePath.Contains(":"),
                    id + "：相对路径不得是绝对路径或含盘符 —— " + item.RelativePath);
            }
        }
    }

    [TestMethod]
    public void EveryRealVersion_PlanHasNoDuplicatePathsAndNoUnsubstitutedPlaceholders()
    {
        DownloadPlanBuilder builder = new DownloadPlanBuilder();

        foreach (string id in AllVersions)
        {
            DownloadPlan plan = builder.Build(LoadVersion(id), WindowsX64);

            // 重复路径本身合法（同哈希的资源对象、指向同一 artifact 的重复库条目），
            // 真正危险的是"同一路径指向不同内容"——并发下载会互相覆盖。
            Assert.AreEqual(
                0,
                plan.ConflictingPaths.Count,
                id + "：同一路径出现了内容不一致的条目：" + string.Join(", ", plan.ConflictingPaths));
            Assert.AreEqual(
                plan.Items.Count,
                plan.Items.Select(i => i.RelativePath).Distinct(StringComparer.Ordinal).Count(),
                id + "：计划内的相对路径必须两两不同");

            foreach (DownloadItem item in plan.Items)
            {
                // ${arch} 这类模板绝不能泄漏到最终路径里——泄漏了就是下载到一个不存在的地址。
                Assert.IsFalse(
                    item.RelativePath.Contains("${"),
                    id + "：路径里残留了未替换的占位符 —— " + item.RelativePath);
                Assert.IsFalse(item.Url.Contains("${"), id + "：URL 里残留了未替换的占位符 —— " + item.Url);
            }
        }
    }

    [TestMethod]
    public void PlanBuilding_IsDeterministic()
    {
        VersionDetail version = LoadVersion("26.3");
        AssetIndex index = VersionMetadataParser.ParseAssetIndex(ReadGolden("assets-1.16.json"), "1.16");
        DownloadPlanBuilder builder = new DownloadPlanBuilder();

        DownloadPlan first = builder.Build(version, WindowsX64, index);
        DownloadPlan second = builder.Build(version, WindowsX64, index);

        CollectionAssert.AreEqual(
            first.Items.Select(i => i.RelativePath).ToList(),
            second.Items.Select(i => i.RelativePath).ToList(),
            "同一输入两次产出的计划必须逐项一致");
    }

    // ---------- natives 的三种真实形态 ----------

    [TestMethod]
    public void Version1_7_10_SubstitutesTheArchTemplateInNativeClassifiers()
    {
        VersionDetail version = LoadVersion("1.7.10");
        DownloadPlanBuilder builder = new DownloadPlanBuilder();

        List<DownloadItem> onX64 = builder.Build(version, WindowsX64).Items
            .Where(i => i.Kind == DownloadItemKind.LibraryClassifier)
            .ToList();

        Assert.AreEqual(4, onX64.Count, "1.7.10 有 4 个 natives 库，Windows 上都应产出 classifier 项");
        foreach (DownloadItem item in onX64)
        {
            StringAssert.Contains(item.RelativePath, "natives-windows");
        }

        // twitch-platform 的 natives[windows] 是 "natives-windows-${arch}"：64 位环境必须落到 -64。
        DownloadItem twitchX64 = onX64.Single(i => i.RelativePath.Contains("twitch-platform"));
        StringAssert.Contains(twitchX64.RelativePath, "natives-windows-64");

        List<DownloadItem> onX86 = builder.Build(version, WindowsX86).Items
            .Where(i => i.Kind == DownloadItemKind.LibraryClassifier)
            .ToList();

        DownloadItem twitchX86 = onX86.Single(i => i.RelativePath.Contains("twitch-platform"));
        StringAssert.Contains(
            twitchX86.RelativePath,
            "natives-windows-32",
            "32 位环境必须落到 -32：${arch} 的取值是 32/64，不是架构名");
    }

    [TestMethod]
    public void Version1_7_10_SkipsDanglingNativeReferencesInsteadOfFailing()
    {
        VersionDetail version = LoadVersion("1.7.10");

        // twitch-platform 声明了 natives[linux] = natives-linux，但它的 classifiers 里没有这一项。
        // 正确行为是跳过该库，而不是抛异常或下载一个不存在的文件。
        DownloadPlan plan = new DownloadPlanBuilder().Build(version, LinuxX64);

        Assert.IsFalse(
            plan.Items.Any(i => i.RelativePath.Contains("twitch-platform")),
            "twitch-platform 声明的 Linux 变体不存在（悬空引用），该库应被整体跳过");

        // 跳过必须是针对这一个悬空引用，而不是"Linux 上一律不要 natives"。
        Assert.IsTrue(
            plan.Items.Any(i => i.RelativePath.Contains("lwjgl-platform") && i.RelativePath.Contains("natives-linux")),
            "合法的 Linux natives 变体必须照常进入计划");
    }

    [TestMethod]
    public void Version1_16_5_OnlyEmitsClassifiersForTheCurrentPlatform()
    {
        DownloadPlan plan = new DownloadPlanBuilder().Build(LoadVersion("1.16.5"), WindowsX64);

        List<DownloadItem> classifiers = plan.Items
            .Where(i => i.Kind == DownloadItemKind.LibraryClassifier)
            .ToList();

        Assert.IsTrue(classifiers.Count > 0, "1.16.5 在 Windows 上应有本地库变体");

        foreach (DownloadItem item in classifiers)
        {
            StringAssert.Contains(item.RelativePath, "natives-windows");

            // osx / linux 专属的库被规则挡在外面，绝不能出现。
            Assert.IsFalse(item.RelativePath.Contains("natives-macos"), "macOS 变体不该出现在 Windows 计划里");
            Assert.IsFalse(item.RelativePath.Contains("natives-linux"), "Linux 变体不该出现在 Windows 计划里");
        }
    }

    [TestMethod]
    public void Version26_3_HasNoNativesMappingButKeepsClassifierLibraries()
    {
        VersionDetail version = LoadVersion("26.3");
        DownloadPlan plan = new DownloadPlanBuilder().Build(version, WindowsX64);

        Assert.AreEqual(
            0,
            plan.Items.Count(i => i.Kind == DownloadItemKind.LibraryClassifier),
            "26.x 没有 natives 映射，不该产出 classifier 通道的项");

        Assert.IsTrue(
            plan.Items.Any(i => i.Kind == DownloadItemKind.Library && i.RelativePath.Contains("natives-")),
            "26.x 的 natives 是坐标自带 classifier 的独立库条目，必须走普通库通道被下到");
    }

    [TestMethod]
    public void RuleFiltering_ChangesTheLibrarySetPerPlatform()
    {
        VersionDetail version = LoadVersion("1.16.5");
        DownloadPlanBuilder builder = new DownloadPlanBuilder();

        // 1.16.5 的元数据设计成"每个平台恰好命中一个变体"，因此三平台的 classifier **数量相同**，
        // 差别在变体本身。断言数量不同是错的——要断言的是"集合互不相交且各带自己的平台标记"。
        EnvironmentProfile osx = new EnvironmentProfile(EnvironmentProfile.OsOsx, EnvironmentProfile.ArchArm64);

        List<string> windows = ClassifierPaths(builder, version, WindowsX64);
        List<string> linux = ClassifierPaths(builder, version, LinuxX64);
        List<string> mac = ClassifierPaths(builder, version, osx);

        Assert.IsTrue(windows.Count > 0 && linux.Count > 0 && mac.Count > 0);
        Assert.IsTrue(windows.All(x => x.Contains("natives-windows")), "Windows 计划里混入了别的平台变体");
        Assert.IsTrue(linux.All(x => x.Contains("natives-linux")), "Linux 计划里混入了别的平台变体");
        // macOS 上存在两种 classifier 命名约定：LWJGL 用 natives-macos，java-objc-bridge 用 natives-osx。
        // 所以不能断言某一种具体命名——这正是 NativeClassifierSelector 绝不硬编码
        // "平台 → classifier" 映射、必须始终走库自己的 natives 表的原因。
        Assert.IsTrue(
            mac.All(x => x.Contains("natives-macos") || x.Contains("natives-osx")),
            "macOS 计划里混入了别的平台变体");
        Assert.IsTrue(mac.Any(x => x.Contains("natives-osx")), "应包含使用 natives-osx 命名约定的那个库");
        Assert.IsTrue(mac.Any(x => x.Contains("natives-macos")), "应包含使用 natives-macos 命名约定的库");

        Assert.AreEqual(0, windows.Intersect(linux, StringComparer.Ordinal).Count(), "同一变体不得同时属于两个平台");
        Assert.AreEqual(0, windows.Intersect(mac, StringComparer.Ordinal).Count());
        Assert.AreEqual(0, linux.Intersect(mac, StringComparer.Ordinal).Count());
    }

    // ---------- 资源索引 ----------

    [TestMethod]
    public void AssetIndex_MatchesTheTotalSizeDeclaredInVersionMetadata()
    {
        // 索引内体积求和必须与版本元数据声明的 totalSize 完全一致——这是两份元数据之间的交叉校验。
        foreach (var pair in new[]
                 {
                     (Id: "1.7.10", Objects: 686, Total: 112396854L),
                     (Id: "1.16.5", Objects: 2615, Total: 334438503L),
                 })
        {
            VersionDetail version = LoadVersion(pair.Id);
            AssetIndex index = VersionMetadataParser.ParseAssetIndex(
                ReadGolden("assets-" + version.AssetIndex!.Id + ".json"), version.AssetIndex!.Id);

            Assert.AreEqual(pair.Objects, index.Count, pair.Id + "：对象数量");
            Assert.AreEqual(pair.Total, index.TotalSize, pair.Id + "：索引内体积之和");
            Assert.AreEqual(
                version.AssetIndex!.TotalSize,
                index.TotalSize,
                pair.Id + "：版本元数据声明的 totalSize 与实际索引不一致");
        }
    }

    [TestMethod]
    public void AssetObjects_BecomeOneItemEachWithHashedPaths()
    {
        VersionDetail version = LoadVersion("1.7.10");
        AssetIndex index = VersionMetadataParser.ParseAssetIndex(ReadGolden("assets-1.7.10.json"), "1.7.10");

        DownloadPlan plan = new DownloadPlanBuilder().Build(version, WindowsX64, index);

        List<DownloadItem> objects = plan.Items.Where(i => i.Kind == DownloadItemKind.AssetObject).ToList();

        Assert.AreEqual(686, index.Count);

        // 资源对象内容寻址：不同逻辑名可能指向同一份内容，必须折叠成一个文件。
        int uniqueHashes = index.Objects.Values.Select(o => o.Hash).Distinct(StringComparer.Ordinal).Count();
        Assert.AreEqual(674, uniqueHashes, "1.7.10 的 686 个逻辑名只对应 674 份不同内容");
        Assert.AreEqual(uniqueHashes, objects.Count, "同哈希的逻辑名必须折叠成一个下载项");
        Assert.AreEqual(686 - uniqueHashes, plan.DuplicatePaths.Count, "被折叠的重复项必须被如实记录");

        foreach (DownloadItem item in objects)
        {
            string hash = item.Sha1!;
            Assert.AreEqual("objects/" + hash.Substring(0, 2) + "/" + hash, item.RelativePath);
            Assert.AreEqual(
                "https://resources.download.minecraft.net/" + hash.Substring(0, 2) + "/" + hash,
                item.Url);
        }

        long expectedBytes = index.Objects.Values
            .GroupBy(o => o.Hash, StringComparer.Ordinal)
            .Sum(g => g.First().Size);
        Assert.AreEqual(expectedBytes, objects.Sum(i => i.Size ?? 0), "折叠后的体积之和应等于各唯一哈希之和");
    }

    [TestMethod]
    public void AssetIndexParser_RejectsMalformedAndUnusableInput()
    {
        Assert.ThrowsException<LauncherException>(() => VersionMetadataParser.ParseAssetIndex("{ not json", "x"));
        Assert.ThrowsException<LauncherException>(() => VersionMetadataParser.ParseAssetIndex("[]", "x"));
        Assert.ThrowsException<LauncherException>(() => VersionMetadataParser.ParseAssetIndex("{}", "x"));
        Assert.ThrowsException<LauncherException>(
            () => VersionMetadataParser.ParseAssetIndex("{\"objects\":{}}", "x"));
    }

    [TestMethod]
    public void AssetIndexParser_SkipsObjectsWithoutHashInsteadOfFailingTheWholeIndex()
    {
        AssetIndex index = VersionMetadataParser.ParseAssetIndex(
            "{\"objects\":{" +
            "\"good.png\":{\"hash\":\"bdf48ef6b5d0d23bbb02e17d04865216179f510a\",\"size\":3665}," +
            "\"no-hash.png\":{\"size\":1}," +
            "\"bad-shape\":\"not-an-object\"}}",
            "test");

        Assert.AreEqual(1, index.Count);
        Assert.AreEqual("good.png", index.Objects["good.png"].Name);
        Assert.AreEqual(3665, index.Objects["good.png"].Size);
    }

    // ---------- 路径约定 ----------

    [TestMethod]
    public void CachePathConventions_UsesForwardSlashesAndOfficialLayout()
    {
        CachePathConventions paths = new CachePathConventions();

        Assert.AreEqual("meta/version-1.16.5.json", paths.VersionDetailFile("1.16.5"));
        Assert.AreEqual("meta/client-1.16.5.jar", paths.ClientJarFile("1.16.5"));
        Assert.AreEqual("meta/assets-1.16.json", paths.AssetIndexFile("1.16"));
        Assert.AreEqual(
            "objects/ab/abcdef0123456789abcdef0123456789abcdef",
            paths.ObjectFile("abcdef0123456789abcdef0123456789abcdef"));

        LibraryName.TryParse("org.lwjgl:lwjgl:3.2.1", out LibraryName? plain);
        Assert.AreEqual(
            "libraries/org/lwjgl/lwjgl/3.2.1/lwjgl-3.2.1.jar",
            paths.LibraryFile(null, plain, "org.lwjgl:lwjgl:3.2.1"));

        LibraryName.TryParse("org.lwjgl:lwjgl:3.2.1:natives-windows", out LibraryName? classified);
        Assert.AreEqual(
            "libraries/org/lwjgl/lwjgl/3.2.1/lwjgl-3.2.1-natives-windows.jar",
            paths.LibraryFile(null, classified, "org.lwjgl:lwjgl:3.2.1:natives-windows"));

        // 元数据给了权威路径时必须用它，而不是自己推导。
        Assert.AreEqual(
            "libraries/com/mojang/netty/1.8.8/netty-1.8.8.jar",
            paths.LibraryFile(@"com\mojang\netty\1.8.8\netty-1.8.8.jar", plain, "x"));
    }

    [TestMethod]
    public void NativeClassifierSelector_HandlesAllThreeRealShapes()
    {
        // 形态一：普通映射
        LibraryRef plain = new LibraryRef
        {
            RawName = "org.lwjgl.lwjgl:lwjgl-platform:2.9.1",
            Natives = new Dictionary<string, string>(StringComparer.Ordinal) { { "windows", "natives-windows" } },
            Classifiers = new Dictionary<string, DownloadRef>(StringComparer.Ordinal)
            {
                { "natives-windows", new DownloadRef("https://example.invalid/a.jar") },
            },
        };

        Assert.AreEqual("natives-windows", NativeClassifierSelector.Select(plain, WindowsX64));

        // 形态二：模板值
        LibraryRef templated = new LibraryRef
        {
            RawName = "tv.twitch:twitch-platform:5.16",
            Natives = new Dictionary<string, string>(StringComparer.Ordinal) { { "windows", "natives-windows-${arch}" } },
            Classifiers = new Dictionary<string, DownloadRef>(StringComparer.Ordinal)
            {
                { "natives-windows-32", new DownloadRef("https://example.invalid/32.jar") },
                { "natives-windows-64", new DownloadRef("https://example.invalid/64.jar") },
            },
        };

        Assert.AreEqual("natives-windows-64", NativeClassifierSelector.Select(templated, WindowsX64));
        Assert.AreEqual("natives-windows-32", NativeClassifierSelector.Select(templated, WindowsX86));

        // 形态三：悬空引用
        LibraryRef dangling = new LibraryRef
        {
            RawName = "tv.twitch:twitch-platform:5.16",
            Natives = new Dictionary<string, string>(StringComparer.Ordinal) { { "linux", "natives-linux" } },
            Classifiers = new Dictionary<string, DownloadRef>(StringComparer.Ordinal)
            {
                { "natives-windows-64", new DownloadRef("https://example.invalid/64.jar") },
            },
        };

        Assert.IsNull(NativeClassifierSelector.Select(dangling, LinuxX64), "悬空引用必须返回 null，而不是抛异常");

        // 没有 natives 映射的库（现代版本）不参与这条通道。
        Assert.IsNull(NativeClassifierSelector.Select(new LibraryRef { RawName = "a:a:1.0" }, WindowsX64));
        Assert.AreEqual("64", NativeClassifierSelector.ArchToken(EnvironmentProfile.ArchArm64));
        Assert.AreEqual("32", NativeClassifierSelector.ArchToken(EnvironmentProfile.ArchX86));
    }

    // ---------- 环境探测 ----------

    [TestMethod]
    public void PlatformProbe_ReturnsAUsableProfile()
    {
        EnvironmentProfile profile = PlatformProbe.Current();

        CollectionAssert.Contains(
            new[] { EnvironmentProfile.OsWindows, EnvironmentProfile.OsLinux, EnvironmentProfile.OsOsx },
            profile.OsName);

        CollectionAssert.Contains(
            new[] { EnvironmentProfile.ArchX86, EnvironmentProfile.ArchX86_64, EnvironmentProfile.ArchArm64 },
            profile.OsArch);

        Assert.IsFalse(string.IsNullOrEmpty(profile.OsVersion));
        Assert.AreEqual(0, profile.Features.Count, "默认不应带任何特性开关");
    }
}
