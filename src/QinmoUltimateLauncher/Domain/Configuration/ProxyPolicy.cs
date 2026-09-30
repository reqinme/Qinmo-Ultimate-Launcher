using System;

namespace Qul.Domain.Configuration;

/// <summary>
/// 代理三态的换算。
///
/// 配置里有三种选择（跟随系统 / 手动 / 直连），而传输层需要的是
/// **另一种三态字符串**：<c>null</c> = 跟随系统，空串 = 直连，其他 = 显式地址。
///
/// **为什么必须区分 <c>null</c> 与空串**：只有区分开，"关掉代理"这个用户意图才表达得出来。
/// 早先这一段换算写死在下载选项里，而那个选项**从没有人从配置赋值**——
/// 于是"手动代理"与"直连"两种模式静默失效，连"代理地址非法"的错误码都不可达。
///
/// 把它做成纯函数是为了能直接测：三种模式的映射不依赖网络，
/// 而依赖网络的测法必然会变成"不稳定所以被忽略"。
/// </summary>
public static class ProxyPolicy
{
    /// <summary>配置里的代理选择 → 传输层认识的代理串。</summary>
    public static string? Describe(ProxyMode mode, string? address)
    {
        switch (mode)
        {
            case ProxyMode.Direct:
                return string.Empty;

            case ProxyMode.Manual:
                // 选了手动却没填地址：**当作直连**，而不是静默回落到系统代理。
                // 用户选手动通常正是因为不想走系统代理；回落到系统代理
                // 会再造出一次"我明明设了代理却没生效"的困惑。
                return string.IsNullOrWhiteSpace(address) ? string.Empty : address!.Trim();

            default:
                return null;
        }
    }

    /// <summary>供界面与诊断报告使用的一句话说明。</summary>
    public static string DescribeForHumans(ProxyMode mode, string? address)
    {
        switch (mode)
        {
            case ProxyMode.Direct:
                return "直连（不经过任何代理）";

            case ProxyMode.Manual:
                return string.IsNullOrWhiteSpace(address)
                    ? "手动代理（未填地址，按直连处理）"
                    : "手动代理 " + address!.Trim();

            default:
                return "跟随系统代理设置";
        }
    }
}
