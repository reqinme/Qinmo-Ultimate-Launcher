using System;
using System.Collections.Generic;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Boot;
using Qul.Infrastructure.Security;

namespace Qul.Infrastructure.Cli;

/// <summary>
/// 账户相关的命令行入口（P5 的"登出"交付物）。
///
/// 刻意单独成文件：它只读 DPAPI 存储、不碰网络，
/// 与"检查更新/启动游戏"那几条依赖外部服务的命令不是一类东西。
/// </summary>
public static partial class CliRunner
{
    /// <summary>
    /// 账户状态与登出。凭据在磁盘上是 DPAPI 密文，
    /// 这里只列账户键，**从不回显任何令牌**。
    /// </summary>
    private static int Account(BootContext boot, IReadOnlyList<string> args)
    {
        string action = args.Count > 1 ? args[1].Trim().ToLowerInvariant() : "status";

        DpapiTokenStore store = new DpapiTokenStore(boot.Layout.SecretsDirectory, boot.Log);
        IReadOnlyList<string> accounts = store.ListAccounts();

        switch (action)
        {
            case "status":
                if (accounts.Count == 0)
                {
                    Report(boot, "未登录：本机没有保存任何账户会话。");
                }
                else
                {
                    Report(boot, "已保存 " + accounts.Count + " 个账户会话（磁盘上为 DPAPI 密文）：");

                    for (int i = 0; i < accounts.Count; i++)
                    {
                        Report(boot, "  " + accounts[i]);
                    }
                }

                return ExitOk;

            case "signout":
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

            default:
                Report(boot, "用法：account [status|signout]");
                return ExitUsage;
        }
    }
}
