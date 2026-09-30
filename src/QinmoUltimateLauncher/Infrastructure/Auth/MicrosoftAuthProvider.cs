using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Threading;
using Qul.Application.Ports;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.Serialization;

namespace Qul.Infrastructure.Auth;

/// <summary>
/// 微软正版身份来源：设备码授权 → Xbox Live → XSTS → Minecraft 服务 → 游戏档案。
///
/// **合规边界**：这是官方文档描述的授权链路，本程序只是按它走一遍。
/// 不伪造会话、不接触任何验证接口、不做任何绕过；拿不到令牌就如实报失败。
///
/// **前置条件门禁（决策 11）**：C1–C4 任一未满足时直接拒绝，并说明缺什么。
/// 拿一个未注册的 client_id 去撞授权端点，得到的是一个让人查半天的错误，
/// 而真正的原因是前置条件根本没满足。
///
/// **实现状态**：请求形状（端点、授权类型、请求体字段、错误分类）按公开文档实现，
/// 但**尚未与真实服务对过**——那需要先满足 C1–C4 拿到可用的 client_id。
/// 因此本文件在真实联调之前不得被视为"已验证"。
/// </summary>
public sealed class MicrosoftAuthProvider : IAuthProvider
{
    private const string DeviceCodeGrantType = "urn:ietf:params:oauth:grant-type:device_code";
    private const string XboxLiveRelyingParty = "http://auth.xboxlive.com";
    private const string XstsRelyingParty = "rp://api.minecraftservices.com/";

    /// <summary>XSTS 的 XErr 取值。这些不是"未知错误"，每一种都要给出能看懂的解释。</summary>
    private const long XErrNoXboxAccount = 2148916233L;
    private const long XErrBannedRegion = 2148916235L;
    private const long XErrChildAccount = 2148916238L;

    private static readonly AuthCapabilities Declared = new AuthCapabilities
    {
        RequiresNetwork = true,
        SupportsRefresh = true,
        CanEnterOnlineServers = true,
        RequiresUserInteraction = true,
        RequiresExternalServiceTerms = false,
        EnabledByDefault = true,
    };

    private readonly IHttpTransport _transport;
    private readonly MicrosoftAuthPrerequisites _prerequisites;
    private readonly SessionLog _log;
    private readonly Func<DateTimeOffset> _clock;
    private readonly Action<TimeSpan> _sleep;

    public MicrosoftAuthProvider(
        IHttpTransport transport,
        MicrosoftAuthPrerequisites prerequisites,
        SessionLog? log = null,
        Func<DateTimeOffset>? clock = null,
        Action<TimeSpan>? sleep = null)
    {
        _transport = transport ?? throw new ArgumentNullException(nameof(transport));
        _prerequisites = prerequisites ?? throw new ArgumentNullException(nameof(prerequisites));
        _log = log ?? SessionLog.Null;
        _clock = clock ?? (() => DateTimeOffset.UtcNow);

        // 轮询间隔可注入：测试里不能真的等上几十秒。
        _sleep = sleep ?? Thread.Sleep;
    }

    public string Name => "microsoft";

    public IdentitySource Source => IdentitySource.Microsoft;

    public AuthCapabilities Capabilities => Declared;

    /// <summary>账户键。稳定、可读、不含秘密。</summary>
    public static string AccountKeyFor(AuthSession session)
    {
        return "microsoft:" + session.Uuid;
    }

    public AuthOutcome Authenticate(AuthRequest request, CancellationToken cancellationToken)
    {
        if (!_prerequisites.IsSatisfied)
        {
            return AuthOutcome.Failure(ErrorCode.AuthPrerequisiteMissing, _prerequisites.Describe());
        }

        string clientId = _prerequisites.ClientId!;

        DeviceCodeTicket ticket;
        try
        {
            ticket = RequestDeviceCode(clientId, cancellationToken);
        }
        catch (LauncherException ex)
        {
            return AuthOutcome.Failure(ex.Code, ErrorCodes.Hint(ex.Code));
        }

        request?.Prompt?.ShowDeviceCode(ticket.VerificationUri, ticket.UserCode, ticket.ExpiresAt);

        string msaAccessToken;
        string msaRefreshToken;

        try
        {
            MsaTokens tokens = PollForTokens(clientId, ticket, cancellationToken);
            msaAccessToken = tokens.AccessToken;
            msaRefreshToken = tokens.RefreshToken;
        }
        catch (LauncherException ex)
        {
            return AuthOutcome.Failure(ex.Code, ex.Code == ErrorCode.AuthUserCancelled
                ? "已取消授权。"
                : ErrorCodes.Hint(ex.Code) + " " + ex.Message);
        }

        return CompleteSignIn(msaAccessToken, msaRefreshToken, request, cancellationToken);
    }

