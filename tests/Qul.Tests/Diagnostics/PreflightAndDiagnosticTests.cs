using System;
using System.Collections.Generic;
using System.Linq;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Diagnostics;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Diagnostics;

namespace Qul.Tests.Diagnostics;

/// <summary>
/// P6 里能被钉死的那部分：预检清单的适用范围，以及诊断导出确实脱敏。
/// </summary>
[TestClass]
[DoNotParallelize]
/// <summary>
/// **不并行**：这几个用例会读写 <c>Redactor</c> 的全局静态脱敏配置，
/// 而整个测试程序集是方法级并行的。并行时一个用例改掉的路径前缀
/// 会把另一个用例的断言静默破坏——表现为"偶尔红一次、重跑又绿"，
/// 那比稳定失败更浪费时间：它会教人不必认真看待红灯。
/// </summary>
public sealed class PreflightAndDiagnosticTests
{
    private static readonly string[] OfflineNotices =
    {
        "离线账户仅在本机有效，无法进入正版验证（online-mode）服务器。",
        "离线账户不带访问令牌，因此进入不了需要正版验证的服务器与 Realms。",
        "该身份标识由本机按固定规则生成，仅作本地标识，与 Mojang 官方发放的账号标识无关。",
    };

    // ---------- 预检：适用范围 ----------

    [TestMethod]
    public void WithoutATarget_NotASingleServerConclusionIsProduced()
    {
        // 这是最要紧的一条纪律：用户没说要去哪，就什么都别说。
        // 一个凭空冒出来的服务器警告，比没有警告更糟。
        foreach (string? target in new[] { null, string.Empty, "   " })
        {
            IReadOnlyList<PreflightItem> items = PreflightCheck.Evaluate(Healthy(target));

            Assert.AreEqual(
                0,
                items.Count(i => i.Scope == PreflightScope.WhenTargetConfigured),
                "未配置目标时不得产生任何服务器兼容性结论（target=「" + (target ?? "null") + "」）");
        }
    }

    [TestMethod]
    public void WithAnOfflineTarget_TheServerWarningAppears()
    {
        IReadOnlyList<PreflightItem> items = PreflightCheck.Evaluate(Healthy("mc.example.com"));

        PreflightItem? item = items.FirstOrDefault(i => i.Id == PreflightCheck.IdServerCompatibility);

        Assert.IsNotNull(item, "配置了目标就该给出结论");
        Assert.AreEqual(PreflightScope.WhenTargetConfigured, item!.Scope);
        Assert.AreEqual(PreflightSeverity.Warning, item.Severity);
        StringAssert.Contains(item.Detail, "mc.example.com");
        Assert.IsNull(
            items.FirstOrDefault(i => i.Id == PreflightCheck.IdServerAddress),
            "正常地址不该被误报成形状可疑");
    }

    [TestMethod]
    public void VerifiedIdentity_WithATarget_ProducesNoCompatibilityWarning()
    {
        PreflightContext context = Healthy("mc.example.com");
        context.IdentitySource = IdentitySource.Microsoft;
        context.IsOnlineVerified = true;
        context.IdentityNotices = Array.Empty<string>();

        IReadOnlyList<PreflightItem> items = PreflightCheck.Evaluate(context);

        Assert.IsNull(items.FirstOrDefault(i => i.Id == PreflightCheck.IdServerCompatibility));
    }

    [TestMethod]
    public void MalformedTarget_IsWarnedAboutItsShape()
    {
        IReadOnlyList<PreflightItem> items = PreflightCheck.Evaluate(Healthy("这不是地址"));

        // 一个中文串、一段带协议的 URL，都不该被当成地址放过去。
        Assert.IsNotNull(items.FirstOrDefault(i => i.Id == PreflightCheck.IdServerAddress));
        Assert.IsNotNull(
            PreflightCheck.Evaluate(Healthy("https://mc.example.com")).FirstOrDefault(i => i.Id == PreflightCheck.IdServerAddress),
            "带协议的 URL 不是服务器地址，应当被提醒");

        PreflightItem? item = items.FirstOrDefault(i => i.Id == PreflightCheck.IdServerAddress);

        Assert.IsNotNull(item);
        Assert.AreEqual(PreflightSeverity.Warning, item!.Severity, "地址形状可疑只是提醒，不该拦住启动");
    }

