using Qul.Infrastructure.Launch;
using Qul.Domain.Configuration;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Java;
using Qul.Application.Ports;
using Qul.Domain.Diagnostics;
using Qul.Domain.Runtime;
using Qul.Infrastructure.Runtime;

namespace Qul.Tests.Runtime;

/// <summary>
/// Java 选择策略与多来源合并。
/// 策略部分是纯函数，可以穷举；探测链路另外用本机真实安装做一次端到端验证。
/// </summary>
[TestClass]
public sealed class JavaRuntimeSelectionTests
{
    // ---------- 选择策略 ----------

    [TestMethod]
    public void Selector_ByDefaultPrefersTheSmallestSatisfyingVersion()
    {
        List<JavaRuntimeCandidate> all = new List<JavaRuntimeCandidate>
        {
            Runtime(@"C:\j25\java.exe", "25.0.4.1"),
            Runtime(@"C:\j17\java.exe", "17.0.20"),
            Runtime(@"C:\j11\java.exe", "11.0.32"),
            Runtime(@"C:\j8\java.exe", "1.8.0_503"),
        };

        // 这是对规划文档措辞的有意偏离：Minecraft 元数据里的 majorVersion 是**下界**，
        // 用"最新的"去跑 1.16.5（要求 8）会直接失败。
        Assert.AreEqual(
            @"C:\j8\java.exe",
            Select(all, 8).Selected!.ExecutablePath);

        Assert.AreEqual(
            @"C:\j17\java.exe",
            Select(all, 17).Selected!.ExecutablePath);

        Assert.AreEqual(
            @"C:\j17\java.exe",
            Select(all, 13).Selected!.ExecutablePath,
            "没有精确匹配时取满足要求里最小的那个");
    }

    [TestMethod]
    public void Selector_CanBeSwitchedToLatestWhenExplicitlyAsked()
    {
        List<JavaRuntimeCandidate> all = new List<JavaRuntimeCandidate>
        {
            Runtime(@"C:\j25\java.exe", "25.0.4.1"),
            Runtime(@"C:\j8\java.exe", "1.8.0_503"),
        };

        JavaSelectionResult latest = JavaRuntimeSelector.Select(
            all,
            new JavaSelectionRequest { RequiredMajorVersion = 8, Policy = JavaSelectionPolicy.Latest });

        Assert.AreEqual(@"C:\j25\java.exe", latest.Selected!.ExecutablePath);
    }

    [TestMethod]
    public void Selector_ReportsMismatchWithActionableExplanation()
    {
        List<JavaRuntimeCandidate> all = new List<JavaRuntimeCandidate>
        {
            Runtime(@"C:\j8\java.exe", "1.8.0_503"),
            Runtime(@"C:\j11\java.exe", "11.0.32"),
        };

        JavaSelectionResult result = Select(all, 17);

        Assert.IsFalse(result.Succeeded);
        Assert.AreEqual(ErrorCode.JavaVersionMismatch, result.Error);
        Assert.IsNotNull(result.Explanation);
        StringAssert.Contains(result.Explanation!, "17");
        StringAssert.Contains(result.Explanation!, "最低的是 8", "应指出本机最低的是哪个版本");
    }

    [TestMethod]
    public void Selector_ReportsMissingJavaWithAnExecutableInstruction()
    {
        JavaSelectionResult result = Select(new List<JavaRuntimeCandidate>(), 17);

        Assert.IsFalse(result.Succeeded);
        Assert.AreEqual(ErrorCode.JavaNotFound, result.Error);
        StringAssert.Contains(result.Explanation!, "请安装");
        StringAssert.Contains(result.Explanation!, "手动指定");
    }

    [TestMethod]
    public void Selector_HonoursArchitectureConstraint()
    {
        List<JavaRuntimeCandidate> all = new List<JavaRuntimeCandidate>
        {
            Runtime(@"C:\x86\java.exe", "1.8.0_503", arch: "x86", bitness: 32),
            Runtime(@"C:\x64\java.exe", "1.8.0_503", arch: "amd64", bitness: 64),
        };

        JavaSelectionResult x64 = JavaRuntimeSelector.Select(
            all,
            new JavaSelectionRequest { RequiredMajorVersion = 8, RequiredOsArch = "x86_64" });

        Assert.AreEqual(@"C:\x64\java.exe", x64.Selected!.ExecutablePath);

        JavaSelectionResult arm = JavaRuntimeSelector.Select(
            all,
            new JavaSelectionRequest { RequiredMajorVersion = 8, RequiredOsArch = "arm64" });

        Assert.IsFalse(arm.Succeeded, "本机没有 arm64 的 Java，必须如实报告而不是凑合选一个");
        Assert.AreEqual(ErrorCode.JavaVersionMismatch, arm.Error);
    }