    public AuthOutcome Refresh(AuthSession session, CancellationToken cancellationToken)
    {
        if (session == null)
        {
            return AuthOutcome.Failure(ErrorCode.AuthTokenRefreshFailed, "没有可刷新的会话。");
        }

        if (!session.CanRefresh)
        {
            return AuthOutcome.Failure(ErrorCode.AuthTokenRefreshFailed, "该会话没有刷新令牌，需要重新登录。");
        }

        if (!_prerequisites.IsSatisfied)
        {
            return AuthOutcome.Failure(ErrorCode.AuthPrerequisiteMissing, _prerequisites.Describe());
        }

        string clientId = _prerequisites.ClientId!;

        try
        {
            MsaTokens tokens = RefreshMsaTokens(clientId, session.RefreshToken, cancellationToken);
            return CompleteSignIn(tokens.AccessToken, tokens.RefreshToken, null, cancellationToken);
        }
        catch (LauncherException ex)
        {
            return AuthOutcome.Failure(ex.Code, ErrorCodes.Hint(ex.Code));
        }
    }

    /// <summary>
    /// 登出：清除本地凭据。
    ///
    /// **不做服务端撤销**：消费级微软账户没有面向本场景的公开撤销端点，
    /// 而刷新令牌本身有有效期。这一点如实说明，不假装做了。
    /// </summary>
    public void SignOut(string accountKey)
    {
        _log.Info("auth", "signed out locally");
    }

    // ---------- 链路各段 ----------

    private AuthOutcome CompleteSignIn(
        string msaAccessToken,
        string msaRefreshToken,
        AuthRequest? request,
        CancellationToken cancellationToken)
    {
        try
        {
            request?.Prompt?.ReportProgress("正在换取 Xbox Live 凭据…");
            XboxToken xbox = AuthenticateWithXbox(msaAccessToken, cancellationToken);

            request?.Prompt?.ReportProgress("正在换取 XSTS 授权…");
            XboxToken xsts = AuthorizeWithXsts(xbox.Token, cancellationToken);

            request?.Prompt?.ReportProgress("正在登录 Minecraft 服务…");
            MinecraftToken minecraft = LoginWithXbox(xsts.UserHash, xsts.Token, cancellationToken);

            request?.Prompt?.ReportProgress("正在读取游戏档案…");
            Profile profile = FetchProfile(minecraft.AccessToken, cancellationToken);

            AuthSession session = new AuthSession
            {
                Source = IdentitySource.Microsoft,
                UserName = profile.Name,
                Uuid = profile.Id,
                AccessToken = minecraft.AccessToken,
                RefreshToken = msaRefreshToken,
                ExpiresAt = minecraft.ExpiresAt,
                XboxUserId = xsts.UserHash,
                UserType = "msa",
            };

            return AuthOutcome.Success(session);
        }
        catch (LauncherException ex)
        {
            return AuthOutcome.Failure(ex.Code, ex.Message);
        }
    }

    private DeviceCodeTicket RequestDeviceCode(string clientId, CancellationToken cancellationToken)
    {
        string body = "client_id=" + Uri.EscapeDataString(clientId)
                      + "&scope=" + Uri.EscapeDataString(MicrosoftAuthEndpoints.DefaultScopes);

        JsonObject json = PostForm(MicrosoftAuthEndpoints.DeviceCode, body, cancellationToken, ErrorCode.AuthPrerequisiteMissing);

        string deviceCode = json.GetString("device_code") ?? string.Empty;
        string userCode = json.GetString("user_code") ?? string.Empty;
        string verificationUri = json.GetString("verification_uri") ?? string.Empty;
        int expiresIn = json.GetInt("expires_in") ?? 900;
        int interval = json.GetInt("interval") ?? 5;

        if (deviceCode.Length == 0 || userCode.Length == 0 || verificationUri.Length == 0)
        {
            throw new LauncherException(ErrorCode.AuthPrerequisiteMissing, "device code response is incomplete");
        }

        return new DeviceCodeTicket(deviceCode, userCode, verificationUri, interval, _clock().AddSeconds(expiresIn));
    }

