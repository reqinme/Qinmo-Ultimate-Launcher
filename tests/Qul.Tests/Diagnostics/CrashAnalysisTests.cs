using System;
using System.IO;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Diagnostics;

namespace Qul.Tests.Diagnostics;

/// <summary>
/// 崩溃分析：从游戏自己写的崩溃报告里读出**能行动**的结论。
///
/// 这些用例的价值不在于"覆盖了多少分支"，而在于钉住三件容易做错的事：
/// <list type="number">
/// <item>具体原因不能被笼统规则吃掉（规则顺序）；</item>
/// <item>认不出来时必须说认不出来，不能编一个猜测；</item>
/// <item>不能把上一次的旧报告当成本次的原因。</item>
/// </list>
/// </summary>
[TestClass]
public class CrashAnalysisTests
{
    private static readonly UTF8Encoding Utf8 = new UTF8Encoding(false);

    // ---------- 具体原因 ----------

    [TestMethod]
    public void Analyze_RecognisesAJavaVersionMismatch()
    {
        string report =
            "---- Minecraft Crash Report ----\n"
            + "// Oh dear\n\n"
            + "java.lang.UnsupportedClassVersionError: net/minecraft/client/main/Main has been compiled by a more recent version of the Java Runtime\n"
            + "\tat java.lang.ClassLoader.defineClass1(Native Method)\n";

        CrashFinding? finding = CrashAnalysis.Analyze(report, 1);

        Assert.IsNotNull(finding);
        StringAssert.Contains(finding!.Summary, "Java");
        StringAssert.Contains(finding.Evidence, "UnsupportedClassVersionError");
        Assert.IsTrue(finding.Suggestions.Count >= 2, "结论必须带可操作的下一步");
    }

    [TestMethod]
    public void Analyze_RecognisesMissingNatives()
    {
        string report =
            "---- Minecraft Crash Report ----\n"
            + "java.lang.UnsatisfiedLinkError: no lwjgl in java.library.path\n";

        CrashFinding? finding = CrashAnalysis.Analyze(report, 1);

        Assert.IsNotNull(finding);
        StringAssert.Contains(finding!.Summary, "本地库");
        StringAssert.Contains(finding.Summary, "natives");
    }

    [TestMethod]
    public void Analyze_RecognisesOutOfMemory()
    {
        CrashFinding? finding = CrashAnalysis.Analyze(
            "---- Minecraft Crash Report ----\njava.lang.OutOfMemoryError: Java heap space\n", 1);

        Assert.IsNotNull(finding);
        StringAssert.Contains(finding!.Summary, "内存");
    }

    [TestMethod]
    public void Analyze_RecognisesAGraphicsDriverProblem()
    {
        CrashFinding? finding = CrashAnalysis.Analyze(
            "---- Minecraft Crash Report ----\norg.lwjgl.LWJGLException: Pixel format not accelerated\n", 1);

        Assert.IsNotNull(finding);
        StringAssert.Contains(finding!.Summary, "显卡");
    }

    [TestMethod]
    public void Analyze_RecognisesAModProblem()
    {
        CrashFinding? finding = CrashAnalysis.Analyze(
            "---- Minecraft Crash Report ----\nnet.minecraftforge.fml.common.ModLoadingException: bad mod\n", 1);

        Assert.IsNotNull(finding);
        StringAssert.Contains(finding!.Summary, "模组");
    }

    // ---------- 容易做错的三件事 ----------

    [TestMethod]
    public void Analyze_PicksTheMoreActionableCauseWhenSeveralMarkersArePresent()
    {
        // **规则顺序是有意义的。**
        //
        // 真实报告里常常同时出现多个特征：一个装了一堆模组的实例内存爆了，
        // 报告里既有 `OutOfMemoryError`，也有满屏的 `net.minecraftforge` 痕迹。
        //
        // 这时该给玩家哪一条？**内存那条**——"把最大内存调大"是他五秒钟能试的事；
        // "先只留一个模组试"要花半小时。**先给能立刻动手的。**
        //
        // （这条用例不是装饰：把分析器里两条规则的顺序对调，它就会红。）
        string report =
            "---- Minecraft Crash Report ----\n"
            + "// Oh dear\n"
            + "Description: Ticking memory connection\n\n"
            + "java.lang.OutOfMemoryError: Java heap space\n"
            + "\tat net.minecraftforge.fml.common.ModContainer.<init>(ModContainer.java:1)\n";

        CrashFinding? finding = CrashAnalysis.Analyze(report, 1);

        Assert.IsNotNull(finding);
        StringAssert.Contains(
            finding!.Summary, "内存",
            "同一份报告里既有内存不足又有模组痕迹时，应当先给玩家能立刻动手的那条");
    }