    [TestMethod]
    public void Selector_Prefers64BitButHonoursExplicitBitnessRequirement()
    {
        List<JavaRuntimeCandidate> all = new List<JavaRuntimeCandidate>
        {
            Runtime(@"C:\a32\java.exe", "1.8.0_503", bitness: 32),
            Runtime(@"C:\a64\java.exe", "1.8.0_503", bitness: 64),
        };

        Assert.AreEqual(@"C:\a64\java.exe", Select(all, 8).Selected!.ExecutablePath, "未指定位宽时优先 64 位");

        JavaSelectionResult forced32 = JavaRuntimeSelector.Select(
            all,
            new JavaSelectionRequest { RequiredMajorVersion = 8, RequiredBitness = 32 });

        Assert.AreEqual(@"C:\a32\java.exe", forced32.Selected!.ExecutablePath);

        // 位宽未知（0）不构成拒绝理由：那是"探测不到"，不是"不匹配"。
        List<JavaRuntimeCandidate> unknown = new List<JavaRuntimeCandidate>
        {
            Runtime(@"C:\u\java.exe", "17.0.20", arch: null!, bitness: 0),
        };

        Assert.IsTrue(
            JavaRuntimeSelector.Select(unknown, new JavaSelectionRequest { RequiredMajorVersion = 8, RequiredBitness = 32 }).Succeeded);
    }

    [TestMethod]
    public void Selector_IsDeterministicRegardlessOfInputOrder()
    {
        JavaRuntimeCandidate a = Runtime(@"C:\b\java.exe", "1.8.0_503", bitness: 64);
        JavaRuntimeCandidate b = Runtime(@"C:\a\java.exe", "1.8.0_503", bitness: 64);

        JavaRuntimeCandidate? first = JavaRuntimeSelector
            .Select(new[] { a, b }, new JavaSelectionRequest { RequiredMajorVersion = 8 })
            .Selected;
        JavaRuntimeCandidate? second = JavaRuntimeSelector
            .Select(new[] { b, a }, new JavaSelectionRequest { RequiredMajorVersion = 8 })
            .Selected;

        Assert.IsNotNull(first);
        Assert.AreEqual(first!.ExecutablePath, second!.ExecutablePath, "并列候选必须由稳定规则裁决，不能随输入顺序漂移");
        Assert.AreEqual(@"C:\a\java.exe", first.ExecutablePath, "并列时以路径序收尾");
    }

    [TestMethod]
    public void Selector_ExplainsEveryCandidate()
    {
        JavaRuntimeCandidate good = Runtime(@"C:\good\java.exe", "17.0.20");
        JavaRuntimeCandidate old = Runtime(@"C:\old\java.exe", "1.8.0_503");
        JavaRuntimeCandidate weird = new JavaRuntimeCandidate { ExecutablePath = @"C:\weird\java.exe" };

        JavaSelectionResult result = JavaRuntimeSelector.Select(
            new[] { good, old, weird },
            new JavaSelectionRequest { RequiredMajorVersion = 17 });

        Assert.AreEqual(3, result.Assessments.Count, "每个候选都要有判定，这是'说得清'的落点");
        Assert.AreEqual(1, result.Assessments.Count(x => x.Accepted));
        Assert.IsTrue(result.Assessments.Where(x => !x.Accepted).All(x => !string.IsNullOrWhiteSpace(x.Reason)));
        Assert.IsTrue(result.Assessments.Any(x => x.Reason.Contains("无法识别")));
    }

    // ---------- 多来源合并 ----------

    [TestMethod]
    public void Resolver_PrefersTheManualOverride()
    {
        JavaRuntimeResolver resolver = new JavaRuntimeResolver(new IJavaRuntimeProvider[]
        {
            new FakeProvider("manual", Runtime(@"C:\manual\java.exe", "17.0.20", source: JavaRuntimeSource.Manual)),
            new FakeProvider("detected", Runtime(@"C:\detected\java.exe", "25.0.4.1")),
        });

        JavaResolutionOutcome outcome = resolver.Resolve(new JavaSelectionRequest { RequiredMajorVersion = 8 });

        Assert.IsTrue(outcome.Succeeded);
        Assert.AreEqual(@"C:\manual\java.exe", outcome.Selection.Selected!.ExecutablePath, "手动指定必须覆盖自动探测");
        Assert.IsTrue(outcome.Notes.Any(n => n.Contains("已采用手动指定")));
    }

