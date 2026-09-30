using System;
using System.Collections.Generic;
using System.Threading;
using Qul.Application.Ports;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;
using Qul.Infrastructure.Diagnostics;

namespace Qul.Application.Identity;

/// <summary>
/// 账户状态的编排：恢复、登录、登出。
///
/// 在这之前，<see cref="IAuthProvider"/> 与 <see cref="ITokenStore"/> 两半都写好也测过了，
/// **却没有任何地方把它们连起来**——令牌存储实现了却从不被调用，
/// 等于"登录一次，关掉就要重登"。这个类补的就是这一段。
///
/// 它刻意不碰界面、不碰网络细节：只负责"会话该不该留、要不要刷新、什么时候清掉"。
/// </summary>
public sealed class AccountManager
{
    private readonly IAuthProvider _provider;
    private readonly ITokenStore _tokens;
    private readonly string _accountKey;
    private readonly SessionLog _log;

    public AccountManager(IAuthProvider provider, ITokenStore tokens, string accountKey, SessionLog? log = null)
    {
        _provider = provider ?? throw new ArgumentNullException(nameof(provider));
        _tokens = tokens ?? throw new ArgumentNullException(nameof(tokens));

        if (string.IsNullOrWhiteSpace(accountKey))
        {
            throw new ArgumentException("account key is required", nameof(accountKey));
        }

        _accountKey = accountKey;
        _log = log ?? SessionLog.Null;
    }

    public string AccountKey => _accountKey;

    /// <summary>从加密存储恢复指定账户的会话。</summary>
    public AuthSession? Restore(CancellationToken cancellationToken)
    {
        return RestoreKey(_accountKey, cancellationToken);
    }

    /// <summary>
    /// 从任意一个已保存的账户恢复。
    ///
    /// 需要它的原因很实际：账户键里含 uuid，而**启动时我们还不知道 uuid**——
    /// 得先登录才知道。所以恢复只能反过来，从存储里已有哪些账户去找。
    /// </summary>
    public AuthSession? RestoreAny(CancellationToken cancellationToken)
    {
        IReadOnlyList<string> keys = _tokens.ListAccounts();

        for (int i = 0; i < keys.Count; i++)
        {
            AuthSession? candidate = RestoreKey(keys[i], cancellationToken);

            if (candidate != null)
            {
                return candidate;
            }
        }

        return null;
    }

    /// <summary>
    /// 登录并落盘。**只有成功才写** ——失败的登录不该在磁盘上留下任何痕迹。
    /// </summary>
    public AuthOutcome SignIn(AuthRequest request, CancellationToken cancellationToken)
    {
        AuthOutcome outcome = _provider.Authenticate(request, cancellationToken);

        if (!outcome.Succeeded || outcome.Session == null)
        {
            return outcome;
        }

        RegisterSecrets(outcome.Session);
        Save(outcome.Session);
        return outcome;
    }

    /// <summary>
    /// 登出：清除本地凭据。
    /// **不等于向服务端撤销授权**——这一点在 <see cref="IAuthProvider.SignOut"/> 里也没假装做过。
    /// </summary>
    public void SignOut()
    {
        _tokens.Delete(_accountKey);
        _provider.SignOut(_accountKey);
        _log.Info("auth", "signed out and cleared the stored credential");
    }

    /// <summary>当前是否存有一份可用会话。不做网络访问，只读本地。</summary>
    public bool HasStoredSession()
    {
        AuthSession? stored = _tokens.Load(_accountKey);
        return stored != null && !stored.IsExpired;
    }

    /// <summary>
    /// 恢复某个账户键对应的会话。
    ///
    /// 三种情况分得很清楚：
    ///   仍然有效        → 直接用
    ///   过期但能刷新    → 刷新后落盘新会话，用新的
    ///   过期且不能刷新  → **清掉**，让用户重新登录（而不是留一份永远用不了的凭据）
    ///
    /// 任何一步失败都返回 null 并如实记日志——"记不住登录"是遗憾，不是错误。
    /// </summary>
    private AuthSession? RestoreKey(string accountKey, CancellationToken cancellationToken)
    {
        AuthSession? stored = _tokens.Load(accountKey);

        if (stored == null)
        {
            return null;
        }

        if (!stored.IsExpired)
        {
            RegisterSecrets(stored);
            _log.Info("auth", "restored a valid session");
            return stored;
        }

        if (!stored.CanRefresh || !_provider.Capabilities.SupportsRefresh)
        {
            _log.Info("auth", "stored session expired and cannot be refreshed; discarding");
            _tokens.Delete(accountKey);
            return null;
        }

        AuthOutcome refreshed = _provider.Refresh(stored, cancellationToken);

        if (!refreshed.Succeeded || refreshed.Session == null)
        {
            _log.Warn("auth", "refresh failed; discarding the stored session", refreshed.Error ?? ErrorCode.AuthTokenRefreshFailed);
            _tokens.Delete(accountKey);
            return null;
        }

        RegisterSecrets(refreshed.Session);
        Save(refreshed.Session);
        return refreshed.Session;
    }

    /// <summary>
    /// 把会话里的令牌登记进脱敏器，**让它们万一出现在日志里也会被掩掉**。
    ///
    /// 这个机制原先全工程**零调用**——一个宣称提供保护、实际从不生效的东西
    /// 比没有更糟，因为它让人以为已经保护了。
    ///
    /// 两处都要登记：登录时（Save 之前）与**恢复时**。
    /// 只登记登录的话，下次启动从磁盘恢复出来的令牌就不受保护了。
    ///
    /// 顺带把 <c>Describe()</c> 从日志里去掉：它含用户名与裸 uuid，
    /// 而这两样正是脱敏规则要掩的东西。
    /// </summary>
    private static void RegisterSecrets(AuthSession session)
    {
        Redactor.RegisterSecret(session.AccessToken);
        Redactor.RegisterSecret(session.RefreshToken);
    }

    private void Save(AuthSession session)
    {
        try
        {
            _tokens.Save(_accountKey, session);
            _log.Info("auth", "saved the session");
        }
        catch (LauncherException ex)
        {
            // 存不下不代表这次登录没用——本次仍然可用，只是下次要重登。
            _log.Warn("auth", "could not persist the session", ex.Code);
        }
    }
}