    [TestMethod]
    public void Analyze_AdmitsWhenItDoesNotRecogniseTheCause()
    {
        // **认不出来就说认不出来。** 编一个像模像样的猜测，
        // 会让玩家照着错误方向折腾——那比"不知道"更糟。
        string report =
            "---- Minecraft Crash Report ----\n"
            + "// Whoops\n"
            + "Description: Something entirely new\n\n"
            + "some.unknown.Exception: nothing we have seen before\n";

        CrashFinding? finding = CrashAnalysis.Analyze(report, 1);

        Assert.IsNotNull(finding);
        StringAssert.Contains(finding!.Summary, "没有我们认得的原因");
        StringAssert.Contains(finding.Evidence, "Something entirely new");
        Assert.IsTrue(finding.Suggestions.Count >= 1, "即使认不出来也要给出下一步");
    }

    [TestMethod]
    public void Analyze_WithNoReportAtAllStillSaysSomethingUseful()
    {
        CrashFinding? finding = CrashAnalysis.Analyze(null, 1);

        Assert.IsNotNull(finding);
        StringAssert.Contains(finding!.Summary, "没有留下崩溃报告");
        StringAssert.Contains(finding.Evidence, "exitCode=1");
    }

    [TestMethod]
    public void Analyze_ReturnsNothingWhenTheGameExitedNormally()
    {
        // 正常退出不该被报成崩溃。
        Assert.IsNull(CrashAnalysis.Analyze(null, 0));
        Assert.IsNull(CrashAnalysis.Analyze(string.Empty, 0));
    }

    [TestMethod]
    public void Analyze_DoesNotScanBeyondTheHeadOfAHugeReport()
    {
        // 崩溃报告可以很大，而结论在开头。**只看开头**既够用又不会把整个文件读进内存。
        // 把特征串放在超出扫描上限的位置，应当认不出来（这正是"只看开头"的证据）。
        string filler = new string('x', CrashAnalysis.MaxScanLength + 1000);
        string report = "---- Minecraft Crash Report ----\n" + filler
            + "\njava.lang.OutOfMemoryError: Java heap space\n";

        CrashFinding? finding = CrashAnalysis.Analyze(report, 1);

        Assert.IsNotNull(finding);
        Assert.IsFalse(
            finding!.Summary.Contains("内存"),
            "超出扫描上限的内容不应影响结论——否则说明扫描范围失控了");
    }

    // ---------- 定位：不能把旧报告当成本次的原因 ----------

    [TestMethod]
    public void Locator_IgnoresReportsWrittenBeforeThisRun()
    {
        string sandbox = NewSandbox();
        string dir = Path.Combine(sandbox, CrashReportLocator.ReportsDirectoryName);
        Directory.CreateDirectory(dir);

        try
        {
            string stale = Path.Combine(dir, "crash-old.txt");
            string fresh = Path.Combine(dir, "crash-new.txt");

            File.WriteAllText(stale, "old", Utf8);
            File.WriteAllText(fresh, "new", Utf8);

            DateTime now = DateTime.UtcNow;
            File.SetLastWriteTimeUtc(stale, now.AddHours(-3));
            File.SetLastWriteTimeUtc(fresh, now);

            // 本次启动发生在"刚才"——旧报告必须被排除
            string? found = CrashReportLocator.FindNewest(sandbox, new DateTimeOffset(now.AddMinutes(-1), TimeSpan.Zero));

            Assert.IsNotNull(found);
            Assert.AreEqual("crash-new.txt", Path.GetFileName(found));
        }
        finally
        {
            TryDelete(sandbox);
        }
    }

    [TestMethod]
    public void Locator_ReturnsNullWhenThereIsNoReportsDirectory()
    {
        string sandbox = NewSandbox();

        try
        {
            Assert.IsNull(CrashReportLocator.FindNewest(sandbox, DateTimeOffset.UtcNow));
            Assert.IsNull(CrashReportLocator.FindNewest(null, DateTimeOffset.UtcNow));
        }
        finally
        {
            TryDelete(sandbox);
        }
    }

    [TestMethod]
    public void Locator_ReadsTheReportBodyWithoutThrowingOnBadInput()
    {
        Assert.IsNull(CrashReportLocator.Read(null));
        Assert.IsNull(CrashReportLocator.Read(string.Empty));

        string sandbox = NewSandbox();
        try
        {
            string path = Path.Combine(sandbox, "crash.txt");
            File.WriteAllText(path, "hello crash", Utf8);

            Assert.AreEqual("hello crash", CrashReportLocator.Read(path));
            Assert.IsNull(CrashReportLocator.Read(Path.Combine(sandbox, "no-such-file.txt")));
        }
        finally
        {
            TryDelete(sandbox);
        }
    }

    // ---------- 辅助 ----------

    private static string NewSandbox()
    {
        string path = Path.Combine(Path.GetTempPath(), "qul-crash-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(path);
        return path;
    }

    private static void TryDelete(string path)
    {
        try
        {
            if (Directory.Exists(path))
            {
                Directory.Delete(path, recursive: true);
            }
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }
}
