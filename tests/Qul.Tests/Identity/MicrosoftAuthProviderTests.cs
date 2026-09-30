using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using System.Threading;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Ports;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;
using Qul.Infrastructure.Auth;

namespace Qul.Tests.Identity;

/// <summary>
/// 微软认证链路的结构测试。
///
/// **注意**：这些用例验证的是"请求形状与失败分类是否符合公开文档"，以及"门禁是否真的拦得住"。
/// 它们**不能**替代与真实服务的联调——那需要先满足 C1–C4 拿到可用的 client_id。
/// 本文件里的断言在该前提下才具备完整意义。
/// </summary>
[TestClass]
public sealed class MicrosoftAuthProviderTests
{
    private const string ClientId = "11111111-2222-3333-4444-555555555555";

    private const string DeviceCodeJson =
        "{\"device_code\":\"DEV-CODE\",\"user_code\":\"ABCD-EFGH\",\"verification_uri\":\"https://microsoft.com/link\"," +
        "\"expires_in\":900,\"interval\":1}";

    private const string TokenSuccessJson =
        "{\"access_token\":\"MSA-ACCESS\",\"refresh_token\":\"MSA-REFRESH\",\"expires_in\":3600}";

    private const string XboxJson = "{\"Token\":\"XBL-TOKEN\",\"DisplayClaims\":{\"xui\":[{\"uhs\":\"UHS-1\"}]}}";

    private const string XstsJson = "{\"Token\":\"XSTS-TOKEN\",\"DisplayClaims\":{\"xui\":[{\"uhs\":\"UHS-1\"}]}}";

    private const string MinecraftJson = "{\"access_token\":\"MC-ACCESS\",\"expires_in\":86400}";

    private const string ProfileJson = "{\"id\":\"069a79f444e94726a5befca90e38aaf5\",\"name\":\"QulTester\"}";

    // ---------- 门禁 ----------

    [TestMethod]
    public void Gate_BlocksBeforeSendingAnyRequest()
    {
        ScriptedTransport transport = new ScriptedTransport();
        MicrosoftAuthProvider provider = CreateProvider(transport, new MicrosoftAuthPrerequisites());

        AuthOutcome outcome = provider.Authenticate(new AuthRequest(), CancellationToken.None);

        Assert.IsFalse(outcome.Succeeded);
        Assert.AreEqual(ErrorCode.AuthPrerequisiteMissing, outcome.Error);
        StringAssert.Contains(outcome.Explanation!, "C1");
        Assert.AreEqual(
            0,
            transport.Requests.Count,
            "前置条件没满足就不该去撞授权端点——那只会换来一个让人查半天的错误");
    }

    // ---------- 设备码流程 ----------

    [TestMethod]
    public void DeviceCodeFlow_PollsThroughPendingAndProducesASession()
    {
        ScriptedTransport transport = new ScriptedTransport();
        RecordingPrompt prompt = new RecordingPrompt();

        transport.Enqueue(MicrosoftAuthEndpoints.DeviceCode, 200, DeviceCodeJson);
        transport.Enqueue(MicrosoftAuthEndpoints.Token, 400, "{\"error\":\"authorization_pending\"}");
        transport.Enqueue(MicrosoftAuthEndpoints.Token, 400, "{\"error\":\"slow_down\"}");
        transport.Enqueue(MicrosoftAuthEndpoints.Token, 200, TokenSuccessJson);
        transport.Enqueue(MicrosoftAuthEndpoints.XboxUserAuthenticate, 200, XboxJson);
        transport.Enqueue(MicrosoftAuthEndpoints.XboxXstsAuthorize, 200, XstsJson);
        transport.Enqueue(MicrosoftAuthEndpoints.MinecraftLoginWithXbox, 200, MinecraftJson);
        transport.Enqueue(MicrosoftAuthEndpoints.MinecraftProfile, 200, ProfileJson);

        AuthOutcome outcome = CreateProvider(transport, Ready()).Authenticate(
            new AuthRequest { Prompt = prompt }, CancellationToken.None);

        Assert.IsTrue(outcome.Succeeded, outcome.Explanation);
        Assert.AreEqual("QulTester", outcome.Session!.UserName);
        Assert.AreEqual("069a79f444e94726a5befca90e38aaf5", outcome.Session.Uuid);
        Assert.AreEqual("MC-ACCESS", outcome.Session.AccessToken, "注入启动参数的应当是 Minecraft 服务发的令牌");
        Assert.AreEqual("MSA-REFRESH", outcome.Session.RefreshToken);
        Assert.AreEqual("UHS-1", outcome.Session.XboxUserId);
        Assert.IsFalse(outcome.Session.IsExpired);

        Assert.AreEqual("ABCD-EFGH", prompt.UserCode, "用户码必须被送到界面");
        StringAssert.Contains(prompt.VerificationUri!, "microsoft.com");
        Assert.IsTrue(prompt.Progress.Count >= 3, "链路各段都应有过程叙述");
    }

