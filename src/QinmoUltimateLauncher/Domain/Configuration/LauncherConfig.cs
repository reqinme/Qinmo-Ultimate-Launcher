using System.Collections.Generic;
using Qul.Domain.Diagnostics;

using Qul.Domain.Identity;

namespace Qul.Domain.Configuration;

public enum IdentitySource
{
    /// <summary>离线账户。默认身份来源；必须在启动前强制告知其无法进入正版验证服务器。</summary>
    Offline = 0,

    Microsoft = 1,

    /// <summary>第三方验证。P10 能力，默认关闭、需用户显式启用。</summary>
    ThirdParty = 2,
}

public enum JavaSelectionMode
{
    Auto = 0,
    Manual = 1,
}

public enum ProxyMode
{
    System = 0,
    Manual = 1,
    Direct = 2,
}

public enum ContentSourceKind
{
    /// <summary>官方源。MVP 阶段唯一允许的取值。</summary>
    Official = 0,

    /// <summary>镜像源。P10 能力，默认关闭、需用户显式启用。</summary>
    Mirror = 1,
}

public sealed class IdentitySettings
{
    public IdentitySource Source { get; set; } = IdentitySource.Offline;

    /// <summary>离线账户显示名。仅本地标识，不冒充官方 UUID、不伪造官方会话。</summary>
    public string? OfflineUserName { get; set; }

    /// <summary>上次使用账户的非秘密标识键。不是令牌，也不是 UUID。</summary>
    public string? LastAccountKey { get; set; }

    public MicrosoftAuthSettings Microsoft { get; set; } = new MicrosoftAuthSettings();
}

/// <summary>
/// 微软正版登录的前置条件（决策 11 的 C1–C4）。
///
/// 前三项有**客观依据**：应用已在微软平台注册、client_id 与权限范围已定、流程选了设备码。
/// 这些在应用注册完成时即可置真。
///
/// **C4（第三方启动器相关条款核对）不是代码能推断的事实**，只能由产品负责人核对后确认，
/// 所以它默认 false —— 门禁会如实显示"C4 尚未核对"，而不是替用户签字。
/// </summary>
public sealed class MicrosoftAuthSettings
{
    /// <summary>C1：已在微软平台完成应用注册。</summary>
    public bool ApplicationRegistered { get; set; } = true;

    /// <summary>
    /// C2：client_id。
    ///
    /// **它不是秘密，也不该被当成秘密。** 桌面应用无法保守机密（客户端凭据才是秘密，
    /// 而公共客户端根本不该有凭据），所以它随程序分发。
    /// 默认值是本项目的正式应用注册：Qinmo_Ultimate_Launcher，受支持的帐户类型为"所有 Microsoft 帐户用户"。
    /// </summary>
    public string? ClientId { get; set; } = "2a06b5bc-7b61-4a36-9edf-52fb89525943";

    /// <summary>C2：所需权限范围已确认（<c>XboxLive.signin offline_access</c>，均为首方范围，无需在门户另配权限）。</summary>
    public bool ScopesConfirmed { get; set; } = true;

    /// <summary>C3：流程已定——设备码流程，因此**不需要**重定向 URI。</summary>
    public bool FlowDecided { get; set; } = true;

    /// <summary>C4：第三方启动器相关条款已核对。**默认 false，需产品负责人显式置真。**</summary>
    public bool ThirdPartyTermsChecked { get; set; }

    public MicrosoftAuthPrerequisites ToPrerequisites()
    {
        return new MicrosoftAuthPrerequisites
        {
            ApplicationRegistered = ApplicationRegistered,
            ClientId = ClientId,
            ScopesConfirmed = ScopesConfirmed,
            FlowDecided = FlowDecided,
            ThirdPartyTermsChecked = ThirdPartyTermsChecked,
        };
    }
}

public sealed class JavaSettings
{
    public JavaSelectionMode Mode { get; set; } = JavaSelectionMode.Auto;

    /// <summary>手动指定的 Java 可执行路径；仅 Manual 模式生效。</summary>
    public string? ManualPath { get; set; }
}

public sealed class MemorySettings
{
    /// <summary>为空表示使用安全默认值，不由启动器猜测用户机器上限。</summary>
    public int? MaxMb { get; set; }
}

public sealed class LaunchSettings
{
    public string? GameDirectory { get; set; }

    public List<string> ExtraJvmArgs { get; set; } = new List<string>();

    /// <summary>用户显式配置的服务器地址。只在该值存在时才触发条件式服务器预检。</summary>
    public string? ServerQuickConnect { get; set; }

    public List<string> RecentServers { get; set; } = new List<string>();
}

public sealed class NetworkSettings
{
    public ProxyMode ProxyMode { get; set; } = ProxyMode.System;

    public string? ProxyAddress { get; set; }

    /// <summary>
    /// 恒为 false，且不提供开启入口：证书异常必须明确提示，绝不静默绕过。
    /// 保留该成员是为了把"不可绕过"写成显式契约，而不是一句口头约定。
    /// </summary>
    public bool AllowInvalidCertificate => false;

    /// <summary>
    /// 同时进行的下载条数。
    ///
    /// 1.7.10（分段之前，前后各插基准校准漂移）：
    ///   并发 8  → 307 KB/s
    ///   并发 32 → 884 KB/s   （2.88 倍）
    ///   并发 64 → 1016 KB/s
    ///
    /// 26.3（分段之后，5224 项 / 586 MB，三明治式对照）：
    ///   并发 32 → 1667、1760 KB/s（两次，稳定在 ±5%）
    ///   并发 64 → 2215 KB/s（夹在两次 32 之间测的）
    ///
    /// 默认定在 64，与 PCL2 的 `ToolDownloadThread`（63 + 1）一致。
    /// **26.3 那次对照是三明治式的**：64 夹在两次 32 之间，
    /// 所以这个 1.3 倍不能归因于网络漂移——那正是我先前栽过的坑。
    /// </summary>
    public int MaxConcurrency { get; set; } = 64;
}

public sealed class ContentSourceSettings
{
    public ContentSourceKind Kind { get; set; } = ContentSourceKind.Official;

    /// <summary>P10 字段。MVP 阶段必须为 null。</summary>
    public string? MirrorBaseUrl { get; set; }
}

public sealed class JavaRuntimeSettings
{
    /// <summary>P10 能力：自动下载 JRE。默认关闭。</summary>
    public bool AutoDownload { get; set; }
}

public sealed class DiagnosticsSettings
{
    public LogLevel LogLevel { get; set; } = LogLevel.Info;
}

/// <summary>
/// 用户配置。只存用户意图，不存运行状态（状态放 data/state/）。
/// 硬约束：本类型中永远不得出现任何凭据字段——令牌一律走 DPAPI 密文，独立存放。
/// </summary>
public sealed class LauncherConfig
{
    public const int CurrentSchemaVersion = 1;

    public int SchemaVersion { get; set; } = CurrentSchemaVersion;

    public IdentitySettings Identity { get; set; } = new IdentitySettings();

    public JavaSettings Java { get; set; } = new JavaSettings();

    public MemorySettings Memory { get; set; } = new MemorySettings();

    public LaunchSettings Launch { get; set; } = new LaunchSettings();

    public NetworkSettings Network { get; set; } = new NetworkSettings();

    public ContentSourceSettings ContentSource { get; set; } = new ContentSourceSettings();

    public JavaRuntimeSettings JavaRuntime { get; set; } = new JavaRuntimeSettings();

    public DiagnosticsSettings Diagnostics { get; set; } = new DiagnosticsSettings();

    public static LauncherConfig CreateDefault()
    {
        return new LauncherConfig();
    }
}