    // ---------- 预检：身份告知不可跳过 ----------

    [TestMethod]
    public void OfflineIdentity_AlwaysRequiresAcknowledgement()
    {
        IReadOnlyList<PreflightItem> items = PreflightCheck.Evaluate(Healthy(null));

        PreflightItem? item = items.FirstOrDefault(i => i.Id == PreflightCheck.IdIdentityCapability);

        Assert.IsNotNull(item, "离线身份必须产生能力告知");
        Assert.IsTrue(item!.RequiresAcknowledgement, "这条告知不可默认跳过");
        Assert.AreEqual(PreflightScope.Always, item.Scope, "它与有没有配置服务器无关");
        Assert.AreEqual(PreflightSeverity.Warning, item.Severity);
        Assert.IsTrue(PreflightCheck.NeedsAcknowledgement(items));

        foreach (string notice in OfflineNotices)
        {
            StringAssert.Contains(item.Detail, notice);
        }
    }

    [TestMethod]
    public void HealthyOnlineIdentity_NeedsNoAcknowledgement()
    {
        PreflightContext context = Healthy(null);
        context.IdentitySource = IdentitySource.Microsoft;
        context.IsOnlineVerified = true;
        context.IdentityNotices = Array.Empty<string>();

        IReadOnlyList<PreflightItem> items = PreflightCheck.Evaluate(context);

        Assert.IsFalse(PreflightCheck.NeedsAcknowledgement(items));
        Assert.IsFalse(PreflightCheck.HasBlocking(items));
    }

    // ---------- 预检：阻断与提醒 ----------

    [TestMethod]
    public void MissingJava_BlocksTheLaunch()
    {
        PreflightContext context = Healthy(null);
        context.JavaAvailable = false;

        IReadOnlyList<PreflightItem> items = PreflightCheck.Evaluate(context);

        Assert.IsTrue(PreflightCheck.HasBlocking(items));
        Assert.AreEqual(PreflightSeverity.Blocking, items.First(i => i.Id == PreflightCheck.IdJavaAvailable).Severity);
    }

    [TestMethod]
    public void InsufficientDisk_BlocksTheLaunch()
    {
        PreflightContext context = Healthy(null);
        context.EstimatedBytes = 500L * 1024 * 1024;
        context.FreeDiskBytes = 100L * 1024 * 1024;

        IReadOnlyList<PreflightItem> items = PreflightCheck.Evaluate(context);

        Assert.IsTrue(PreflightCheck.HasBlocking(items));
        StringAssert.Contains(items.First(i => i.Id == PreflightCheck.IdDiskSpace).Detail, "MB");
    }

    [TestMethod]
    public void MissingClient_IsInformationalNotAnError()
    {
        // 没下过客户端是"首次启动要多等一会儿"，不是错误——报成错误会吓到人。
        PreflightContext context = Healthy(null);
        context.ClientInstalled = false;

        IReadOnlyList<PreflightItem> items = PreflightCheck.Evaluate(context);

        PreflightItem item = items.First(i => i.Id == PreflightCheck.IdClientInstalled);
        Assert.AreEqual(PreflightSeverity.Info, item.Severity);
        Assert.IsFalse(item.BlocksLaunch);
    }

    // ---------- 诊断导出 ----------