    [TestMethod]
    public void DeclinedAuthorization_IsReportedAsUserCancelled()
    {
        ScriptedTransport transport = new ScriptedTransport();
        transport.Enqueue(MicrosoftAuthEndpoints.DeviceCode, 200, DeviceCodeJson);
        transport.Enqueue(MicrosoftAuthEndpoints.Token, 400, "{\"error\":\"authorization_declined\"}");

        AuthOutcome outcome = CreateProvider(transport, Ready()).Authenticate(new AuthRequest(), CancellationToken.None);

        Assert.IsFalse(outcome.Succeeded);
        Assert.AreEqual(ErrorCode.AuthUserCancelled, outcome.Error, "用户取消必须与网络故障区分开");
        StringAssert.Contains(outcome.Explanation!, "取消");
    }

    [TestMethod]
    public void ExpiredDeviceCode_AndBadClientId_AreClassifiedDifferently()
    {
        ScriptedTransport expired = new ScriptedTransport();
        expired.Enqueue(MicrosoftAuthEndpoints.DeviceCode, 200, DeviceCodeJson);
        expired.Enqueue(MicrosoftAuthEndpoints.Token, 400, "{\"error\":\"expired_token\"}");

        AuthOutcome expiredOutcome = CreateProvider(expired, Ready()).Authenticate(new AuthRequest(), CancellationToken.None);
        Assert.AreEqual(ErrorCode.AuthTokenRefreshFailed, expiredOutcome.Error);

        ScriptedTransport badClient = new ScriptedTransport();
        badClient.Enqueue(MicrosoftAuthEndpoints.DeviceCode, 200, DeviceCodeJson);
        badClient.Enqueue(MicrosoftAuthEndpoints.Token, 400, "{\"error\":\"unauthorized_client\"}");

        AuthOutcome badClientOutcome = CreateProvider(badClient, Ready()).Authenticate(new AuthRequest(), CancellationToken.None);
        Assert.AreEqual(
            ErrorCode.AuthPrerequisiteMissing,
            badClientOutcome.Error,
            "client_id 不被接受几乎总是前置条件的问题，不该报成网络错误");
    }

    [TestMethod]
    public void UnusableXboxAccount_IsExplainedInsteadOfShowingARawCode()
    {
        ScriptedTransport transport = new ScriptedTransport();
        transport.Enqueue(MicrosoftAuthEndpoints.DeviceCode, 200, DeviceCodeJson);
        transport.Enqueue(MicrosoftAuthEndpoints.Token, 200, TokenSuccessJson);
        transport.Enqueue(MicrosoftAuthEndpoints.XboxUserAuthenticate, 200, XboxJson);
        transport.Enqueue(MicrosoftAuthEndpoints.XboxXstsAuthorize, 401, "{\"XErr\":2148916233}");

        AuthOutcome outcome = CreateProvider(transport, Ready()).Authenticate(new AuthRequest(), CancellationToken.None);

        Assert.IsFalse(outcome.Succeeded);
        Assert.AreEqual(ErrorCode.AuthNoOwnership, outcome.Error);
        StringAssert.Contains(outcome.Explanation!, "Xbox 档案");
    }

    [TestMethod]
    public void MissingGameProfile_MeansNoOwnership()
    {
        ScriptedTransport transport = new ScriptedTransport();
        transport.Enqueue(MicrosoftAuthEndpoints.DeviceCode, 200, DeviceCodeJson);
        transport.Enqueue(MicrosoftAuthEndpoints.Token, 200, TokenSuccessJson);
        transport.Enqueue(MicrosoftAuthEndpoints.XboxUserAuthenticate, 200, XboxJson);
        transport.Enqueue(MicrosoftAuthEndpoints.XboxXstsAuthorize, 200, XstsJson);
        transport.Enqueue(MicrosoftAuthEndpoints.MinecraftLoginWithXbox, 200, MinecraftJson);
        transport.Enqueue(MicrosoftAuthEndpoints.MinecraftProfile, 404, "{}");

        AuthOutcome outcome = CreateProvider(transport, Ready()).Authenticate(new AuthRequest(), CancellationToken.None);

        Assert.AreEqual(ErrorCode.AuthNoOwnership, outcome.Error);
        StringAssert.Contains(outcome.Explanation!, "不拥有");
    }

    // ---------- 刷新 ----------

