using System;
using System.Collections.Generic;
using Qul.Domain.Diagnostics;

namespace Qul.Domain.Downloads;

public enum DownloadItemKind
{
    VersionManifest,
    VersionDetail,
    ClientJar,
    Library,
    LibraryClassifier,
    AssetIndex,
    AssetObject,
    LoggingConfig,
}

/// <summary>
/// 一个待取回的资源。
///
/// **校验基准只能来自官方元数据。** 本类型刻意不提供任何"由内容来源提供哈希"的入口——
/// 不是靠约定，而是靠结构：镜像之类的来源即便被接入，也无处安放它自己的哈希。
/// </summary>
public sealed class DownloadItem
{
    public DownloadItemKind Kind { get; set; }

    public string Url { get; set; } = string.Empty;

    /// <summary>
    /// 备用下载源，按优先级排列。
    /// 一个源连续失败时换下一个——镜像抽风或官方被限速都不至于卡死整个安装。
    /// **摘要不随源改变**：它永远来自官方元数据，所以换源不降低完整性保证。
    /// </summary>
    public IReadOnlyList<string> FallbackUrls { get; set; } = Array.Empty<string>();

    /// <summary>十六进制 SHA-1。缺失该值时必须拒绝下载，而不是"先下下来再说"。</summary>
    public string? Sha1 { get; set; }

    public long? Size { get; set; }

    /// <summary>相对缓存根的存放路径。绝对路径一律不参与计划。</summary>
    public string RelativePath { get; set; } = string.Empty;

    /// <summary>可选资源：取不到不阻断整体，但必须出现在结果里。</summary>
    public bool IsOptional { get; set; }

    public string Describe()
    {
        return Kind + " " + RelativePath;
    }
}

public sealed class DownloadPlan
{
    public string Name { get; set; } = string.Empty;

    public IReadOnlyList<DownloadItem> Items { get; set; } = Array.Empty<DownloadItem>();

    /// <summary>已知体积之和。含未知体积的项时为下界，仅用于磁盘预检与进度估算。</summary>
    public long KnownTotalBytes
    {
        get
        {
            long total = 0;
            for (int i = 0; i < Items.Count; i++)
            {
                long? size = Items[i].Size;
                if (size.HasValue && size.Value > 0)
                {
                    total += size.Value;
                }
            }

            return total;
        }
    }

    /// <summary>
    /// 构建期发现的重复相对路径。
    /// 重复本身是**合法且常见**的：资源对象是内容寻址的（不同逻辑名可能同哈希），
    /// 官方元数据里也存在指向同一 artifact 的重复库条目。
    /// 构建器按"首次出现优先"折叠它们，并在此如实记录——折叠是为了不重复下载，记录是为了不把事实藏起来。
    /// </summary>
    public IReadOnlyList<string> DuplicatePaths { get; set; } = Array.Empty<string>();

    /// <summary>
    /// **内容不一致**的重复路径：同一条相对路径却对应不同的 URL 或不同的 SHA-1。
    /// 这才是真正的危险——并发下载会互相覆盖，最终文件是哪一个完全看运气。
    /// 真实元数据上必须恒为空，由测试守住。
    /// </summary>
    public IReadOnlyList<string> ConflictingPaths { get; set; } = Array.Empty<string>();
}

public enum DownloadItemState
{
    /// <summary>本地已存在且校验通过，本次未产生任何网络请求。</summary>
    Present,

    Downloaded,

    Failed,

    /// <summary>元数据未提供校验值，已拒绝下载（QUL-DL-0003）。</summary>
    RefusedNoChecksum,
}

public sealed class DownloadItemReport
{
    public DownloadItemReport(DownloadItem item, DownloadItemState state, ErrorCode? error, int attempts, long bytesTransferred)
    {
        Item = item;
        State = state;
        Error = error;
        Attempts = attempts;
        BytesTransferred = bytesTransferred;
    }

    public DownloadItem Item { get; }

    public DownloadItemState State { get; }

    public ErrorCode? Error { get; }

    public int Attempts { get; }

    public long BytesTransferred { get; }
}

public sealed class DownloadReport
{
    public DownloadReport(IReadOnlyList<DownloadItemReport> items)
    {
        Items = items;

        List<DownloadItemReport> failed = new List<DownloadItemReport>();
        long bytes = 0;
        int present = 0;
        int downloaded = 0;

        for (int i = 0; i < items.Count; i++)
        {
            DownloadItemReport report = items[i];
            bytes += report.BytesTransferred;

            switch (report.State)
            {
                case DownloadItemState.Present:
                    present++;
                    break;
                case DownloadItemState.Downloaded:
                    downloaded++;
                    break;
                default:
                    failed.Add(report);
                    break;
            }
        }

        Failures = failed;
        BytesTransferred = bytes;
        PresentCount = present;
        DownloadedCount = downloaded;
    }

    public IReadOnlyList<DownloadItemReport> Items { get; }

    public IReadOnlyList<DownloadItemReport> Failures { get; }

    public long BytesTransferred { get; }

    public int PresentCount { get; }

    public int DownloadedCount { get; }

    /// <summary>是否存在阻断性失败：可选资源的失败不算阻断。</summary>
    public bool IsComplete
    {
        get
        {
            for (int i = 0; i < Failures.Count; i++)
            {
                if (!Failures[i].Item.IsOptional)
                {
                    return false;
                }
            }

            return true;
        }
    }

    /// <summary>是否存在任何失败（含可选资源）。</summary>
    public bool HasAnyFailure => Failures.Count > 0;
}

public sealed class DownloadProgress
{
    public int FilesCompleted { get; set; }

    public int FilesTotal { get; set; }

    public long BytesTransferred { get; set; }

    public long? KnownTotalBytes { get; set; }

    public string CurrentPath { get; set; } = string.Empty;
}
