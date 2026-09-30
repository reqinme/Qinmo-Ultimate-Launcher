using System;
using System.Collections.Generic;
using Qul.Domain.Configuration;

namespace Qul.Domain.Identity;

/// <summary>
/// 一次启动所用的玩家身份。
///
/// **身份来源决定能力边界**，因此这个对象必须能把自己的限制说清楚——
/// 界面拿 <see cref="CapabilityNotices"/> 做强制告知，启动参数组装拿它决定注入什么。
/// </summary>
public sealed class PlayerIdentity
{
    public PlayerIdentity(IdentitySource source, string userName, string uuid)
    {
        Source = source;
        UserName = userName ?? throw new ArgumentNullException(nameof(userName));
        Uuid = uuid ?? throw new ArgumentNullException(nameof(uuid));
    }

    public IdentitySource Source { get; }

    public string UserName { get; }

    /// <summary>32 位无连字符十六进制——这正是启动参数 --uuid 需要的形式。</summary>
    public string Uuid { get; }

    /// <summary>
    /// 访问令牌。**离线账户没有令牌，这里的值是本地占位符**，
    /// 它不会被送去 Mojang，也不代表任何官方会话。
    /// </summary>
    public string? AccessToken { get; set; }

    /// <summary>注入 --userType 的值：legacy / msa。</summary>
    public string UserType { get; set; } = "legacy";

    /// <summary>是否经过在线验证。离线账户恒为 false。</summary>
    public bool IsOnlineVerified { get; set; }

    /// <summary>面向用户的能力限制说明。离线来源必须非空，且界面不可默认跳过。</summary>
    public IReadOnlyList<string> CapabilityNotices { get; set; } = Array.Empty<string>();

    /// <summary>带连字符的标准写法，仅用于展示与排障。</summary>
    public string UuidWithDashes
    {
        get
        {
            if (Uuid.Length != 32)
            {
                return Uuid;
            }

            return Uuid.Substring(0, 8) + "-" + Uuid.Substring(8, 4) + "-" + Uuid.Substring(12, 4)
                   + "-" + Uuid.Substring(16, 4) + "-" + Uuid.Substring(20, 12);
        }
    }

    public string Describe()
    {
        return Source + " / " + UserName + " / " + UuidWithDashes;
    }
}