    [TestMethod]
    public void Refresh_UsesTheRefreshGrantAndRebuildsTheSession()
    {
        ScriptedTransport transport = new ScriptedTransport();
        transport.Enqueue(MicrosoftAuthEndpoints.Token, 200, "{\"access_token\":\"MSA-ACCESS-2\",\"refresh_token\":\"MSA-REFRESH-2\",\"expires_in\":3600}");
        transport.Enqueue(MicrosoftAuthEndpoints.XboxUserAuthenticate, 200, XboxJson);
        transport.Enqueue(MicrosoftAuthEndpoints.XboxXstsAuthorize, 200, XstsJson);
        transport.Enqueue(MicrosoftAuthEndpoints.MinecraftLoginWithXbox, 200, MinecraftJson);
        transport.Enqueue(MicrosoftAuthEndpoints.MinecraftProfile, 200, ProfileJson);

        AuthSession stale = new AuthSession
        {
            Source = IdentitySource.Microsoft,
            UserName = "QulTester",
            Uuid = "069a79f444e94726a5befca90e38aaf5",
            AccessToken = "OLD",
            RefreshToken = "MSA-REFRESH",
            ExpiresAt = DateTimeOffset.UtcNow.AddMinutes(-1),
        };

        AuthOutcome outcome = CreateProvider(transport, Ready()).Refresh(stale, CancellationToken.None);

        Assert.IsTrue(outcome.Succeeded, outcome.Explanation);
        Assert.AreEqual("MC-ACCESS", outcome.Session!.AccessToken);
        Assert.AreEqual("MSA-REFRESH-2", outcome.Session.RefreshToken, "刷新令牌换了新的就必须存新的");

        StringAssert.Contains(transport.Requests[0].Body!, "grant_type=refresh_token");
        StringAssert.Contains(transport.Requests[0].Body!, "refresh_token=MSA-REFRESH");
    }

    [TestMethod]
    public void Refresh_WithoutARefreshTokenFailsWithoutCallingAnything()
    {
        ScriptedTransport transport = new ScriptedTransport();

        AuthOutcome outcome = CreateProvider(transport, Ready()).Refresh(
            new AuthSession { Source = IdentitySource.Microsoft, RefreshToken = string.Empty },
            CancellationToken.None);

        Assert.IsFalse(outcome.Succeeded);
        Assert.AreEqual(ErrorCode.AuthTokenRefreshFailed, outcome.Error);
        Assert.AreEqual(0, transport.Requests.Count);
    }

    // ---------- 请求形状 ----------

    [TestMethod]
    public void RequestShapes_MatchTheDocumentedProtocol()
    {
        ScriptedTransport transport = new ScriptedTransport();
        transport.Enqueue(MicrosoftAuthEndpoints.DeviceCode, 200, DeviceCodeJson);
        transport.Enqueue(MicrosoftAuthEndpoints.Token, 200, TokenSuccessJson);
        transport.Enqueue(MicrosoftAuthEndpoints.XboxUserAuthenticate, 200, XboxJson);
        transport.Enqueue(MicrosoftAuthEndpoints.XboxXstsAuthorize, 200, XstsJson);
        transport.Enqueue(MicrosoftAuthEndpoints.MinecraftLoginWithXbox, 200, MinecraftJson);
        transport.Enqueue(MicrosoftAuthEndpoints.MinecraftProfile, 200, ProfileJson);

        CreateProvider(transport, Ready()).Authenticate(new AuthRequest(), CancellationToken.None);

        HttpFetchRequest deviceCode = transport.Requests[0];
        Assert.AreEqual("POST", deviceCode.Method);
        Assert.AreEqual("application/x-www-form-urlencoded", deviceCode.ContentType);
        StringAssert.Contains(deviceCode.Body!, "client_id=" + Uri.EscapeDataString(ClientId));
        StringAssert.Contains(deviceCode.Body!, "scope=");
        StringAssert.Contains(
            Uri.UnescapeDataString(deviceCode.Body!),
            "XboxLive.signin",
            "scope 必须包含 XboxLive.signin");
        StringAssert.Contains(
            Uri.UnescapeDataString(deviceCode.Body!),
            "offline_access",
            "offline_access 是拿到刷新令牌的前提");

        HttpFetchRequest xbox = transport.Requests[2];
        Assert.AreEqual("POST", xbox.Method);
        Assert.AreEqual("application/json", xbox.ContentType);
        StringAssert.Contains(xbox.Headers["x-xbl-contract-version"], "1");
        StringAssert.Contains(xbox.Body!, "\"RpsTicket\":\"d=MSA-ACCESS\"");
        StringAssert.Contains(xbox.Body!, "user.auth.xboxlive.com");

        HttpFetchRequest xsts = transport.Requests[3];
        StringAssert.Contains(xsts.Body!, "\"SandboxId\":\"RETAIL\"");
        StringAssert.Contains(xsts.Body!, "rp://api.minecraftservices.com/");

        HttpFetchRequest minecraft = transport.Requests[4];
        StringAssert.Contains(minecraft.Body!, "XBL3.0 x=UHS-1;XSTS-TOKEN");
    }

