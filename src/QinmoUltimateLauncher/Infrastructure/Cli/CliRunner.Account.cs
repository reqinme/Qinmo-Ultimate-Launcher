using System;
using System.Collections.Generic;
using System.Threading;
using Qul.Application.Ports;
using Qul.Application.Identity;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;
using Qul.Infrastructure.Auth;
using Qul.Infrastructure.Boot;
using Qul.Infrastructure.Net;
using Qul.Infrastructure.Security;

namespace Qul.Infrastructure.Cli;

/// <summary>
/// 账户相关的命令行入口（P5）。
///
/// 刻意单独成文件：它要么只读本地 DPAPI 存储、要么只走身份链路，
/// 与"检查更新/启动游戏"那几条命令不是一类东西。
/// </summary>
public static partial class CliRunner
{
    /// <summary>
    /// 账户状态 / 登出 / 登录。
    /// 凭据在磁盘上是 DPAPI 密文，**从不回显任何令牌**。
    /// </summary>
    private static int Account(BootContext boot, IReadOnlyList<string> args)
    {
        string action = args.Count > 1 ? args[1].Trim().ToLowerInvariant() : "status";

        switch (action)
        {
            case "status":
                return AccountStatus(boot);

            case "signout":
                return AccountSignOut(boot);

            case "login":
                return AccountLogin(boot);

            default:
                Report(boot, "用法：account [status|signout|login]");
                return ExitUsage;
        }
    }

    private static int AccountStatus(BootContext boot)
    {
        DpapiTokenStore store = new DpapiTokenStore(boot.Layout.SecretsDirectory, boot.Log);
        IReadOnlyList<string> accounts = store.ListAccounts();

        if (accounts.Count == 0)
        {
            Report(boot, "未登录：本机没有保存任何账户会话。");
            return ExitOk;
        }

        Report(boot, "已保存 " + accounts.Count + " 个账户会话（磁盘上为 DPAPI 密文）：");

        for (int i = 0; i < accounts.Count; i++)
        {
            Report(boot, "  " + accounts[i]);
        }

        return ExitOk;
    }

    private static int AccountSignOut(BootContext boot)
    {
        DpapiTokenStore store = new DpapiTokenStore(boot.Layout.SecretsDirectory, boot.Log);
        IReadOnlyList<string> accounts = store.ListAccounts();

        if (accounts.Count == 0)
        {
            Report(boot, "当前没有可登出的账户。");
            return ExitOk;
        }

        for (int i = 0; i < accounts.Count; i++)
        {
            store.Delete(accounts[i]);
            Report(boot, "已登出并清除本地凭据：" + accounts[i]);
        }

        Report(boot, "注意：这不会向服务端撤销授权，只是清除本机凭据。");
        return ExitOk;
    }

    /// <summary>
    /// 走设备码流程登录。
    ///
    /// **门禁未满足时一个网络请求都不发。** 拿未注册的 client_id 去撞授权端点，
    /// 拿到的是一个让人查半天的错误，而真正的原因在前置条件里。
    /// </summary>
    private static int AccountLogin(BootContext boot)
    {
        MicrosoftAuthPrerequisites prerequisites = boot.Config.Identity.Microsoft.ToPrerequisites();

        if (!prerequisites.IsSatisfied)
        {
            Report(boot, prerequisites.Describe());
            Report(boot, "在 config.json 的 identity.microsoft 里核对并置真相应项后重试。");
            return ExitUsage;
        }

        Report(boot, "正在向微软申请设备码…");

        DpapiTokenStore store = new DpapiTokenStore(boot.Layout.SecretsDirectory, boot.Log);
        MicrosoftAuthProvider provider = new MicrosoftAuthProvider(new HttpTransport(), prerequisites, boot.Log);

        using (CancellationTokenSource source = new CancellationTokenSource())
        {
            ConsoleCancelEventHandler handler = (sender, e) =>
            {
                // Ctrl+C 应当优雅取消，而不是让进程直接消失、留下半截状态
                e.Cancel = true;
                source.Cancel();
            };

            Console.CancelKeyPress += handler;

            try
            {
                AuthOutcome outcome = provider.Authenticate(
                    new AuthRequest { Prompt = new ConsoleAuthPrompt(boot) },
                    source.Token);

                if (!outcome.Succeeded || outcome.Session == null)
                {
                    Report(boot, "登录失败：" + (outcome.Error.HasValue ? outcome.Error.Value.ToString() : "未知错误"));
                    Report(boot, "  " + (outcome.Explanation ?? "没有更多说明。"));
                    return ExitFailed;
                }

                AuthSession session = outcome.Session;
                store.Save(MicrosoftAuthProvider.AccountKeyFor(session), session);

                Report(boot, string.Empty);
                Report(boot, "登录成功：" + session.UserName + "（" + session.Uuid + "）");
                Report(boot, "能否进入正版验证服务器：" + (session.ToIdentity().IsOnlineVerified ? "可以" : "不可以"));
                Report(boot, "会话已以 DPAPI 密文保存到本机，下次启动会自动恢复。");
                return ExitOk;
            }
            finally
            {
                Console.CancelKeyPress -= handler;
            }
        }
    }

    /// <summary>把设备码与进度打到控制台。用户要在浏览器里完成授权，所以必须写清楚。</summary>
    private sealed class ConsoleAuthPrompt : IAuthPrompt
    {
        private readonly BootContext _boot;

        public ConsoleAuthPrompt(BootContext boot)
        {
            _boot = boot;
        }

        public void ShowDeviceCode(string verificationUri, string userCode, DateTimeOffset expiresAt)
        {
            Report(_boot, string.Empty);
            Report(_boot, "  请在浏览器中打开：" + verificationUri);
            Report(_boot, "  并输入代码：" + userCode);
            Report(_boot, "  有效期至本地时间 " + expiresAt.ToLocalTime().ToString("HH:mm:ss") + "。");
            Report(_boot, string.Empty);
            Report(_boot, "完成后回到这里，正在等待授权…");
        }

        public void ReportProgress(string message)
        {
            Report(_boot, message);
        }
    }
}
