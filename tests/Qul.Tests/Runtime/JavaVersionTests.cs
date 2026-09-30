using System;
using System.IO;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Runtime;

namespace Qul.Tests.Runtime;

/// <summary>
/// Java 版本号与探测输出的解析。
/// 样本是**真实抓取的 java.exe 输出**（见 Golden/java-*.txt），因此格式假设不是猜的。
/// </summary>
[TestClass]
public sealed class JavaVersionTests
{
    private static string ReadGolden(string fileName)
    {
        string path = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "Golden", fileName);
        Assert.IsTrue(File.Exists(path), "缺少黄金样本：" + path);
        return File.ReadAllText(path, Encoding.UTF8);
    }

    [TestMethod]
    public void JavaVersion_ParsesEveryRealVersionShapeOnThisMachine()
    {
        // 这七条全是本机实测值：三代命名一网打尽。
        AssertVersion("1.7.0_80", 7, 0, 0, 0, 80);
        AssertVersion("1.8.0_503", 8, 0, 0, 0, 503);
        AssertVersion("11.0.32", 11, 0, 32, 0, 0);
        AssertVersion("17.0.20", 17, 0, 20, 0, 0);
        AssertVersion("21.0.12.1", 21, 0, 12, 1, 0);
        AssertVersion("25.0.4.1", 25, 0, 4, 1, 0);
        AssertVersion("1.8.0", 8, 0, 0, 0, 0);
    }

    [TestMethod]
    public void JavaVersion_StripsBuildMetadataAndQuotes()
    {
        AssertVersion("1.8.0_503-b01", 8, 0, 0, 0, 503);
        AssertVersion("17.0.20+7-LTS-191", 17, 0, 20, 0, 0);
        AssertVersion("25.0.4.1+7-LTS", 25, 0, 4, 1, 0);
        AssertVersion("\"17.0.20\"", 17, 0, 20, 0, 0);
        AssertVersion("  11.0.32  ", 11, 0, 32, 0, 0);
        AssertVersion("9", 9, 0, 0, 0, 0);
    }

    [TestMethod]
    public void JavaVersion_RejectsGarbage()
    {
        foreach (string? bad in new[] { null, string.Empty, "   ", "abc", "v17", "0.0.0", "1.0", "1", "1.0_", "-1.0" })
        {
            Assert.IsFalse(
                JavaVersion.TryParse(bad, out _),
                "不该接受：" + (bad ?? "<null>"));
        }
    }

    [TestMethod]
    public void JavaVersion_IsComparableAndOrderedByMajorFirst()
    {
        JavaVersion.TryParse("1.8.0_503", out JavaVersion eight);
        JavaVersion.TryParse("11.0.32", out JavaVersion eleven);
        JavaVersion.TryParse("17.0.20", out JavaVersion seventeen);
        JavaVersion.TryParse("1.8.0_502", out JavaVersion eightOlder);

        Assert.IsTrue(eight.CompareTo(eleven) < 0);
        Assert.IsTrue(eleven.CompareTo(seventeen) < 0);
        Assert.IsTrue(eight.CompareTo(eightOlder) > 0, "同主版本下 update 号更大的是更新的");
        Assert.IsTrue(eight.Equals(JavaVersion.TryParse("1.8.0_503", out JavaVersion other) ? other : default));
    }

    // ---------- 真实输出解析 ----------

    [TestMethod]
    public void Parser_ReadsAuthoritativePropertiesOutput()
    {
        // 实测：-XshowSettings:properties -version 的输出全部走 stderr，stdout 为 0 字节。
        JavaProbeResult result = ParseProperties("java-jdk-17.0.20.properties.txt");

        Assert.AreEqual(17, result.Version.Major);
        Assert.AreEqual("x86_64", result.OsArch, "amd64 必须被归一化");
        Assert.AreEqual(64, result.Bitness);
        Assert.AreEqual("Oracle Corporation", result.Vendor);
    }

    [TestMethod]
    public void Parser_ReadsLegacyJava8AndJava7Installations()
    {
        JavaProbeResult java8 = ParseProperties("java-jre1.8.0_503.properties.txt");
        Assert.AreEqual(8, java8.Version.Major, "1.8.0_503 的主版本是 8，不是 1");
        Assert.AreEqual(503, java8.Version.Update);
        Assert.AreEqual(64, java8.Bitness);

        JavaProbeResult java7 = ParseProperties("java-jdk1.7.0_80.properties.txt");
        Assert.AreEqual(7, java7.Version.Major);
        Assert.AreEqual(80, java7.Version.Update);
        Assert.AreEqual(64, java7.Bitness);
    }

    [TestMethod]
    public void Parser_FallsBackToPlainVersionLine()
    {
        // 纯 -version 输出里没有属性键值对，只能从 java version "17.0.20" 这类行取版本。
        JavaProbeResult result = ParseProperties("java-jdk-17.0.20.version.txt");

        Assert.AreEqual(17, result.Version.Major);
        Assert.IsNull(result.OsArch, "回落路径拿不到 os.arch，此处必须如实为 null 而不是瞎猜");
        Assert.AreEqual(64, result.Bitness, "位宽可以从 vm name 里的 64-Bit 推断");
    }

    [TestMethod]
    public void Parser_HandlesBothStreamsAndGarbage()
    {
        Assert.IsTrue(JavaProbeOutputParser.TryParse(
            string.Empty,
            "openjdk version \"11.0.32\" 2026-01-01\nOpenJDK Runtime Environment (build 11.0.32+9)\n",
            out JavaProbeResult fallback));
        Assert.AreEqual(11, fallback.Version.Major);

        Assert.IsTrue(JavaProbeOutputParser.TryParse(
            "    java.version = 9.0.4\n    os.arch = x86\n    sun.arch.data.model = 32\n",
            string.Empty,
            out JavaProbeResult fromStdout));
        Assert.AreEqual(9, fromStdout.Version.Major);
        Assert.AreEqual("x86", fromStdout.OsArch);
        Assert.AreEqual(32, fromStdout.Bitness);

        Assert.IsFalse(JavaProbeOutputParser.TryParse(null, null, out _));
        Assert.IsFalse(JavaProbeOutputParser.TryParse("Error: could not open file", string.Empty, out _));
        Assert.IsFalse(JavaProbeOutputParser.TryParse("    java.version = not-a-version", string.Empty, out _));
    }

    [TestMethod]
    public void Parser_ResolvesBitnessWithoutGuessingFromArchName()
    {
        // 判定顺序：sun.arch.data.model → vm name → 未知（0）。
        Assert.AreEqual(32, JavaProbeOutputParser.ResolveBitness(32, "Java HotSpot(TM) 64-Bit Server VM"));
        Assert.AreEqual(64, JavaProbeOutputParser.ResolveBitness(64, null));
        Assert.AreEqual(32, JavaProbeOutputParser.ResolveBitness(0, "Java HotSpot(TM) Client VM 32-Bit"));
        Assert.AreEqual(0, JavaProbeOutputParser.ResolveBitness(0, "Some VM"), "拿不到就返回未知，绝不用架构名猜");
    }

    [TestMethod]
    public void RuntimeCandidate_NormalizesArchNames()
    {
        Assert.AreEqual("x86_64", JavaRuntimeCandidate.NormalizeArch("amd64"));
        Assert.AreEqual("x86_64", JavaRuntimeCandidate.NormalizeArch("AMD64"));
        Assert.AreEqual("x86", JavaRuntimeCandidate.NormalizeArch("i386"));
        Assert.AreEqual("arm64", JavaRuntimeCandidate.NormalizeArch("aarch64"));
        Assert.IsNull(JavaRuntimeCandidate.NormalizeArch(null));

        JavaRuntimeCandidate candidate = new JavaRuntimeCandidate { Bitness = 32 };
        Assert.AreEqual("32", candidate.ArchToken, "${arch} 要的是 32/64，不是架构名");
    }

    private static void AssertVersion(
        string text,
        int major,
        int minor,
        int patch,
        int build,
        int update)
    {
        Assert.IsTrue(JavaVersion.TryParse(text, out JavaVersion version), "无法解析：" + text);
        Assert.AreEqual(major, version.Major, text + " 的主版本");
        Assert.AreEqual(minor, version.Minor, text + " 的次版本");
        Assert.AreEqual(patch, version.Patch, text + " 的修订号");
        Assert.AreEqual(build, version.Build, text + " 的构建号");
        Assert.AreEqual(update, version.Update, text + " 的 update 号");
    }

    private static JavaProbeResult ParseProperties(string goldenFile)
    {
        string text = ReadGolden(goldenFile);
        Assert.IsTrue(JavaProbeOutputParser.TryParse(string.Empty, text, out JavaProbeResult result), goldenFile);
        return result;
    }
}
