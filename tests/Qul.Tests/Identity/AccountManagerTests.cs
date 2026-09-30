using System;
using System.IO;
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
/// 账户编排：恢复、登录、登出，以及"令牌加密存储"这条交付物
/// 真正被接进流程里（在此之前它实现了却从不被调用）。
/// </summary>
[TestClass]
public sealed class AccountManagerTests
{
    private const string AccountKey = "microsoft:069a79f444e94726a5befca90e38aaf5";
    private const string AccessToken = "SECRET-ACCESS-TOKEN-9f3c1d";
    private const string RefreshToken = "SECRET-REFRESH-TOKEN-2b8e4a";

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

    // ---------- 恢复 ----------

    [TestMethod]
    public void Restore_ReturnsNullWhenNothingWasStored()
    {
        AccountManager manager = NewManager(new FakeProvider());

        Assert.IsNull(manager.Restore(CancellationToken.None));
        Assert.IsFalse(manager.HasStoredSession());
    }

    [TestMethod]
    public void SignIn_ThenRestore_SurvivesARestart()
    {
        // 这才是"令牌加密存储"这条交付物的意义所在——
        // 在此之前存储写好了却没人调用，等于登录一次、关掉就要重登。
        FakeProvider provider = new FakeProvider();
        provider.AuthenticateResult = AuthOutcome.Success(Session(DateTimeOffset.UtcNow.AddHours(1)));

        DpapiTokenStore store = NewStore();

        AccountManager first = new AccountManager(provider, store, AccountKey);
        Assert.IsTrue(first.SignIn(new AuthRequest(), CancellationToken.None).Succeeded);

        // 换一个全新的 manager 实例，模拟下次启动
        AccountManager afterRestart = new AccountManager(new FakeProvider(), store, AccountKey);
        AuthSession? restored = afterRestart.Restore(CancellationToken.None);

        Assert.IsNotNull(restored);
        Assert.AreEqual("QulTester", restored!.UserName);
        Assert.AreEqual(AccessToken, restored.AccessToken);
        Assert.AreEqual(RefreshToken, restored.RefreshToken);
    }

    [TestMethod]
    public void Restore_RefreshesAnExpiredSessionAndPersistsTheNewOne()
    {
        FakeProvider provider = new FakeProvider();
        provider.AuthenticateResult = AuthOutcome.Success(Session(DateTimeOffset.UtcNow.AddMinutes(-5)));

        DpapiTokenStore store = NewStore();
        new AccountManager(provider, store, AccountKey).SignIn(new AuthRequest(), CancellationToken.None);

        provider.RefreshResult = AuthOutcome.Success(new AuthSession
        {
            Source = IdentitySource.Microsoft,
            UserName = "QulTester",
            Uuid = "069a79f444e94726a5befca90e38aaf5",
            AccessToken = "REFRESHED-ACCESS",
            RefreshToken = "REFRESHED-REFRESH",
            ExpiresAt = DateTimeOffset.UtcNow.AddHours(1),
        });

        AuthSession? restored = new AccountManager(provider, store, AccountKey).Restore(CancellationToken.None);

        Assert.IsNotNull(restored);
        Assert.AreEqual("REFRESHED-ACCESS", restored!.AccessToken);
        Assert.AreEqual(1, provider.RefreshCalls);

        // 刷新结果必须落盘，否则每次启动都要再刷一次
        AuthSession? reloaded = store.Load(AccountKey);
        Assert.IsNotNull(reloaded);
        Assert.AreEqual("REFRESHED-REFRESH", reloaded!.RefreshToken);
    }

    [TestMethod]
    public void Restore_DiscardsAnExpiredSessionThatCannotBeRefreshed()
    {
        // 留一份永远用不了的凭据只会让用户困惑：既登不进去，也看不出为什么。
        FakeProvider provider = new FakeProvider();
        provider.AuthenticateResult = AuthOutcome.Success(Session(DateTimeOffset.UtcNow.AddMinutes(-5)));

        DpapiTokenStore store = NewStore();
        new AccountManager(provider, store, AccountKey).SignIn(new AuthRequest(), CancellationToken.None);

        provider.RefreshResult = AuthOutcome.Failure(ErrorCode.AuthTokenRefreshFailed, "refresh rejected");

        AccountManager manager = new AccountManager(provider, store, AccountKey);
        Assert.IsNull(manager.Restore(CancellationToken.None));
        Assert.IsNull(store.Load(AccountKey), "刷新失败就必须清掉，不能留着");
    }

    [TestMethod]
    public void Restore_DiscardsAnExpiredSessionWhenTheProviderCannotRefresh()
    {
        FakeProvider provider = new FakeProvider();
        provider.AuthenticateResult = AuthOutcome.Success(Session(DateTimeOffset.UtcNow.AddMinutes(-5)));
        provider.Capabilities.SupportsRefresh = false;

        DpapiTokenStore store = NewStore();
        new AccountManager(provider, store, AccountKey).SignIn(new AuthRequest(), CancellationToken.None);

        Assert.IsNull(new AccountManager(provider, store, AccountKey).Restore(CancellationToken.None));
        Assert.AreEqual(0, provider.RefreshCalls, "来源不支持刷新就不该去试");
    }