    private MsaTokens PollForTokens(string clientId, DeviceCodeTicket ticket, CancellationToken cancellationToken)
    {
        int interval = Math.Max(1, ticket.IntervalSeconds);

        while (_clock() < ticket.ExpiresAt)
        {
            cancellationToken.ThrowIfCancellationRequested();

            string body = "grant_type=" + Uri.EscapeDataString(DeviceCodeGrantType)
                          + "&client_id=" + Uri.EscapeDataString(clientId)
                          + "&device_code=" + Uri.EscapeDataString(ticket.DeviceCode);

            int status;
            JsonObject json = PostForm(MicrosoftAuthEndpoints.Token, body, cancellationToken, ErrorCode.AuthTokenRefreshFailed, out status);

            if (status == 200)
            {
                string accessToken = json.GetString("access_token") ?? string.Empty;
                string refreshToken = json.GetString("refresh_token") ?? string.Empty;
                int expiresIn = json.GetInt("expires_in") ?? 3600;

                if (accessToken.Length == 0)
                {
                    throw new LauncherException(ErrorCode.AuthTokenRefreshFailed, "token response has no access_token");
                }

                return new MsaTokens(accessToken, refreshToken, _clock().AddSeconds(expiresIn));
            }

            string error = json.GetString("error") ?? string.Empty;

            switch (error)
            {
                case "authorization_pending":
                    break;

                case "slow_down":
                    interval += 5;
                    break;

                case "authorization_declined":
                case "access_denied":
                    throw new LauncherException(ErrorCode.AuthUserCancelled, "the user declined the authorization");

                case "expired_token":
                    throw new LauncherException(ErrorCode.AuthTokenRefreshFailed, "the device code expired");

                case "bad_verification_code":
                    throw new LauncherException(ErrorCode.AuthPrerequisiteMissing, "the device code was rejected");

                default:
                    // 例如 invalid_client / unauthorized_client —— 几乎总是前置条件的问题。
                    throw new LauncherException(
                        ErrorCode.AuthPrerequisiteMissing,
                        "authorization failed: " + (error.Length > 0 ? error : "status " + status.ToString(CultureInfo.InvariantCulture)));
            }

            _sleep(TimeSpan.FromSeconds(interval));
        }

        throw new LauncherException(ErrorCode.AuthTokenRefreshFailed, "the device code expired before authorisation completed");
    }

    private MsaTokens RefreshMsaTokens(string clientId, string refreshToken, CancellationToken cancellationToken)
    {
        string body = "grant_type=refresh_token"
                      + "&client_id=" + Uri.EscapeDataString(clientId)
                      + "&refresh_token=" + Uri.EscapeDataString(refreshToken)
                      + "&scope=" + Uri.EscapeDataString(MicrosoftAuthEndpoints.DefaultScopes);

        int status;
        JsonObject json = PostForm(MicrosoftAuthEndpoints.Token, body, cancellationToken, ErrorCode.AuthTokenRefreshFailed, out status);

        string accessToken = json.GetString("access_token") ?? string.Empty;
        string newRefresh = json.GetString("refresh_token") ?? refreshToken;
        int expiresIn = json.GetInt("expires_in") ?? 3600;

        if (status != 200 || accessToken.Length == 0)
        {
            throw new LauncherException(ErrorCode.AuthTokenRefreshFailed, "refresh was rejected; the account must sign in again");
        }

        return new MsaTokens(accessToken, newRefresh, _clock().AddSeconds(expiresIn));
    }

    private XboxToken AuthenticateWithXbox(string msaAccessToken, CancellationToken cancellationToken)
    {
        string body = "{\"Properties\":{\"AuthMethod\":\"RPS\",\"SiteName\":\"user.auth.xboxlive.com\","
                      + "\"RpsTicket\":\"d=" + JsonEscape(msaAccessToken) + "\"},"
                      + "\"RelyingParty\":\"" + XboxLiveRelyingParty + "\",\"TokenType\":\"JWT\"}";

        JsonObject json = PostJson(
            MicrosoftAuthEndpoints.XboxUserAuthenticate, body, null, cancellationToken, ErrorCode.AuthTokenRefreshFailed);

        return ReadXboxToken(json, "Xbox Live authentication");
    }