    [TestMethod]
    public void Report_MasksPathsUuidsAndAddresses()
    {
        const string ProfilePath = @"D:\QulTestProfile\Someone";
        Redactor.RegisterPathPrefix(ProfilePath, isDataRoot: false);

        string report = DiagnosticReportBuilder.Build(new DiagnosticReportInput
        {
            LauncherVersion = "0.1.0",
            DataPlacement = "Portable",
            Notes = new[]
            {
                ProfilePath + @"\data\logs\session-1.log",
                "远端地址 203.0.113.77 连接失败",
                "账户标识 069a79f4-44e9-4726-a5be-fca90e38aaf5",
            },
        });

        Assert.IsFalse(report.Contains(ProfilePath), "用户目录必须被折叠成占位符");
        StringAssert.Contains(report, Redactor.UserProfileMask);
        Assert.IsFalse(report.Contains("203.0.113.77"), "IP 地址必须被脱敏");
        StringAssert.Contains(report, Redactor.IpMask);
        Assert.IsFalse(report.Contains("069a79f4-44e9-4726-a5be-fca90e38aaf5"));
        StringAssert.Contains(report, Redactor.UuidMask);
    }

    [TestMethod]
    public void Report_MasksRegisteredSecrets()
    {
        const string Token = "eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiJ9.QUL-TEST-SECRET";
        Redactor.RegisterSecret(Token);

        string report = DiagnosticReportBuilder.Build(new DiagnosticReportInput
        {
            LauncherVersion = "0.1.0",
            DataPlacement = "Portable",
            ErrorDetail = "token rejected: " + Token,
            LogTail = new[] { "auth exchange=" + Token },
        });

        Assert.IsFalse(report.Contains(Token), "访问令牌绝不能出现在诊断报告里");
        Assert.IsFalse(report.Contains("QUL-TEST-SECRET"));
        StringAssert.Contains(report, Redactor.Mask);
    }

    [TestMethod]
    public void Report_CarriesWhatIsNeededToLocateTheProblem()
    {
        string report = DiagnosticReportBuilder.Build(new DiagnosticReportInput
        {
            LauncherVersion = "0.1.0",
            DataPlacement = "Portable",
            IdentitySource = IdentitySource.Offline,
            IsOnlineVerified = false,
            VersionId = "1.7.10",
            SkeletonHash = "49331bc0b7d3",
            Preflight = PreflightCheck.Evaluate(Healthy(null)),
            Error = ErrorCode.ProcStartFailed,
            ErrorDetail = "CreateProcess failed",
            LogTail = new[] { "line-1", "line-2" },
        });

        StringAssert.Contains(report, "QUL-PROC-0001");
        StringAssert.Contains(report, "1.7.10");
        StringAssert.Contains(report, "49331bc0b7d3");
        StringAssert.Contains(report, "line-2");
        StringAssert.Contains(report, "## 复现步骤", "没有复现步骤的报告等于把排查成本丢给用户");
        StringAssert.Contains(report, "[Always/Warning] identity.capability");
    }

    [TestMethod]
    public void Report_RendersEveryPreflightItemWithItsScope()
    {
        PreflightContext context = Healthy("mc.example.com");
        context.JavaAvailable = false;

        string report = DiagnosticReportBuilder.Build(new DiagnosticReportInput
        {
            LauncherVersion = "0.1.0",
            DataPlacement = "Portable",
            Preflight = PreflightCheck.Evaluate(context),
        });

        StringAssert.Contains(report, PreflightCheck.IdJavaAvailable);
        StringAssert.Contains(report, PreflightCheck.IdServerCompatibility);
        StringAssert.Contains(report, "[WhenTargetConfigured/Warning]");
    }

    [TestMethod]
    public void Report_WithNoErrorSaysSoRatherThanShowingBlank()
    {
        string report = DiagnosticReportBuilder.Build(new DiagnosticReportInput
        {
            LauncherVersion = "0.1.0",
            DataPlacement = "Portable",
        });

        StringAssert.Contains(report, "错误码=无");
        StringAssert.Contains(report, "（无）");
    }

    // ---------- 工具 ----------

    private static PreflightContext Healthy(string? target)
    {
        return new PreflightContext
        {
            IdentitySource = IdentitySource.Offline,
            IdentityNotices = OfflineNotices,
            IsOnlineVerified = false,
            ServerTarget = target,
            JavaAvailable = true,
            ClientInstalled = true,
            FreeDiskBytes = 100L * 1024 * 1024 * 1024,
            EstimatedBytes = 200L * 1024 * 1024,
        };
    }
}
