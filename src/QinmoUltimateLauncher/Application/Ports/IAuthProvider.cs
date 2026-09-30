using System;
using System.Threading;
using Qul.Domain.Configuration;
using Qul.Domain.Identity;

namespace Qul.Application.Ports;

/// <summary>
/// 身份来源的能力声明。
/// 与运行时来源一样，做成"声明"而不是"假设"——界面按能力渲染，
/// 而不是按来源类型写 if。
/// </summary>
public sealed class AuthCapabilities
{
    /// <summary>是否需要联网。离线来源为否。</summary>
    public bool RequiresNetwork { get; set; }

    /// <summary>是否支持静默刷新。</summary>
    public bool SupportsRefresh { get; set; }

    /// <summary>能否进入正版验证（online-mode）服务器。</summary>
    public bool CanEnterOnlineServers { get; set; }

    /// <summary>是否需要用户交互（打开浏览器、输入代码）。</summary>
    public bool RequiresUserInteraction { get; set; }

    /// <summary>是否依赖第三方服务条款。</summary>
    public bool RequiresExternalServiceTerms { get; set; }

    /// <summary>是否默认可用。可选能力一律默认关闭。</summary>
    public bool EnabledByDefault { get; set; }
}

/// <summary>
/// 认证过程中的用户交互钩子。
/// 做成接口是为了让认证流程能在没有界面的情况下被驱动与测试（命令行、自动化用例）。
/// </summary>
public interface IAuthPrompt
{
    /// <summary>告诉用户去哪里、输入什么代码。</summary>
    void ShowDeviceCode(string verificationUri, string userCode, DateTimeOffset expiresAt);

    /// <summary>过程叙述，例如"正在换取 Xbox 凭据…"。</summary>
    void ReportProgress(string message);
}

public sealed class AuthRequest
{
    public IAuthPrompt? Prompt { get; set; }

    /// <summary>离线来源用；其他来源忽略。</summary>
    public string? OfflineUserName { get; set; }
}

/// <summary>
/// 身份来源。
///
/// MVP 有两个实现（离线、微软正版），第三个（第三方验证）在 P10 且默认关闭——
/// 因此这个抽象现在就有存在理由。
/// </summary>
public interface IAuthProvider
{
    string Name { get; }

    IdentitySource Source { get; }

    AuthCapabilities Capabilities { get; }

    /// <summary>
    /// 取得一份可用的会话。失败时**返回带错误码的结果**，不抛裸异常——
    /// "用户取消"与"网络不通"必须能被界面区分开。
    /// </summary>
    AuthOutcome Authenticate(AuthRequest request, CancellationToken cancellationToken);

    /// <summary>用刷新令牌换一份新会话。不支持刷新或没有刷新令牌时返回失败。</summary>
    AuthOutcome Refresh(AuthSession session, CancellationToken cancellationToken);

    /// <summary>登出：清除本地凭据。绝不等同于"向服务端撤销授权"。</summary>
    void SignOut(string accountKey);
}