    // ---------- 工具 ----------

    private static MicrosoftAuthPrerequisites Ready()
    {
        return new MicrosoftAuthPrerequisites
        {
            ApplicationRegistered = true,
            ClientId = ClientId,
            ScopesConfirmed = true,
            FlowDecided = true,
            ThirdPartyTermsChecked = true,
        };
    }

    private static MicrosoftAuthProvider CreateProvider(ScriptedTransport transport, MicrosoftAuthPrerequisites prerequisites)
    {
        return new MicrosoftAuthProvider(
            transport,
            prerequisites,
            log: null,
            clock: () => DateTimeOffset.UtcNow,
            sleep: _ => { });
    }

    private sealed class RecordingPrompt : IAuthPrompt
    {
        public string? UserCode { get; private set; }

        public string? VerificationUri { get; private set; }

        public List<string> Progress { get; } = new List<string>();

        public void ShowDeviceCode(string verificationUri, string userCode, DateTimeOffset expiresAt)
        {
            VerificationUri = verificationUri;
            UserCode = userCode;
        }

        public void ReportProgress(string message)
        {
            Progress.Add(message);
        }
    }

    /// <summary>按 URL 排队返回预置响应；同一个 URL 的最后一个响应会被重复使用。</summary>
    private sealed class ScriptedTransport : IHttpTransport
    {
        private readonly Dictionary<string, Queue<KeyValuePair<int, string>>> _script =
            new Dictionary<string, Queue<KeyValuePair<int, string>>>(StringComparer.Ordinal);

        public List<HttpFetchRequest> Requests { get; } = new List<HttpFetchRequest>();

        public void Enqueue(string url, int status, string body)
        {
            if (!_script.TryGetValue(url, out Queue<KeyValuePair<int, string>>? queue))
            {
                queue = new Queue<KeyValuePair<int, string>>();
                _script[url] = queue;
            }

            queue.Enqueue(new KeyValuePair<int, string>(status, body));
        }

        public HttpFetchResponse Fetch(HttpFetchRequest request, CancellationToken cancellationToken)
        {
            Requests.Add(request);

            if (!_script.TryGetValue(request.Url, out Queue<KeyValuePair<int, string>>? queue) || queue.Count == 0)
            {
                return new HttpFetchResponse
                {
                    Status = HttpFetchStatus.Other,
                    StatusCode = 500,
                    Content = new MemoryStream(),
                };
            }

            KeyValuePair<int, string> entry = queue.Count > 1 ? queue.Dequeue() : queue.Peek();
            byte[] bytes = Encoding.UTF8.GetBytes(entry.Value);

            return new HttpFetchResponse
            {
                Status = entry.Key == 200 ? HttpFetchStatus.Success : HttpFetchStatus.Other,
                StatusCode = entry.Key,
                ContentLength = bytes.Length,
                TotalLength = bytes.Length,
                Content = new MemoryStream(bytes),
            };
        }
    }
    [TestMethod]
    public void Authenticate_SurfacesTheProviderErrorWhenTheDeviceCodeIsRejected()
    {
        // **这条用例守的是一个真实踩过的坑。**
        // 设备码被 Azure 拒绝时，先前只回一句"微软正版登录当前不可用"——
        // 那听起来像前置条件没满足，会把排查引向完全错误的方向。
        // 真实原因（AADSTS70002：应用必须标记为 mobile）就写在 401 的响应体里。
        ScriptedTransport transport = new ScriptedTransport();
        transport.Enqueue(
            MicrosoftAuthEndpoints.DeviceCode,
            401,
            "{\"error\":\"invalid_client\",\"error_description\":\"AADSTS70002: The client application must be marked as 'mobile'.\"}");

        MicrosoftAuthProvider provider = CreateProvider(transport, Ready());

        AuthOutcome outcome = provider.Authenticate(new AuthRequest(), CancellationToken.None);

        Assert.IsFalse(outcome.Succeeded);
        StringAssert.Contains(outcome.Explanation, "invalid_client", "必须带上 OAuth 的 error");
        StringAssert.Contains(outcome.Explanation, "AADSTS70002", "必须带上 Azure 的原因，否则排查会走错方向");
    }
}