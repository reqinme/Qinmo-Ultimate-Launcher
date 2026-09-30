using System;
using System.Collections.Generic;

namespace Qul.Domain.Identity;

/// <summary>微软 OAuth 与 Xbox / Minecraft 服务链路用到的公开端点。</summary>
public static class MicrosoftAuthEndpoints
{
    /// <summary>
    /// 租户段固定为 <c>common</c>。
    ///
    /// 本项目应用注册的"受支持的帐户类型"是**所有 Microsoft 帐户用户**，
    /// 对应的正是 <c>common</c>：同时覆盖个人账户与工作/学校账户。
    ///
    /// 用 <c>consumers</c> 会只认个人账户，工作/学校账户会被直接拒掉；
    /// 用 <c>organizations</c> 则相反。两者都比注册声明的范围窄。
    /// 没有 Xbox 档案的账户会在后面的 Xbox 步骤得到明确说明，
    /// 所以放宽入口不会把问题藏起来，只会让更多账户有机会走完。
    /// </summary>
    private const string Tenant = "common";

    public static readonly string DeviceCode = "https://login.microsoftonline.com/" + Tenant + "/oauth2/v2.0/devicecode";
    public static readonly string Token = "https://login.microsoftonline.com/" + Tenant + "/oauth2/v2.0/token";
    public static readonly string Authorize = "https://login.microsoftonline.com/" + Tenant + "/oauth2/v2.0/authorize";

    public const string XboxUserAuthenticate = "https://user.auth.xboxlive.com/user/authenticate";
    public const string XboxXstsAuthorize = "https://xsts.auth.xboxlive.com/xsts/authorize";

    public const string MinecraftLoginWithXbox = "https://api.minecraftservices.com/authentication/login_with_xbox";
    public const string MinecraftProfile = "https://api.minecraftservices.com/minecraft/profile";
    public const string MinecraftEntitlements = "https://api.minecraftservices.com/entitlements/mcstore";

    /// <summary>默认权限范围。<c>offline_access</c> 是拿到刷新令牌的前提。</summary>
    public const string DefaultScopes = "XboxLive.signin offline_access";
}

/// <summary>
/// P5 的前置条件门禁（决策 11）。
///
/// 四项都是**外部事实**：应用有没有注册、权限范围定没定、走哪条流程、第三方启动器相关条款核没核对。
/// 本程序无从验证它们，只能由产品负责人逐项确认后置真。
///
/// **任一未满足，微软正版登录不得进入可用状态**——它必须报 QUL-AUTH-0005 并说明缺什么，
/// 而不是"尝试一下看看行不行"。拿未注册的 client_id 去撞授权端点，得到的是一个
/// 让人查半天的错误，而真正的原因是前置条件根本没满足。
/// </summary>
public sealed class MicrosoftAuthPrerequisites
{
    /// <summary>C1：已在微软平台完成应用注册。</summary>
    public bool ApplicationRegistered { get; set; }

    /// <summary>C2：client_id 已确定。</summary>
    public string? ClientId { get; set; }

    /// <summary>C2：所需权限范围已确认。</summary>
    public bool ScopesConfirmed { get; set; }

    /// <summary>C3：回调 URI 或设备码流程已确定。</summary>
    public bool FlowDecided { get; set; }

    /// <summary>C4：第三方启动器相关条款已核对。</summary>
    public bool ThirdPartyTermsChecked { get; set; }

    public IReadOnlyList<string> Unmet
    {
        get
        {
            List<string> unmet = new List<string>();

            if (!ApplicationRegistered)
            {
                unmet.Add("C1 尚未在微软平台完成应用注册");
            }

            if (string.IsNullOrWhiteSpace(ClientId))
            {
                unmet.Add("C2 缺少 client_id");
            }

            if (!ScopesConfirmed)
            {
                unmet.Add("C2 所需权限范围尚未确认");
            }

            if (!FlowDecided)
            {
                unmet.Add("C3 回调 URI 与设备码流程尚未确定");
            }

            if (!ThirdPartyTermsChecked)
            {
                unmet.Add("C4 第三方启动器相关条款尚未核对");
            }

            return unmet;
        }
    }

    public bool IsSatisfied => Unmet.Count == 0;

    /// <summary>供界面显示的一句话说明；已满足时为空。</summary>
    public string Describe()
    {
        IReadOnlyList<string> unmet = Unmet;
        if (unmet.Count == 0)
        {
            return string.Empty;
        }

        return "微软正版登录当前不可用：" + string.Join("；", unmet) + "。";
    }
}
