using System.Collections.Generic;
using Qul.Domain.Diagnostics;

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
