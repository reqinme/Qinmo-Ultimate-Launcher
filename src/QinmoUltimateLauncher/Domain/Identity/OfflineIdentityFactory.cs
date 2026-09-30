using System;
using System.Collections.Generic;
using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;

namespace Qul.Domain.Identity;

/// <summary>
/// 离线身份的最小实现。
///
/// **合规边界（必须原文保留）**：
/// 这里生成的是一个**纯本地标识**。它不会向 Mojang 发送任何请求，不伪造官方会话，
/// 也换不来任何在线能力。它唯一的作用是让游戏客户端有一个名字与一个稳定的本地标识可以带进启动参数。
///
/// 生成规则刻意采用社区通行的 <c>OfflinePlayer:&lt;名字&gt;</c> 约定，理由是互通：
/// 离线模式服务端就是用同一规则算出玩家标识的，用别的规则会让客户端与服务端对不上。
///
/// **为什么可以用 MD5**：这里的 MD5 不是安全原语，只是"把一个名字稳定地映射成一串十六进制"的哈希约定。
/// 它不承担任何防篡改或防碰撞的安全职责——身份标识本身不含秘密，改了也只是换一个本地名字。
/// 生成的标识是 **UUID v3**，而 Mojang 官方发放的是 **v4**，版本位不同，因此两者在结构上可区分、不会混淆。
/// </summary>
public static class OfflineIdentityFactory
{
    /// <summary>社区通行命名空间前缀。</summary>
    public const string NamespacePrefix = "OfflinePlayer:";

    /// <summary>
    /// 离线账户的访问令牌占位值。
    /// 它**不是**会话令牌，永远不会被送去 Mojang；正版验证服务器也不会接受它。
    /// </summary>
    public const string PlaceholderAccessToken = "0";

    /// <summary>派生标识使用的 UUID 版本位。官方发放的是 v4，据此可区分。</summary>
    public const int DerivedUuidVersion = 3;

    public const int MaxNameLength = 16;

    private static readonly IReadOnlyList<string> Notices = new[]
    {
        "离线账户仅在本机有效，无法进入正版验证（online-mode）服务器。",
        "离线账户不带访问令牌，因此进入不了需要正版验证的服务器与 Realms。",
        "该身份标识由本机按固定规则生成，仅作本地标识，与 Mojang 官方发放的账号标识无关。",
    };

    /// <summary>面向用户的强制告知。界面必须展示，且不可默认跳过。</summary>
    public static IReadOnlyList<string> CapabilityNotices => Notices;

    public static bool IsValidUserName(string? userName, out string reason)
    {
        if (string.IsNullOrWhiteSpace(userName))
        {
            reason = "offline name is empty";
            return false;
        }

        string trimmed = userName!.Trim();

        if (trimmed.Length > MaxNameLength)
        {
            reason = "offline name is longer than " + MaxNameLength.ToString(CultureInfo.InvariantCulture) + " characters";
            return false;
        }

        for (int i = 0; i < trimmed.Length; i++)
        {
            char c = trimmed[i];
            bool allowed = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_';
            if (!allowed)
            {
                reason = "offline name contains a character outside [A-Za-z0-9_]";
                return false;
            }
        }

        reason = string.Empty;
        return true;
    }

    /// <summary>
    /// 构造离线身份。名字不合法时抛 <see cref="ErrorCode.AuthOfflineNameInvalid"/>，
    /// 而不是悄悄替换成一个能用的名字——那样用户会以另一个身份进游戏却毫不知情。
    /// </summary>
    public static PlayerIdentity Create(string userName)
    {
        string trimmed = (userName ?? string.Empty).Trim();

        if (!IsValidUserName(trimmed, out string reason))
        {
            throw new LauncherException(ErrorCode.AuthOfflineNameInvalid, reason);
        }

        return new PlayerIdentity(IdentitySource.Offline, trimmed, ComputeUuid(trimmed))
        {
            AccessToken = PlaceholderAccessToken,
            UserType = "legacy",
            IsOnlineVerified = false,
            CapabilityNotices = Notices,
        };
    }

    /// <summary>按固定规则派生本地标识，同一名字恒得同一个值。</summary>
    public static string ComputeUuid(string userName)
    {
        string seed = NamespacePrefix + (userName ?? string.Empty);

        byte[] hash;
        using (MD5 md5 = MD5.Create())
        {
            hash = md5.ComputeHash(Encoding.UTF8.GetBytes(seed));
        }

        // UUID v3 的版本位与 RFC 4122 变体位。
        hash[6] = (byte)((hash[6] & 0x0F) | 0x30);
        hash[8] = (byte)((hash[8] & 0x3F) | 0x80);

        StringBuilder builder = new StringBuilder(32);
        for (int i = 0; i < 16; i++)
        {
            builder.Append(hash[i].ToString("x2", CultureInfo.InvariantCulture));
        }

        return builder.ToString();
    }

    /// <summary>
    /// 判断一个标识是否由本规则派生（版本位为 3）。
    /// 这是"与官方标识可区分"这条要求的可执行形式——官方发放的是 v4，绝不会被判为 true。
    /// </summary>
    public static bool IsDerivedUuid(string? uuid)
    {
        if (string.IsNullOrEmpty(uuid) || uuid!.Length != 32)
        {
            return false;
        }

        return uuid[12] == '3';
    }
}