    // ---------- 失败与登出 ----------

    [TestMethod]
    public void SignIn_WritesNothingWhenAuthenticationFails()
    {
        FakeProvider provider = new FakeProvider();
        provider.AuthenticateResult = AuthOutcome.Failure(ErrorCode.AuthUserCancelled, "用户取消了");

        DpapiTokenStore store = NewStore();
        AuthOutcome outcome = new AccountManager(provider, store, AccountKey).SignIn(new AuthRequest(), CancellationToken.None);

        Assert.IsFalse(outcome.Succeeded);
        Assert.AreEqual(ErrorCode.AuthUserCancelled, outcome.Error);
        Assert.IsNull(store.Load(AccountKey), "失败的登录不该在磁盘上留下任何痕迹");
    }

    [TestMethod]
    public void SignOut_ClearsTheStoredCredential()
    {
        FakeProvider provider = new FakeProvider();
        provider.AuthenticateResult = AuthOutcome.Success(Session(DateTimeOffset.UtcNow.AddHours(1)));

        DpapiTokenStore store = NewStore();
        AccountManager manager = new AccountManager(provider, store, AccountKey);
        manager.SignIn(new AuthRequest(), CancellationToken.None);

        manager.SignOut();

        Assert.IsNull(store.Load(AccountKey));
        Assert.IsFalse(manager.HasStoredSession());
        Assert.AreEqual(1, provider.SignOutCalls, "也要给来源一个清理的机会");
    }

    // ---------- 与安全基线的交叉检查 ----------

    [TestMethod]
    public void TheStoredCredentialNeverContainsPlaintextTokens()
    {
        FakeProvider provider = new FakeProvider();
        provider.AuthenticateResult = AuthOutcome.Success(Session(DateTimeOffset.UtcNow.AddHours(1)));

        DpapiTokenStore store = NewStore();
        new AccountManager(provider, store, AccountKey).SignIn(new AuthRequest(), CancellationToken.None);

        string[] files = Directory.GetFiles(store.Directory);
        Assert.AreEqual(1, files.Length);

        byte[] raw = File.ReadAllBytes(files[0]);

        foreach (string secret in new[] { AccessToken, RefreshToken })
        {
            byte[] pattern = System.Text.Encoding.ASCII.GetBytes(secret);
            Assert.IsFalse(Contains(raw, pattern), "经 AccountManager 落盘的密文里同样不得出现明文：" + secret);
        }
    }

    // ---------- 工具 ----------

    private DpapiTokenStore NewStore()
    {
        if (_sandbox == null)
        {
            _sandbox = Path.Combine(Path.GetTempPath(), "qul-account", Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(_sandbox);
        }

        return new DpapiTokenStore(_sandbox);
    }

    private AccountManager NewManager(FakeProvider provider)
    {
        return new AccountManager(provider, NewStore(), AccountKey);
    }

    private static AuthSession Session(DateTimeOffset expiresAt)
    {
        return new AuthSession
        {
            Source = IdentitySource.Microsoft,
            UserName = "QulTester",
            Uuid = "069a79f444e94726a5befca90e38aaf5",
            AccessToken = AccessToken,
            RefreshToken = RefreshToken,
            ExpiresAt = expiresAt,
        };
    }

    private static bool Contains(byte[] haystack, byte[] needle)
    {
        if (needle.Length == 0 || haystack.Length < needle.Length)
        {
            return false;
        }

        for (int i = 0; i <= haystack.Length - needle.Length; i++)
        {
            bool matched = true;

            for (int j = 0; j < needle.Length; j++)
            {
                if (haystack[i + j] != needle[j])
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

    private sealed class FakeProvider : IAuthProvider
    {
        public AuthOutcome AuthenticateResult { get; set; } =
            AuthOutcome.Failure(ErrorCode.AuthPrerequisiteMissing, "not configured");

        public AuthOutcome? RefreshResult { get; set; }

        public int RefreshCalls { get; private set; }

        public int SignOutCalls { get; private set; }

        public string Name => "fake";

        public IdentitySource Source => IdentitySource.Microsoft;

        public AuthCapabilities Capabilities { get; } = new AuthCapabilities
        {
            RequiresNetwork = true,
            SupportsRefresh = true,
            CanEnterOnlineServers = true,
            EnabledByDefault = true,
        };

        public AuthOutcome Authenticate(AuthRequest request, CancellationToken cancellationToken)
        {
            return AuthenticateResult;
        }

        public AuthOutcome Refresh(AuthSession session, CancellationToken cancellationToken)
        {
            RefreshCalls++;
            return RefreshResult ?? AuthOutcome.Failure(ErrorCode.AuthTokenRefreshFailed, "no refresh scripted");
        }

        public void SignOut(string accountKey)
        {
            SignOutCalls++;
        }
    }
}
