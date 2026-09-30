using System;
using System.Collections.Generic;
using System.Threading;
using Qul.Domain.Runtime;

namespace Qul.Application.Ports;

/// <summary>
/// 运行时来源的能力声明。
/// 刻意做成"声明"而不是"假设"：未来的自动下载实现没有枚举能力、却可能需要展示许可文本，
/// 调用方应当按能力决定界面与流程，而不是按实现类型写 if。
/// </summary>
public sealed class JavaRuntimeCapabilities
{
    /// <summary>能否在本机枚举出运行时。</summary>
    public bool CanEnumerate { get; set; }

    /// <summary>能否自行获取运行时（自动下载，P10）。MVP 恒为 false。</summary>
    public bool CanFetch { get; set; }

    /// <summary>获取运行时是否需要向用户展示许可文本（JRE 再分发许可）。</summary>
    public bool RequiresLicenseNotice { get; set; }

    /// <summary>是否支持卸载由该来源获取的运行时。</summary>
    public bool CanUninstall { get; set; }

    /// <summary>是否默认启用。可选能力一律默认关闭。</summary>
    public bool EnabledByDefault { get; set; }
}

/// <summary>
/// Java 运行时来源。
///
/// MVP 有**两个实现**（本机探测、手动指定），因此这个抽象现在就有存在理由；
/// 第三个实现（自动下载）在 P10，且默认关闭。
/// </summary>
public interface IJavaRuntimeProvider
{
    /// <summary>用于日志与界面的稳定名称。</summary>
    string Name { get; }

    JavaRuntimeCapabilities Capabilities { get; }

    /// <summary>枚举该来源当前可用的运行时。取不到任何运行时返回空列表，不抛异常。</summary>
    IReadOnlyList<JavaRuntimeCandidate> Discover(CancellationToken cancellationToken);
}
