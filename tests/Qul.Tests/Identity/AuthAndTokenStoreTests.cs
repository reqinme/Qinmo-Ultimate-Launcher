using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Identity;
using Qul.Application.Ports;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;
using Qul.Infrastructure.Security;

namespace Qul.Tests.Identity;

/// <summary>
/// P5 里不依赖外部前置条件的部分：会话模型、门禁、离线来源、DPAPI 加密存储。
/// </summary>
[TestClass]
public sealed class AuthAndTokenStoreTests
{
    private const string AccessToken = "SECRET-ACCESS-TOKEN-4f2a9c";
    private const string RefreshToken = "SECRET-REFRESH-TOKEN-8b1d3e";
    private const string UserName = "QulTester";
    private const string Uuid = "069a79f444e94726a5befca90e38aaf5";

    private string? _sandbox;

    [TestCleanup]
    public void Cleanup()
    {
        if (_sandbox != null && Directory.Exists(_sandbox))
        {
            try
            {
                Directory.Delete(_sandbox, recursive: true);
            }
            catch (IOException)
            {
            }
        }
    }

    // ---------- 会话模型 ----------

    [TestMethod]
    public void Session_ConsidersItselfExpiredTwoMinutesEarly()
    {
        // 留余量是必要的：刚好在启动瞬间过期，会让游戏拿着一个已失效的令牌去连服务器。
        AuthSession soon = new AuthSession { ExpiresAt = DateTimeOffset.UtcNow.AddMinutes(1) };
        AuthSession later = new AuthSession { ExpiresAt = DateTimeOffset.UtcNow.AddHours(1) };

        Assert.IsTrue(soon.IsExpired, "只剩一分钟就该当作已过期");
        Assert.IsFalse(later.IsExpired);
        Assert.IsFalse(new AuthSession { ExpiresAt = DateTimeOffset.MaxValue }.IsExpired);
    }

    [TestMethod]
    public void Session_DescribeLeaksNoCredential()
    {
        AuthSession session = SampleSession();
        string text = session.Describe();

        Assert.IsFalse(text.Contains(AccessToken));
        Assert.IsFalse(text.Contains(RefreshToken));
        StringAssert.Contains(text, UserName);
    }

    // ---------- 前置条件门禁 ----------

    [TestMethod]
    public void Prerequisites_GateIsClosedUntilEveryItemIsConfirmed()
    {
        MicrosoftAuthPrerequisites empty = new MicrosoftAuthPrerequisites();

        Assert.IsFalse(empty.IsSatisfied);
        Assert.AreEqual(5, empty.Unmet.Count, "五项都要能被单独指出，而不是笼统一句'未配置'");
        StringAssert.Contains(empty.Describe(), "微软正版登录当前不可用");
        StringAssert.Contains(empty.Describe(), "C1");
        StringAssert.Contains(empty.Describe(), "C4");

        MicrosoftAuthPrerequisites ready = new MicrosoftAuthPrerequisites
        {
            ApplicationRegistered = true,
            ClientId = "00000000-0000-0000-0000-000000000000",
            ScopesConfirmed = true,
            FlowDecided = true,
            ThirdPartyTermsChecked = true,
        };

        Assert.IsTrue(ready.IsSatisfied);
        Assert.AreEqual(string.Empty, ready.Describe());
    }

    // ---------- 离线来源 ----------

    [TestMethod]
    public void OfflineProvider_DeclaresItsLimitsHonestly()
    {
        OfflineAuthProvider provider = new OfflineAuthProvider();

        Assert.IsFalse(provider.Capabilities.RequiresNetwork);
        Assert.IsFalse(provider.Capabilities.SupportsRefresh);
        Assert.IsFalse(
            provider.Capabilities.CanEnterOnlineServers,
            "离线来源进不了正版验证服务器，能力声明必须如实——界面就是照它渲染的");
        Assert.IsTrue(provider.Capabilities.EnabledByDefault);
    }