    [TestMethod]
    public void Resolver_FallsBackWhenTheManualPathIsUnusable()
    {
        JavaRuntimeResolver resolver = new JavaRuntimeResolver(new IJavaRuntimeProvider[]
        {
            new FakeProvider("manual"),
            new FakeProvider("detected", Runtime(@"C:\detected\java.exe", "17.0.20")),
        });

        JavaResolutionOutcome outcome = resolver.Resolve(new JavaSelectionRequest { RequiredMajorVersion = 8 });

        Assert.AreEqual(@"C:\detected\java.exe", outcome.Selection.Selected!.ExecutablePath);
        Assert.IsTrue(
            outcome.Notes.Any(n => n.Contains("回落到本机探测")),
            "静默回落会让用户以为自己的设置生效了");
    }

    [TestMethod]
    public void Resolver_FallsBackWhenTheManualVersionIsTooLow()
    {
        JavaRuntimeResolver resolver = new JavaRuntimeResolver(new IJavaRuntimeProvider[]
        {
            new FakeProvider("manual", Runtime(@"C:\manual\java.exe", "1.8.0_503", source: JavaRuntimeSource.Manual)),
            new FakeProvider("detected", Runtime(@"C:\detected\java.exe", "25.0.4.1")),
        });

        JavaResolutionOutcome outcome = resolver.Resolve(new JavaSelectionRequest { RequiredMajorVersion = 25 });

        Assert.AreEqual(@"C:\detected\java.exe", outcome.Selection.Selected!.ExecutablePath);
        Assert.IsTrue(outcome.Notes.Any(n => n.Contains("不满足要求")));
    }

    [TestMethod]
    public void Resolver_DeduplicatesTheSameExecutableAcrossProviders()
    {
        const string Shared = @"C:\shared\java.exe";

        JavaRuntimeResolver resolver = new JavaRuntimeResolver(new IJavaRuntimeProvider[]
        {
            new FakeProvider("manual", Runtime(Shared, "17.0.20", source: JavaRuntimeSource.Manual)),
            new FakeProvider("detected", Runtime(Shared, "17.0.20"), Runtime(@"C:\other\java.exe", "17.0.20")),
        });

        JavaResolutionOutcome outcome = resolver.Resolve(new JavaSelectionRequest { RequiredMajorVersion = 8 });

        Assert.AreEqual(2, outcome.Candidates.Count, "同一可执行文件不该被算两次");
    }

    // ---------- 本机真实探测（端到端） ----------

    [TestMethod]
    public void Scanner_EnumeratesRealJavaInstallationsOnThisMachine()
    {
        List<string> paths = JavaInstallationScanner.CollectExecutablePaths();

        Assert.IsTrue(paths.Count > 0, "PATH 或 JAVA_HOME 上至少应有一个 java");
        Assert.IsTrue(paths.All(p => p.EndsWith("java.exe", StringComparison.OrdinalIgnoreCase)));
        Assert.AreEqual(paths.Count, paths.Distinct(StringComparer.OrdinalIgnoreCase).Count(), "候选不应重复");
        CollectionAssert.AreEqual(
            paths.OrderBy(p => p, StringComparer.OrdinalIgnoreCase).ToList(),
            paths,
            "候选顺序必须稳定");
        Assert.IsTrue(paths.Any(File.Exists), "至少应有一个实际存在的 java.exe");
    }

    [TestMethod]
    public void Probe_ExecutesRealJavaAndReportsVersionAndBitness()
    {
        string? executable = JavaInstallationScanner.CollectExecutablePaths().FirstOrDefault(File.Exists);

        Assert.IsNotNull(executable, "本机没有可执行的 java.exe，无法验证探测链路");

        JavaRuntimeCandidate? candidate = new JavaExecutableProbe().Probe(executable!);

        Assert.IsNotNull(candidate, "探测真实 java 失败：" + executable);
        Assert.IsTrue(candidate!.Version.Major >= 7, "主版本异常：" + candidate.Version.Major);
        Assert.IsTrue(
            candidate.Bitness == 32 || candidate.Bitness == 64,
            "位宽必须是 32 或 64，实际是 " + candidate.Bitness);
        Assert.IsNotNull(candidate.OsArch, "架构应能被解析出来");
        Assert.IsFalse(
            candidate.Describe().Contains("(?", StringComparison.Ordinal),
            "Describe 里不该出现未知架构：" + candidate.Describe());
    }

    [TestMethod]
    public void Scanner_FullScanSkipsUnrunnableDirectoriesInsteadOfFailing()
    {
        // 本机实测存在一个不含 java.exe 的空壳目录（Program Files\Java\latest）；
        // 扫描必须跳过它，而不是抛异常或产出无效候选。
        IReadOnlyList<JavaRuntimeCandidate> found = new JavaInstallationScanner().Scan();

        Assert.IsTrue(found.Count >= 1, "本机至少应探测到一个可用 Java");
        Assert.IsTrue(found.All(c => c.Version.IsValid));
        Assert.IsTrue(found.All(c => c.Source == JavaRuntimeSource.Detected));
        Assert.AreEqual(
            found.Count,
            found.Select(c => c.ExecutablePath).Distinct(StringComparer.OrdinalIgnoreCase).Count());
    }

