using System;
using Qul.Domain.Configuration;

namespace Qul.Domain.Identity;

/// <summary>
/// 一次认证得到的会话。
///
/// **这个对象含秘密。** 它只允许出现在两个地方：内存中，以及经系统凭据保护加密后的磁盘密文里。
/// 它绝不进入启动计划骨架、绝不进日志、绝不出现在诊断导出里。
/// </summary>
public sealed class AuthSession
{
    public IdentitySource Source { get; set; } = IdentitySource.Microsoft;

    public string UserName { get; set; } = string.Empty;

    /// <summary>32 位无连字符十六进制。</summary>
    public string Uuid { get; set; } = string.Empty;

    /// <summary>访问令牌。秘密。</summary>
    public string AccessToken { get; set; } = string.Empty;

    /// <summary>刷新令牌。秘密，且比访问令牌更敏感——它能换出新的访问令牌。</summary>
    public string RefreshToken { get; set; } = string.Empty;

    public DateTimeOffset ExpiresAt { get; set; }

    public string? XboxUserId { get; set; }

    /// <summary>注入 --userType 的值。</summary>
    public string UserType { get; set; } = "msa";

    /// <summary>
    /// 是否已过期（留两分钟余量）。
    /// 余量是必要的：刚好在启动瞬间过期会让游戏拿着一个已失效的令牌去连服务器。
    /// </summary>
    public bool IsExpired => DateTimeOffset.UtcNow >= ExpiresAt - TimeSpan.FromMinutes(2);

    public bool CanRefresh => !string.IsNullOrEmpty(RefreshToken);

    public PlayerIdentity ToIdentity()
    {
        // 离线来源绝不可能是"已验证"，而且必须带上它那三条强制告知。
        // 这两个字段一旦写死，离线账户就会被伪装成正版，界面也再也拿不到该显示的告知——
        // 那是直接踩合规红线的写法，所以由来源决定，而不是给个默认值了事。
        bool online = Source != IdentitySource.Offline;

        return new PlayerIdentity(Source, UserName, Uuid)
        {
            AccessToken = AccessToken,
            UserType = UserType,
            IsOnlineVerified = online,
            CapabilityNotices = online ? Array.Empty<string>() : OfflineIdentityFactory.CapabilityNotices,
        };
    }

    /// <summary>用于日志与界面，**不含任何凭据**。</summary>
    public string Describe()
    {
        return Source + " / " + UserName + " / " + Uuid + " / 到期 " + ExpiresAt.ToString("u");
    }
}

/// <summary>认证结果。失败时携带错误码与一句可读的解释，绝不抛裸异常给界面。</summary>
public sealed class AuthOutcome
{
    private AuthOutcome(AuthSession? session, Diagnostics.ErrorCode? error, string? explanation)
    {
        Session = session;
        Error = error;
        Explanation = explanation;
    }

    public AuthSession? Session { get; }

    public Diagnostics.ErrorCode? Error { get; }

    public string? Explanation { get; }

    public bool Succeeded => Session != null;

    public static AuthOutcome Success(AuthSession session)
    {
        return new AuthOutcome(session, null, null);
    }

    public static AuthOutcome Failure(Diagnostics.ErrorCode error, string explanation)
    {
        return new AuthOutcome(null, error, explanation);
    }
}