    [TestMethod]
    public void OfflineProvider_ProducesALocalSession()
    {
        AuthOutcome outcome = new OfflineAuthProvider().Authenticate(
            new AuthRequest { OfflineUserName = "Steve" }, CancellationToken.None);

        Assert.IsTrue(outcome.Succeeded);
        Assert.AreEqual(IdentitySource.Offline, outcome.Session!.Source);
        Assert.AreEqual("Steve", outcome.Session.UserName);
        Assert.AreEqual(32, outcome.Session.Uuid.Length);
        Assert.IsFalse(outcome.Session.CanRefresh, "离线来源没有刷新令牌可言");
        Assert.IsFalse(outcome.Session.IsExpired);
        PlayerIdentity identity = outcome.Session.ToIdentity();
        Assert.IsFalse(identity.IsOnlineVerified, "离线身份不可能是已验证状态");
        Assert.IsTrue(
            identity.CapabilityNotices.Count >= 3,
            "离线身份必须带着它那三条强制告知，绝不能在中途被丢掉");
    }

    [TestMethod]
    public void OfflineProvider_ReportsInvalidNameAsAnErrorCode()
    {
        AuthOutcome outcome = new OfflineAuthProvider().Authenticate(
            new AuthRequest { OfflineUserName = "bad name" }, CancellationToken.None);

        Assert.IsFalse(outcome.Succeeded);
        Assert.AreEqual(ErrorCode.AuthOfflineNameInvalid, outcome.Error);
        Assert.IsNotNull(outcome.Explanation, "失败必须带一句能给人看的话");
    }

    // ---------- DPAPI 加密存储 ----------

    [TestMethod]
    public void TokenStore_RoundTripsEveryField()
    {
        DpapiTokenStore store = new DpapiTokenStore(NewSecrets());
        store.Save("account-1", SampleSession());

        AuthSession? loaded = store.Load("account-1");

        Assert.IsNotNull(loaded);
        Assert.AreEqual(IdentitySource.Microsoft, loaded!.Source);
        Assert.AreEqual(UserName, loaded.UserName);
        Assert.AreEqual(Uuid, loaded.Uuid);
        Assert.AreEqual(AccessToken, loaded.AccessToken);
        Assert.AreEqual(RefreshToken, loaded.RefreshToken);
        Assert.AreEqual("xuid-12345", loaded.XboxUserId);
        Assert.AreEqual("msa", loaded.UserType);
        Assert.IsTrue(Math.Abs((loaded.ExpiresAt - SampleSession().ExpiresAt).TotalSeconds) < 2);
    }

    [TestMethod]
    public void TokenStore_NeverWritesPlaintextToDisk()
    {
        // 这是安全基线里最要紧的一条，所以直接扫文件字节，而不是靠实现自觉。
        string secrets = NewSecrets();
        DpapiTokenStore store = new DpapiTokenStore(secrets);
        store.Save("account-1", SampleSession());

        string[] files = Directory.GetFiles(secrets);
        Assert.AreEqual(1, files.Length, "一次保存应当只产生一个密文文件");

        byte[] raw = File.ReadAllBytes(files[0]);

        foreach (string secret in new[] { AccessToken, RefreshToken, UserName, Uuid, "xuid-12345" })
        {
            Assert.IsFalse(
                ContainsAscii(raw, secret),
                "密文文件里出现了明文：" + secret);
        }
    }

    [TestMethod]
    public void TokenStore_ReturnsNullForUnknownAccount()
    {
        DpapiTokenStore store = new DpapiTokenStore(NewSecrets());

        Assert.IsNull(store.Load("never-saved"));
        Assert.IsNull(store.Load(string.Empty));
    }

