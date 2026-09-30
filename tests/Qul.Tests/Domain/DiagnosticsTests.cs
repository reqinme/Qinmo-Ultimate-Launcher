using System;
using System.Collections.Generic;
using System.Text.RegularExpressions;
using System.IO;
using Qul.Infrastructure.IO;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Diagnostics;

namespace Qul.Tests.Domain;

/// <summary>
/// 错误码目录与脱敏管道的守卫测试。
/// 这两样东西一旦漂移，症状是"用户看到未登记错误"和"令牌进了日志"，都属于发布后才发现的严重缺陷。
/// </summary>
[TestClass]
[DoNotParallelize]
/// <summary>
/// **不并行**：这几个用例会读写 <c>Redactor</c> 的全局静态脱敏配置，
/// 而整个测试程序集是方法级并行的。并行时一个用例改掉的路径前缀
/// 会把另一个用例的断言静默破坏——表现为"偶尔红一次、重跑又绿"，
/// 那比稳定失败更浪费时间：它会教人不必认真看待红灯。
/// </summary>
public sealed class DiagnosticsTests
{
    private static readonly Regex IdPattern = new Regex(@"^QUL-[A-Z]+-\d{4}$", RegexOptions.Compiled);

    [TestMethod]
    public void EveryErrorCode_HasRegisteredIdAndHint()
    {
        foreach (ErrorCode code in Enum.GetValues(typeof(ErrorCode)))
        {
            if (code == ErrorCode.None)
            {
                continue;
            }

            string id = ErrorCodes.Id(code);
            Assert.AreNotEqual(ErrorCodes.UnknownId, id, code + " 未登记错误码编号");
            Assert.IsTrue(IdPattern.IsMatch(id), code + " 的编号格式不合规：" + id);
            Assert.IsFalse(string.IsNullOrWhiteSpace(ErrorCodes.Hint(code)), code + " 缺少人话提示");
        }
    }

    [TestMethod]
    public void ErrorCodeIds_AreUnique()
    {
        Dictionary<string, ErrorCode> seen = new Dictionary<string, ErrorCode>(StringComparer.Ordinal);

        foreach (ErrorCode code in Enum.GetValues(typeof(ErrorCode)))
        {
            if (code == ErrorCode.None)
            {
                continue;
            }

            string id = ErrorCodes.Id(code);
            if (seen.TryGetValue(id, out ErrorCode owner))
            {
                Assert.Fail("编号重复：" + id + " 同时属于 " + owner + " 与 " + code);
            }

            seen[id] = code;
        }
    }

    [TestMethod]
    public void Redactor_MasksUuidAndIp()
    {
        string text = "account 069a79f4-44e9-4726-a5be-fca90e38aaf5 connected from 203.0.113.7";

        string scrubbed = Redactor.Scrub(text);

        StringAssert.DoesNotMatch(scrubbed, new Regex(@"069a79f4-44e9-4726-a5be-fca90e38aaf5"));
        StringAssert.DoesNotMatch(scrubbed, new Regex(@"203\.0\.113\.7"));
        StringAssert.Contains(scrubbed, Redactor.UuidMask);
        StringAssert.Contains(scrubbed, Redactor.IpMask);
    }

    [TestMethod]
    public void Redactor_MasksRegisteredSecret()
    {
        // 用唯一的假令牌，避免与其他并行用例互相干扰。
        string secret = "qinmo-test-token-" + Guid.NewGuid().ToString("N");
        Redactor.RegisterSecret(secret);

        string scrubbed = Redactor.Scrub("Authorization: Bearer " + secret);

        StringAssert.DoesNotMatch(scrubbed, new Regex(Regex.Escape(secret)));
        StringAssert.Contains(scrubbed, Redactor.Mask);
    }

