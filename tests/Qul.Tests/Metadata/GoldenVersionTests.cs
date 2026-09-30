using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Diagnostics;
using Qul.Domain.Metadata;
using Qul.Infrastructure.Metadata;

namespace Qul.Tests.Metadata;

/// <summary>
/// 黄金样本回归。
/// 样本是四个代表性版本的**真实官方元数据**（1.7.10 / 1.12.2 / 1.16.5 / 当前正式版），
/// 冻结在 Golden 目录里，因此测试离线可跑、不受网络与元数据漂移影响。
/// 这些断言锁的是跨年代的结构差异——解析器一旦"顺手简化"，这里会立刻红。
/// </summary>
[TestClass]
public sealed class GoldenVersionTests
{
    private static readonly EnvironmentProfile WindowsOs =
        new EnvironmentProfile(EnvironmentProfile.OsWindows, EnvironmentProfile.ArchX86_64, "10.0.19045");

    private static string ReadGolden(string fileName)
    {
        string path = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "Golden", fileName);
        Assert.IsTrue(File.Exists(path), "缺少黄金样本：" + path);
        return File.ReadAllText(path, Encoding.UTF8);
    }

    private static VersionDetail Load(string id)
    {
        return VersionMetadataParser.ParseVersion(ReadGolden("version-" + id + ".json"), id);
    }

    [TestMethod]
    public void Manifest_ParsesTheOfficialVersionIndex()
    {
        VersionManifest manifest = VersionMetadataParser.ParseManifest(ReadGolden("version_manifest_v2.json"));

        Assert.IsTrue(manifest.Versions.Count > 500, "清单版本数异常偏少：" + manifest.Versions.Count);
        Assert.IsFalse(string.IsNullOrEmpty(manifest.LatestRelease), "latest.release 不得为空");
        Assert.IsFalse(string.IsNullOrEmpty(manifest.LatestSnapshot), "latest.snapshot 不得为空");

        foreach (string id in new[] { "1.7.10", "1.12.2", "1.16.5" })
        {
            VersionSummary? summary = manifest.Find(id);
            Assert.IsNotNull(summary, "清单里找不到 " + id);
            Assert.AreEqual(VersionType.Release, summary!.Type, id + " 应为正式版");
            StringAssert.StartsWith(summary.Url, "https://", id + " 的详情地址应为 https");
            Assert.AreEqual(40, summary.Sha1?.Length ?? 0, id + " 的 sha1 应为 40 位十六进制");

            // 注意：本版清单 917 个条目一个都没有 size 字段——它是可选的，解析器必须容忍缺失，不得据此报错。
        }

        Assert.IsNull(manifest.Find("no-such-version-9.9.9"), "不存在的版本必须返回 null 而不是抛异常");
    }

    [TestMethod]
    public void Version1_7_10_UsesLegacyArgumentsAndClassifierOnlyNatives()
    {
        VersionDetail version = Load("1.7.10");

        Assert.AreEqual("net.minecraft.client.main.Main", version.MainClass);
        Assert.AreEqual("1.7.10", version.Assets);
        Assert.AreEqual("1.7.10", version.AssetIndex?.Id);
        Assert.AreEqual(8, version.JavaVersion?.MajorVersion);
        Assert.AreEqual(33, version.Libraries.Count);
        Assert.IsNull(version.InheritsFrom);

        // 旧式参数写法：一个整串，没有 arguments 对象。
        Assert.IsTrue(version.UsesLegacyArguments, "1.7.10 必须走 minecraftArguments");
        Assert.AreEqual(0, version.GameArguments.Count);
        StringAssert.StartsWith(version.MinecraftArguments!, "--username ${auth_player_name}");

        // 旧式 natives：只有 classifier 内容、没有主 artifact 的库有 4 个。
        List<LibraryRef> nativeHolders = version.Libraries.Where(l => l.IsLegacyNativeHolder).ToList();
        Assert.AreEqual(4, nativeHolders.Count, "1.7.10 的 natives 库数量");

        // natives 映射值的完整不变式：字面可命中，或属于两类已登记的例外。
        // 例外一：模板值——分类器键本身带占位符，例如 "natives-windows-${arch}"，
        //         必须按 JVM 架构替换成 natives-windows-32 / natives-windows-64 才能命中。
        // 例外二：悬空引用——官方元数据自身就声明了一个 classifiers 里不存在的变体。
        // 两类例外都有计数断言，确保样本真的覆盖到了它们，而不是靠"恰好没触发"通过。
        bool sawTemplate = false;
        int danglingReferences = 0;

        foreach (LibraryRef library in nativeHolders)
        {
            Assert.IsNull(library.Artifact, library.RawName + "：该代 natives 库只有 classifier 内容");
            Assert.IsTrue(library.ExtractExclude.Count > 0, library.RawName + " 应带解压排除项");
            Assert.IsTrue(library.Natives.ContainsKey("windows"), library.RawName + " 应有 windows natives 映射");

            foreach (KeyValuePair<string, string> mapping in library.Natives)
            {
                string value = mapping.Value;

                if (value.IndexOf("${", StringComparison.Ordinal) >= 0)
                {
                    sawTemplate = true;
                    Assert.IsFalse(
                        library.Classifiers.ContainsKey(value),
                        library.RawName + "：模板值不该是字面键，必须先做占位符替换");
                    continue;
                }

                if (!library.Classifiers.ContainsKey(value))
                {
                    danglingReferences++;
                    Assert.AreEqual(
                        "natives-linux",
                        value,
                        library.RawName + "：出现了未经登记的悬空引用 " + value);
                    continue;
                }
            }
        }

        Assert.IsTrue(sawTemplate, "1.7 时代存在带 ${arch} 占位符的 natives 分类器键，样本必须覆盖这一形态");
        Assert.IsTrue(danglingReferences > 0, "官方元数据里存在悬空 natives 引用，样本必须覆盖这一形态");

        // 具体锁定 twitch-platform：它的 linux 映射悬空、windows 映射是 ${arch} 模板，
        // 且 classifiers 里没有笼统的 natives-windows，只有 32/64 两个分支。
        // P4 解析 natives 时：变体不存在就跳过该库，遇到模板先替换，两者都不能抛异常。
        LibraryRef twitch = nativeHolders.Single(l => l.RawName == "tv.twitch:twitch-platform:5.16");
        Assert.AreEqual("natives-linux", twitch.Natives["linux"], "悬空引用：声明了但 classifiers 里不存在");
        Assert.IsFalse(twitch.Classifiers.ContainsKey("natives-linux"));
        Assert.AreEqual("natives-windows-${arch}", twitch.Natives["windows"], "分类器键本身带 ${arch} 模板");
        CollectionAssert.AreEquivalent(
            new[] { "natives-osx", "natives-windows-32", "natives-windows-64" },
            twitch.Classifiers.Keys.ToList());

        LibraryRef external = nativeHolders.Single(l => l.RawName == "tv.twitch:twitch-external-platform:4.5");
        Assert.AreEqual("natives-windows-${arch}", external.Natives["windows"]);
        CollectionAssert.AreEquivalent(
            new[] { "natives-windows-32", "natives-windows-64" },
            external.Classifiers.Keys.ToList());
    }

    [TestMethod]
    public void Version1_12_2_HasDuplicateCoordinatesDistinguishedOnlyByNatives()
    {
        VersionDetail version = Load("1.12.2");

        Assert.IsTrue(version.UsesLegacyArguments);
        Assert.AreEqual(39, version.Libraries.Count);

        // 实测：39 条库只对应 37 个唯一坐标——同坐标多变体是这一代的常态。
        int distinctCoordinates = version.Libraries.Select(l => l.RawName).Distinct(StringComparer.Ordinal).Count();
        Assert.AreEqual(37, distinctCoordinates, "同坐标多变体的事实被改变了，去重逻辑必须重新评估");

        List<LibraryRef> textToSpeech = version.Libraries
            .Where(l => l.RawName == "com.mojang:text2speech:1.10.3")
            .ToList();

        Assert.AreEqual(2, textToSpeech.Count, "同一坐标的两个变体都必须保留");
        Assert.AreEqual(1, textToSpeech.Count(l => l.IsLegacyNativeHolder), "其中一个带 natives，另一个不带");
    }

    [TestMethod]
    public void Version1_16_5_UsesNewArgumentsAndCarriesRuleGuardedEntries()
    {
        VersionDetail version = Load("1.16.5");

        Assert.AreEqual(57, version.Libraries.Count);
        Assert.IsFalse(version.UsesLegacyArguments, "1.16.5 必须走 arguments 对象");
        Assert.IsNull(version.MinecraftArguments);
        Assert.IsTrue(version.GameArguments.Count > 0);
        Assert.IsTrue(version.JvmArguments.Count > 0);

        // 实测：57 条库只对应 41 个唯一坐标，16 组同坐标变体。
        int distinctCoordinates = version.Libraries.Select(l => l.RawName).Distinct(StringComparer.Ordinal).Count();
        Assert.AreEqual(41, distinctCoordinates);
        Assert.AreEqual(16, version.Libraries.Count(l => l.IsLegacyNativeHolder));

        // 该代 natives 条目同时带主 artifact 与 classifier 内容。
        foreach (LibraryRef library in version.Libraries.Where(l => l.IsLegacyNativeHolder))
        {
            Assert.IsNotNull(library.Artifact, library.RawName + "：该代 natives 条目应同时带主 artifact");
            Assert.IsTrue(library.Classifiers.Count > 0);
        }

        // 规则约束的 JVM 参数：-XstartOnFirstThread 只在 osx 上生效。
        List<ArgumentEntry> startOnFirstThread = version.JvmArguments
            .Where(a => a.Values.Contains("-XstartOnFirstThread"))
            .ToList();

        Assert.AreEqual(1, startOnFirstThread.Count);
        Assert.IsTrue(startOnFirstThread[0].HasRules, "该参数必须带规则");
        Assert.IsFalse(
            RuleEvaluator.IsAllowed(startOnFirstThread[0].Rules, WindowsOs),
            "osx 专属参数绝不能在 Windows 上生效");
        Assert.IsTrue(
            RuleEvaluator.IsAllowed(startOnFirstThread[0].Rules, new EnvironmentProfile(EnvironmentProfile.OsOsx, EnvironmentProfile.ArchX86_64)));

        // 特性开关参数：--demo 只在 is_demo_user 为真时出现。
        Assert.IsTrue(
            version.GameArguments.Any(a => a.Values.Contains("--demo") && a.HasRules),
            "--demo 必须被解析为带规则的参数项");
    }

    [TestMethod]
    public void Version26_3_UsesClassifierNativesAndHasDefaultUserJvmGroup()
    {
        VersionDetail version = Load("26.3");

        Assert.AreEqual(114, version.Libraries.Count);
        Assert.AreEqual(25, version.JavaVersion?.MajorVersion, "26.x 要求 Java 25");
        Assert.AreEqual("34", version.Assets);
        Assert.IsFalse(version.UsesLegacyArguments);

        // 现代版本根本不用 natives 映射：natives 是坐标里带 classifier 的独立库条目。
        Assert.AreEqual(0, version.Libraries.Count(l => l.IsLegacyNativeHolder), "26.x 不应再出现 natives 映射");
        Assert.IsTrue(
            version.Libraries.Count(l => l.HasClassifierInName) > 0,
            "26.x 的 natives 靠坐标 classifier 表达，解析器必须保留 classifier");

        // 26.x 新增的参数组：P1 不决定怎么用，但必须解析出来。
        Assert.IsTrue(version.DefaultUserJvmArguments.Count > 0, "default-user-jvm 组必须被解析");
    }

    [TestMethod]
    public void RuleFiltering_YieldsDifferentLibrarySetsPerPlatform()
    {
        EnvironmentProfile linux = new EnvironmentProfile(EnvironmentProfile.OsLinux, EnvironmentProfile.ArchX86_64);
        EnvironmentProfile osx = new EnvironmentProfile(EnvironmentProfile.OsOsx, EnvironmentProfile.ArchArm64);

        foreach (string id in new[] { "1.7.10", "1.12.2", "1.16.5" })
        {
            VersionDetail version = Load(id);

            int windowsCount = version.Libraries.Count(l => RuleEvaluator.IsAllowed(l.Rules, WindowsOs));
            int linuxCount = version.Libraries.Count(l => RuleEvaluator.IsAllowed(l.Rules, linux));
            int osxCount = version.Libraries.Count(l => RuleEvaluator.IsAllowed(l.Rules, osx));

            Assert.IsTrue(windowsCount > 0 && linuxCount > 0 && osxCount > 0, id + "：三个平台都应至少有可用库");
            Assert.IsFalse(
                windowsCount == linuxCount && windowsCount == osxCount,
                id + "：规则没有生效——三个平台得到完全相同的库集合");
        }
    }

    /// <summary>
    /// 这条锁的是一个真实踩过的坑：1.16.5 有 16 组库坐标完全相同、仅 natives/classifiers 不同。
    /// 若去重键只用 Maven 坐标，继承合并会静默吃掉一半本地库变体。
    /// </summary>
    [TestMethod]
    public void LibraryMerge_KeepsSameCoordinateNativeVariants()
    {
        VersionDetail version = Load("1.16.5");

        IReadOnlyList<LibraryRef> mergedUnderEmptyParent =
            VersionResolver.MergeLibraries(Array.Empty<LibraryRef>(), version.Libraries);
        Assert.AreEqual(57, mergedUnderEmptyParent.Count, "同坐标的 natives 变体不得被合并掉");

        IReadOnlyList<LibraryRef> mergedOverEmptyChild =
            VersionResolver.MergeLibraries(version.Libraries, Array.Empty<LibraryRef>());
        Assert.AreEqual(57, mergedOverEmptyChild.Count, "父传子方向同样不得合并变体");

        VersionDetail modern = Load("26.3");
        IReadOnlyList<LibraryRef> mergedModern =
            VersionResolver.MergeLibraries(Array.Empty<LibraryRef>(), modern.Libraries);
        Assert.AreEqual(114, mergedModern.Count, "靠坐标 classifier 区分的 natives 变体同样不得被合并掉");
    }

    [TestMethod]
    public void ParsingIsDeterministic()
    {
        // 启动计划骨架要做逐字节比对，前提就是解析本身可复现。
        VersionDetail first = Load("1.16.5");
        VersionDetail second = Load("1.16.5");

        Assert.AreEqual(first.Libraries.Count, second.Libraries.Count);
        CollectionAssert.AreEqual(
            first.Libraries.Select(l => l.RawName).ToList(),
            second.Libraries.Select(l => l.RawName).ToList());
        CollectionAssert.AreEqual(
            first.GameArguments.SelectMany(a => a.Values).ToList(),
            second.GameArguments.SelectMany(a => a.Values).ToList());
    }

    [TestMethod]
    public void ResolvedVersions_PassValidation()
    {
        foreach (string id in new[] { "1.7.10", "1.12.2", "1.16.5", "26.3" })
        {
            VersionDetail version = Load(id);
            LauncherException? error = VersionResolver.Validate(version);
            Assert.IsNull(error, id + " 校验未通过：" + error?.Message);
        }
    }
}