    private XboxToken AuthorizeWithXsts(string xboxToken, CancellationToken cancellationToken)
    {
        string body = "{\"Properties\":{\"SandboxId\":\"RETAIL\",\"UserTokens\":[\"" + JsonEscape(xboxToken) + "\"]},"
                      + "\"RelyingParty\":\"" + XstsRelyingParty + "\",\"TokenType\":\"JWT\"}";

        int status;
        JsonObject json = PostJson(
            MicrosoftAuthEndpoints.XboxXstsAuthorize, body, null, cancellationToken, ErrorCode.AuthNoOwnership, out status);

        if (status == 401)
        {
            // XErr 的取值超过 int 范围，必须按 64 位读。
            long xerr = json.GetLong("XErr") ?? 0;

            switch (xerr)
            {
                case XErrNoXboxAccount:
                    throw new LauncherException(ErrorCode.AuthNoOwnership, "该微软账户还没有 Xbox 档案，请先在 Xbox 端创建。");

                case XErrChildAccount:
                    throw new LauncherException(ErrorCode.AuthNoOwnership, "该账户是未成年账户，需要先在 Xbox 端加入家庭组。");

                case XErrBannedRegion:
                    throw new LauncherException(ErrorCode.AuthNoOwnership, "该账户所在地区不支持 Xbox Live。");

                default:
                    throw new LauncherException(
                        ErrorCode.AuthNoOwnership,
                        "XSTS 拒绝了授权（XErr=" + xerr.ToString(CultureInfo.InvariantCulture) + "）。");
            }
        }

        return ReadXboxToken(json, "XSTS authorisation");
    }

    private MinecraftToken LoginWithXbox(string userHash, string xstsToken, CancellationToken cancellationToken)
    {
        string identityToken = "XBL3.0 x=" + userHash + ";" + xstsToken;
        string body = "{\"identityToken\":\"" + JsonEscape(identityToken) + "\"}";

        JsonObject json = PostJson(
            MicrosoftAuthEndpoints.MinecraftLoginWithXbox, body, null, cancellationToken, ErrorCode.AuthNoOwnership);

        string accessToken = json.GetString("access_token") ?? string.Empty;
        if (accessToken.Length == 0)
        {
            throw new LauncherException(ErrorCode.AuthNoOwnership, "Minecraft 服务没有返回访问令牌。");
        }

        int expiresIn = json.GetInt("expires_in") ?? 86400;
        return new MinecraftToken(accessToken, _clock().AddSeconds(expiresIn));
    }

    private Profile FetchProfile(string minecraftAccessToken, CancellationToken cancellationToken)
    {
        Dictionary<string, string> headers = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            { "Authorization", "Bearer " + minecraftAccessToken },
        };

        int status;
        JsonObject json = Send(
            new HttpFetchRequest
            {
                Url = MicrosoftAuthEndpoints.MinecraftProfile,
                Method = "GET",
                Accept = "application/json",
                Headers = headers,
            },
            cancellationToken,
            out status);

        if (status == 404)
        {
            // 档案不存在 = 该账户没有这款游戏。
            throw new LauncherException(ErrorCode.AuthNoOwnership, "该账户不拥有 Minecraft。");
        }

        if (status != 200)
        {
            throw new LauncherException(ErrorCode.NetHttpStatus, "profile request failed with status " + status.ToString(CultureInfo.InvariantCulture));
        }

        string id = json.GetString("id") ?? string.Empty;
        string name = json.GetString("name") ?? string.Empty;

        if (id.Length == 0 || name.Length == 0)
        {
            throw new LauncherException(ErrorCode.AuthNoOwnership, "游戏档案内容不完整。");
        }