    [TestMethod]
    public void Redactor_FoldsRegisteredPathPrefixes()
    {
        string fakeProfile = @"C:\Users\qinmo-spike-user-" + Guid.NewGuid().ToString("N");
        Redactor.RegisterPathPrefix(fakeProfile, isDataRoot: false);
        Redactor.RegisterPathPrefix(fakeProfile + @"\Qinmo\data", isDataRoot: true);

        string scrubbed = Redactor.Scrub(fakeProfile + @"\Qinmo\data\logs\session-1.log");

        StringAssert.DoesNotMatch(scrubbed, new Regex(Regex.Escape(fakeProfile)));
        StringAssert.StartsWith(scrubbed, Redactor.DataRootMask, "更长的数据根前缀必须优先于用户目录前缀被折叠");
    }

    [TestMethod]
    public void Redactor_ShortValuesAreNotTreatedAsSecrets()
    {
        // 过短的登记值会造成大面积误替换（例如把 "a" 全替换掉），必须直接忽略。
        Redactor.RegisterSecret("abc");

        Assert.AreEqual("abc def", Redactor.Scrub("abc def"));
    }

    [TestMethod]
    public void LogLevels_ParseRoundTrip()
    {
        foreach (LogLevel level in Enum.GetValues(typeof(LogLevel)))
        {
            string text = LogLevels.ToText(level);
            Assert.IsTrue(LogLevels.TryParse(text, out LogLevel parsed), "无法解析 " + text);
            Assert.AreEqual(level, parsed);
        }

        Assert.IsTrue(LogLevels.TryParse("WARNING", out LogLevel warn));
        Assert.AreEqual(LogLevel.Warn, warn);

        Assert.IsFalse(LogLevels.TryParse("verbose", out _), "未知级别必须被拒绝，而不是静默接受");
    }
    [TestMethod]
    public void Redactor_MasksTheThirtyTwoHexAccountIdThisProjectActuallyUses()
    {
        // **这条用例守的是一个"以为保护了、其实没有"的缺陷。**
        //
        // 原先的 Redactor_MasksUuidAndIp 用的是**带连字符**的 36 位形式，
        // 而本项目实际使用与落盘的是**无连字符**的 32 位形式
        // （AuthSession.Uuid / MicrosoftAuthProvider）。
        // 于是掩码规则从来没有生效过——用例却一直是绿的，
        // 因为它断言的是一个产品从不产生的形式。
        const string AccountId = "069a79f444e94726a5befca90e38aaf5";

        string scrubbed = Redactor.Scrub("account " + AccountId + " ready");

        StringAssert.DoesNotMatch(scrubbed, new Regex(AccountId));
        StringAssert.Contains(scrubbed, Redactor.UuidMask);
    }

    [TestMethod]
    public void SessionLog_ScrubsTheMessageNotJustTheDetail()
    {
        // **msg 与 detail 一样必须脱敏。**
        //
        // 先前只有 detail 走 Redactor，msg 直接落盘。
        // 而 App 在启动失败与未处理异常两条路径上，把异常 message 与**完整堆栈**
        // 经 msg 写盘——堆栈里必然含 `C:\Users\<用户名>\...`，
        // 而 BootContext 明明登记了 %USERPROFILE% 期望掩掉它。
        string root = Path.Combine(Path.GetTempPath(), "qul-log-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);

        try
        {
            DataLayout layout = DataLayout.Resolve(root, Path.Combine(root, "profile"));
            layout.EnsureCreated();

            string file;

            using (SessionLog log = SessionLog.Open(layout, LogLevel.Info))
            {
                file = log.FilePath;
                log.Warn("boot", "msg 里出现了 069a79f444e94726a5befca90e38aaf5 这样的账号标识");
            }

            string content = File.ReadAllText(file);

            StringAssert.DoesNotMatch(
                content,
                new Regex("069a79f444e94726a5befca90e38aaf5"),
                "经 msg 写入的内容同样必须被脱敏");
            StringAssert.Contains(content, Redactor.UuidMask);
        }
        finally
        {
            try { Directory.Delete(root, recursive: true); } catch (IOException) { }
        }
    }
}