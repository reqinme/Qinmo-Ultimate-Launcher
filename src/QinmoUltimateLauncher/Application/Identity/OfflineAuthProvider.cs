using System;
using System.Collections.Generic;
using System.Threading;
using Qul.Application.Ports;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;

namespace Qul.Application.Identity;

/// <summary>
/// 离线身份来源（MVP）。
///
/// 它是"第二个实现"，因此 <see cref="IAuthProvider"/> 这个抽象不是空想出来的。
/// 能力声明把它的边界写死了：不联网、不能刷新、**进不了正版验证服务器**、
/// 但需要向用户显示限制说明。
/// </summary>
public sealed class OfflineAuthProvider : IAuthProvider
{
    private static readonly AuthCapabilities Declared = new AuthCapabilities
    {
        RequiresNetwork = false,
        SupportsRefresh = false,
        CanEnterOnlineServers = false,
        RequiresUserInteraction = false,
        RequiresExternalServiceTerms = false,
        EnabledByDefault = true,
    };

    private const string DefaultUserName = "Player";

    public string Name => "offline";

    public IdentitySource Source => IdentitySource.Offline;

    public AuthCapabilities Capabilities => Declared;

    public AuthOutcome Authenticate(AuthRequest request, CancellationToken cancellationToken)
    {
        string? userName = request?.OfflineUserName;

        if (string.IsNullOrWhiteSpace(userName))
        {
            userName = DefaultUserName;
        }

        try
        {
            PlayerIdentity identity = OfflineIdentityFactory.Create(userName!);

            return AuthOutcome.Success(new AuthSession
            {
                Source = IdentitySource.Offline,
                UserName = identity.UserName,
                Uuid = identity.Uuid,
                AccessToken = identity.AccessToken ?? string.Empty,

                // 离线来源没有刷新令牌，也没有到期时间——它不依赖任何服务端状态。
                RefreshToken = string.Empty,
                ExpiresAt = DateTimeOffset.MaxValue,
                UserType = "legacy",
            });
        }
        catch (LauncherException ex)
        {
            return AuthOutcome.Failure(ex.Code, ErrorCodes.Hint(ex.Code));
        }
    }

    public AuthOutcome Refresh(AuthSession session, CancellationToken cancellationToken)
    {
        // 离线会话永不过期，刷新是恒等操作。
        // 如实返回同一个会话，而不是报"不支持刷新"——那会让界面显示一个无意义的错误。
        return session == null
            ? AuthOutcome.Failure(ErrorCode.AuthTokenRefreshFailed, "没有可刷新的会话")
            : AuthOutcome.Success(session);
    }

    public void SignOut(string accountKey)
    {
        // 离线身份没有任何需要服务端撤销的东西；本地凭据由调用方自行清除。
    }

    /// <summary>供界面直接取用的限制说明，不必先构造一次认证。</summary>
    public static IReadOnlyList<string> CapabilityNotices => OfflineIdentityFactory.CapabilityNotices;
}