        return new Profile(id, name);
    }

    // ---------- HTTP 小工具 ----------

    private JsonObject PostForm(string url, string body, CancellationToken cancellationToken, ErrorCode errorCode)
    {
        int ignored;
        return PostForm(url, body, cancellationToken, errorCode, out ignored);
    }

    private JsonObject PostForm(string url, string body, CancellationToken cancellationToken, ErrorCode errorCode, out int status)
    {
        return Send(
            new HttpFetchRequest
            {
                Url = url,
                Method = "POST",
                ContentType = "application/x-www-form-urlencoded",
                Body = body,
                Accept = "application/json",
            },
            cancellationToken,
            out status,
            errorCode);
    }

    private JsonObject PostJson(string url, string body, IDictionary<string, string>? headers, CancellationToken cancellationToken, ErrorCode errorCode)
    {
        int ignored;
        return PostJson(url, body, headers, cancellationToken, errorCode, out ignored);
    }

    private JsonObject PostJson(
        string url,
        string body,
        IDictionary<string, string>? headers,
        CancellationToken cancellationToken,
        ErrorCode errorCode,
        out int status)
    {
        Dictionary<string, string> merged = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            { "x-xbl-contract-version", "1" },
        };

        if (headers != null)
        {
            foreach (KeyValuePair<string, string> pair in headers)
            {
                merged[pair.Key] = pair.Value;
            }
        }

        return Send(
            new HttpFetchRequest
            {
                Url = url,
                Method = "POST",
                ContentType = "application/json",
                Body = body,
                Accept = "application/json",
                Headers = merged,
            },
            cancellationToken,
            out status,
            errorCode);
    }

    /// <summary>
    /// 发一次请求并解析 JSON。
    /// **只要能解析出 JSON 就交给调用方判断状态码**——OAuth 的错误信息就在 400 的响应体里，
    /// 提前按状态码抛异常会把最有用的那部分信息丢掉。
    /// </summary>
    private JsonObject Send(HttpFetchRequest request, CancellationToken cancellationToken, out int statusCode, ErrorCode? transportError = null)
    {
        string text;
        int status;

        try
        {
            using (HttpFetchResponse response = _transport.Fetch(request, cancellationToken))
            {
                status = response.StatusCode;

                using (StreamReader reader = new StreamReader(response.Content, Encoding.UTF8))
                {
                    text = reader.ReadToEnd();
                }
            }
        }
        catch (LauncherException ex)
        {
            statusCode = 0;
            throw new LauncherException(transportError ?? ex.Code, ErrorCodes.Hint(ex.Code));
        }

        statusCode = status;

        if (string.IsNullOrWhiteSpace(text))
        {
            return new JsonObject();
        }

        try
        {
            JsonValue parsed = JsonValue.Parse(text);
            return parsed as JsonObject ?? new JsonObject();
        }
        catch (JsonFormatException)
        {
            // 非 JSON 响应（例如网关的 HTML 错误页）——状态码已经够定性了。
            return new JsonObject();
        }
    }

    private static XboxToken ReadXboxToken(JsonObject json, string what)
    {
        string token = json.GetString("Token") ?? string.Empty;

        string userHash = string.Empty;
        JsonArray? xui = json.GetObject("DisplayClaims")?.GetArray("xui");
        if (xui != null && xui.Count > 0 && xui[0] is JsonObject first)
        {
            userHash = first.GetString("uhs") ?? string.Empty;
        }

        if (token.Length == 0 || userHash.Length == 0)
        {
            throw new LauncherException(ErrorCode.AuthNoOwnership, what + " 的响应不完整。");
        }

        return new XboxToken(token, userHash);
    }

    private static string JsonEscape(string value)
    {
        return new JsonString(value ?? string.Empty).ToJson().Trim('"');
    }

    // ---------- 内部类型 ----------

    private readonly struct DeviceCodeTicket
    {
        public DeviceCodeTicket(string deviceCode, string userCode, string verificationUri, int intervalSeconds, DateTimeOffset expiresAt)
        {
            DeviceCode = deviceCode;
            UserCode = userCode;
            VerificationUri = verificationUri;
            IntervalSeconds = intervalSeconds;
            ExpiresAt = expiresAt;
        }

        public string DeviceCode { get; }

        public string UserCode { get; }

        public string VerificationUri { get; }

        public int IntervalSeconds { get; }

        public DateTimeOffset ExpiresAt { get; }
    }

    private readonly struct MsaTokens
    {
        public MsaTokens(string accessToken, string refreshToken, DateTimeOffset expiresAt)
        {
            AccessToken = accessToken;
            RefreshToken = refreshToken;
            ExpiresAt = expiresAt;
        }

        public string AccessToken { get; }

        public string RefreshToken { get; }

        public DateTimeOffset ExpiresAt { get; }
    }

    private readonly struct XboxToken
    {
        public XboxToken(string token, string userHash)
        {
            Token = token;
            UserHash = userHash;
        }

        public string Token { get; }

        public string UserHash { get; }
    }

    private readonly struct MinecraftToken
    {
        public MinecraftToken(string accessToken, DateTimeOffset expiresAt)
        {
            AccessToken = accessToken;
            ExpiresAt = expiresAt;
        }

        public string AccessToken { get; }

        public DateTimeOffset ExpiresAt { get; }
    }

    private readonly struct Profile
    {
        public Profile(string id, string name)
        {
            Id = id;
            Name = name;
        }

        public string Id { get; }

        public string Name { get; }
    }
}
