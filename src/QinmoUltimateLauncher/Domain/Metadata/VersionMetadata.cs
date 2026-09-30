using System;
using System.Collections.Generic;

namespace Qul.Domain.Metadata;

public enum VersionType
{
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
    Unknown,
}

/// <summary>一处可下载资源。sha1 与 size 允许缺失——元数据里确实存在没有它们的情况。</summary>
public sealed class DownloadRef
{
    public DownloadRef(string url, string? sha1 = null, long? size = null, string? path = null)
    {
        Url = url;
        Sha1 = sha1;
        Size = size;
        Path = path;
    }

    public string Url { get; }

    public string? Sha1 { get; }

    public long? Size { get; }

    /// <summary>相对缓存根的存放路径。缺失时由库坐标推导。</summary>
    public string? Path { get; }
}

public sealed class AssetIndexRef
{
    public string Id { get; set; } = string.Empty;

    public string? Sha1 { get; set; }

    public long? Size { get; set; }

    public long? TotalSize { get; set; }

    public string? Url { get; set; }
}

public sealed class LoggingConfig
{
    /// <summary>客户端日志配置对应的 JVM 参数模板，形如 -Dlog4j.configurationFile=${path}。P4 组装 JVM 参数时使用。</summary>
    public string? Argument { get; set; }

    public DownloadRef? File { get; set; }
}

public sealed class JavaVersionRequirement
{
    public int MajorVersion { get; set; }

    public string? Component { get; set; }
}

public sealed class VersionSummary
{
    public string Id { get; set; } = string.Empty;

    public VersionType Type { get; set; } = VersionType.Unknown;

    public string Url { get; set; } = string.Empty;

    public string? Sha1 { get; set; }

    public long? Size { get; set; }

    public string? ReleaseTime { get; set; }

    public int? ComplianceLevel { get; set; }
}

public sealed class VersionManifest
{
    public string LatestRelease { get; set; } = string.Empty;

    public string LatestSnapshot { get; set; } = string.Empty;

    public IReadOnlyList<VersionSummary> Versions { get; set; } = Array.Empty<VersionSummary>();

    public VersionSummary? Find(string id)
    {
        foreach (VersionSummary summary in Versions)
        {
            if (string.Equals(summary.Id, id, StringComparison.Ordinal))
            {
                return summary;
            }
        }

        return null;
    }
}

/// <summary>
/// 单个版本的元数据。
/// 注意：这里保存的是"未继承合并"的原始形态——inherit 合并由 <c>VersionResolver</c> 负责，
/// 解析阶段绝不做隐式合并，否则排障时分不清某个字段到底来自子版本还是父版本。
/// </summary>
public sealed class VersionDetail
{
    public string Id { get; set; } = string.Empty;

    public VersionType Type { get; set; } = VersionType.Unknown;

    public string? InheritsFrom { get; set; }

    public string? MainClass { get; set; }

    public string? Assets { get; set; }

    public AssetIndexRef? AssetIndex { get; set; }

    public DownloadRef? ClientDownload { get; set; }

    public JavaVersionRequirement? JavaVersion { get; set; }

    public IReadOnlyList<LibraryRef> Libraries { get; set; } = Array.Empty<LibraryRef>();

    public IReadOnlyList<ArgumentEntry> GameArguments { get; set; } = Array.Empty<ArgumentEntry>();

    public IReadOnlyList<ArgumentEntry> JvmArguments { get; set; } = Array.Empty<ArgumentEntry>();

    /// <summary>
    /// 新式元数据里出现的一个额外参数组（26.x 起）。
    /// P1 只负责原样解析，不在这一层决定它怎么用。
    /// </summary>
    public IReadOnlyList<ArgumentEntry> DefaultUserJvmArguments { get; set; } = Array.Empty<ArgumentEntry>();

    /// <summary>旧式（1.7–1.12）的单个命令行字符串，与 GameArguments 二选一。</summary>
    public string? MinecraftArguments { get; set; }

    public LoggingConfig? Logging { get; set; }

    public int? MinimumLauncherVersion { get; set; }

    /// <summary>该版本是否使用旧式参数写法。</summary>
    public bool UsesLegacyArguments => !string.IsNullOrEmpty(MinecraftArguments);
}