    [TestMethod]
    public void TokenStore_DiscardsUnreadableBlobInsteadOfThrowing()
    {
        // 换了 Windows 账户、漫游配置变了、或密文被改坏——三种情况在磁盘上长得一样。
        // 正确行为是"当作没有登录过，请重新登录"，不是崩溃。
        string secrets = NewSecrets();
        DpapiTokenStore store = new DpapiTokenStore(secrets);
        store.Save("account-1", SampleSession());

        string blob = Directory.GetFiles(secrets).Single();
        File.WriteAllBytes(blob, new byte[] { 1, 2, 3, 4, 5, 6, 7, 8 });

        Assert.IsNull(store.Load("account-1"), "解不开就该当作没有登录过");
        Assert.AreEqual(0, Directory.GetFiles(secrets).Length, "解不开的残留必须被清掉，不能留个定时炸弹");
    }

    [TestMethod]
    public void TokenStore_DeleteRemovesTheBlob()
    {
        string secrets = NewSecrets();
        DpapiTokenStore store = new DpapiTokenStore(secrets);
        store.Save("account-1", SampleSession());

        store.Delete("account-1");

        Assert.AreEqual(0, Directory.GetFiles(secrets).Length);
        Assert.IsNull(store.Load("account-1"));
    }

    [TestMethod]
    public void TokenStore_KeepsAccountsSeparate()
    {
        string secrets = NewSecrets();
        DpapiTokenStore store = new DpapiTokenStore(secrets);

        store.Save("account-1", SampleSession());
        store.Save("account-2", new AuthSession
        {
            Source = IdentitySource.Microsoft,
            UserName = "Other",
            Uuid = "11111111111111111111111111111111",
            AccessToken = "OTHER-TOKEN",
            RefreshToken = "OTHER-REFRESH",
            ExpiresAt = DateTimeOffset.UtcNow.AddHours(1),
        });

        Assert.AreEqual(UserName, store.Load("account-1")!.UserName);
        Assert.AreEqual("Other", store.Load("account-2")!.UserName);
        Assert.IsNull(store.Load("account-3"));

        List<string> accounts = store.ListAccounts().ToList();
        CollectionAssert.AreEquivalent(new[] { "account-1", "account-2" }, accounts);
    }

    [TestMethod]
    public void TokenStore_ListSkipsUnreadableFiles()
    {
        string secrets = NewSecrets();
        DpapiTokenStore store = new DpapiTokenStore(secrets);

        store.Save("account-1", SampleSession());
        File.WriteAllBytes(Path.Combine(secrets, "garbage.bin"), new byte[] { 9, 9, 9 });

        List<string> accounts = store.ListAccounts().ToList();

        CollectionAssert.AreEqual(new[] { "account-1" }, accounts, "读不出来的文件不该让整个列表失败");
    }

    // ---------- 工具 ----------

    private string NewSecrets()
    {
        _sandbox = Path.Combine(Path.GetTempPath(), "qul-auth", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_sandbox);
        return _sandbox;
    }

    private static AuthSession SampleSession()
    {
        return new AuthSession
        {
            Source = IdentitySource.Microsoft,
            UserName = UserName,
            Uuid = Uuid,
            AccessToken = AccessToken,
            RefreshToken = RefreshToken,
            ExpiresAt = DateTimeOffset.UtcNow.AddHours(1),
            XboxUserId = "xuid-12345",
            UserType = "msa",
        };
    }

    /// <summary>按字节找 ASCII 子串，不受任何编码影响。</summary>
    private static bool ContainsAscii(byte[] haystack, string needle)
    {
        byte[] pattern = Encoding.ASCII.GetBytes(needle);

        if (pattern.Length == 0 || haystack.Length < pattern.Length)
        {
            return false;
        }

        for (int i = 0; i <= haystack.Length - pattern.Length; i++)
        {
            bool matched = true;

            for (int j = 0; j < pattern.Length; j++)
            {
                if (haystack[i + j] != pattern[j])
                {
                    matched = false;
                    break;
                }
            }

            if (matched)
            {
                return true;
            }
        }

        return false;
    }
}