    // ---------- 工具 ----------

    private static JavaSelectionResult Select(IReadOnlyList<JavaRuntimeCandidate> candidates, int requiredMajor)
    {
        return JavaRuntimeSelector.Select(
            candidates,
            new JavaSelectionRequest { RequiredMajorVersion = requiredMajor });
    }

    private static JavaRuntimeCandidate Runtime(
        string path,
        string version,
        string? arch = "amd64",
        int bitness = 64,
        JavaRuntimeSource source = JavaRuntimeSource.Detected)
    {
        Assert.IsTrue(JavaVersion.TryParse(version, out JavaVersion parsed), "测试数据版本号非法：" + version);

        return new JavaRuntimeCandidate
        {
            ExecutablePath = path,
            Version = parsed,
            OsArch = arch,
            Bitness = bitness,
            Source = source,
        };
    }

    private sealed class FakeProvider : IJavaRuntimeProvider
    {
        private readonly IReadOnlyList<JavaRuntimeCandidate> _candidates;

        public FakeProvider(string name, params JavaRuntimeCandidate[] candidates)
        {
            Name = name;
            _candidates = candidates;
            Capabilities = new JavaRuntimeCapabilities { CanEnumerate = true, EnabledByDefault = true };
        }

        public string Name { get; }

        public JavaRuntimeCapabilities Capabilities { get; }

        public IReadOnlyList<JavaRuntimeCandidate> Discover(CancellationToken cancellationToken)
        {
            return _candidates;
        }
    }
    // ---------- 配置 → 来源列表：接线本身 ----------

    [TestMethod]
    public void BuildJavaProviders_PutsTheManualPathFirstWhenConfigured()
    {
        // **这条守的是接线，不是解析器的语义。**
        //
        // 解析器早就会优先用手动指定（`Resolver_PrefersTheManualOverride` 守着那一半），
        // `ManualJavaRuntimeProvider` 也一直存在。缺的是**中间那根线**：
        // 生产线只传了"本机探测"，于是 `java.mode` / `java.manualPath`
        // 读写了却从不生效——而 P3 规范写着"用户手动指定覆盖"。
        //
        // 两个用例各守一半，合起来才是完整的。
        IReadOnlyList<IJavaRuntimeProvider> providers = LaunchPipeline.BuildJavaProviders(
            new JavaSettings { Mode = JavaSelectionMode.Manual, ManualPath = @"C:\some\java.exe" },
            null);

        Assert.AreEqual(2, providers.Count, "手动 + 探测，两个来源");
        Assert.AreEqual("manual", providers[0].Name, "手动指定必须排在第一个");
        Assert.AreEqual("detected", providers[1].Name, "本机探测仍在，用于回落");
    }

    [TestMethod]
    public void BuildJavaProviders_DoesNotAddAManualSourceWhenTheModeIsAuto()
    {
        // 自动模式下即使填了路径也不该生效——否则用户改成"自动"却仍被手动路径劫持。
        IReadOnlyList<IJavaRuntimeProvider> providers = LaunchPipeline.BuildJavaProviders(
            new JavaSettings { Mode = JavaSelectionMode.Auto, ManualPath = @"C:\some\java.exe" },
            null);

        Assert.AreEqual(1, providers.Count);
        Assert.AreEqual("detected", providers[0].Name);
    }

    [TestMethod]
    public void BuildJavaProviders_SkipsAnEmptyManualPath()
    {
        // 选了手动却没填路径：不要造一个永远返回空的来源。
        foreach (string? empty in new string?[] { null, string.Empty, "   " })
        {
            IReadOnlyList<IJavaRuntimeProvider> providers = LaunchPipeline.BuildJavaProviders(
                new JavaSettings { Mode = JavaSelectionMode.Manual, ManualPath = empty },
                null);

            Assert.AreEqual(1, providers.Count, "空路径不应产生手动来源：" + (empty ?? "<null>"));
            Assert.AreEqual("detected", providers[0].Name);
        }
    }

    [TestMethod]
    public void BuildJavaProviders_SurvivesANullSettingsObject()
    {
        // 防御：配置对象缺失时仍然给出可用的探测来源，而不是抛。
        IReadOnlyList<IJavaRuntimeProvider> providers = LaunchPipeline.BuildJavaProviders(null!, null);

        Assert.AreEqual(1, providers.Count);
        Assert.AreEqual("detected", providers[0].Name);
    }
}