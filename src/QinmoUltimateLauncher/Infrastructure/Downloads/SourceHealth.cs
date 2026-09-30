using System;
using System.Collections.Concurrent;
using Qul.Domain.Downloads;

namespace Qul.Infrastructure.Downloads;

/// <summary>
/// 单次运行内的下载源健康度：按 **主机 + 条目类型** 记录失败，
/// 超过阈值就把这个组合在本轮跳过。
///
/// 存在的理由是一次实测：探测显示镜像对资源对象快得多（1441 vs 452 KB/s），
/// 于是它被排到首位；但镜像**缺很多库文件**，每个库都要先失败一次再换源。
/// 结果整次安装的平均吞吐只有 142 KB/s，而同一窗口内可达 884 KB/s——
/// **时间花在了一次次注定失败的尝试上。**
///
/// 键里带条目类型是刻意的：同一台镜像对资源对象齐全、对库却常缺，
/// 只按主机计数会把它的资源提速一起误伤掉。
///
/// PCL2 是同一个思路（按来源记录失败、超阈值禁用，见 `ModNet.vb:1084`）。
/// </summary>
internal sealed class SourceHealth
{
    private readonly int _disableAfter;
    private readonly ConcurrentDictionary<string, int> _failures =
        new ConcurrentDictionary<string, int>(StringComparer.OrdinalIgnoreCase);

    public SourceHealth(int disableAfter)
    {
        _disableAfter = Math.Max(1, disableAfter);
    }

    /// <summary>这个源在**这一类条目**上是否已经被判定不可用。</summary>
    public bool IsUnhealthy(string url, DownloadItemKind kind)
    {
        int count;
        return _failures.TryGetValue(Key(url, kind), out count) && count >= _disableAfter;
    }

    /// <summary>
    /// 记一次**归因于来源本身**的失败。
    ///
    /// 算进来的有**两类**（与调用点保持一致，先前这里的注释只写了第一类，属于文档与代码不符）：
    ///
    /// 1. <c>NetResourceMissing</c> —— "这个源没有这个文件"。这是最典型的用法：
    ///    镜像缺库文件时，靠它把镜像在**库**这一类上禁掉，而不牵连它的资源对象。
    /// 2. <c>NetHttpStatus</c> —— 4xx/5xx。**这一类是刻意计入的**：
    ///    镜像对并发敏感，实测会对库文件回 403（限流）；不计数的话
    ///    每个文件都要先去撞一次 403 才换源，那正是"时间花在注定失败的尝试上"。
    ///
    /// **不算进来的**：超时与连接中断。那属于传输故障，算在来源头上会误伤好源。
    /// </summary>
    public void RecordFailure(string url, DownloadItemKind kind)
    {
        _failures.AddOrUpdate(Key(url, kind), 1, (_, current) => current + 1);
    }

    private static string Key(string url, DownloadItemKind kind)
    {
        string host = Uri.TryCreate(url, UriKind.Absolute, out Uri? uri) ? uri.Host : url;
        return host + "|" + kind;
    }
}
