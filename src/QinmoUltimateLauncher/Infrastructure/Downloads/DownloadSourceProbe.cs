using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Threading;
using Qul.Application.Ports;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Infrastructure.Diagnostics;

namespace Qul.Infrastructure.Downloads;

/// <summary>
/// 探测各下载源的实际吞吐，按快的在前排序。
///
/// **为什么需要它**：只按"失败"换源是不够的。
/// 一个"慢但能通"的官方源永远不会触发切换——几千个文件就那么一个个慢慢磨完。
/// 而"官方慢"恰恰是最常见的场景，也是引入镜像的初衷。
///
/// 做法很朴素：拿计划里**最小的一个条目**做样本，逐个候选源各取一次并计时，
/// 按实测速度排序。样本小则代价可忽略，而它决定的是后面几千个文件走哪条路。
///
/// 探测失败、样本过大、或只有一个候选源时，**一律返回原始顺序**——
/// 探测是优化，不该成为新的故障点。
/// </summary>
public sealed class DownloadSourceProbe
{
    /// <summary>样本上限。超过这个大小就不值得为了排序去多下一遍。</summary>
    private const long MaxSampleBytes = 2L * 1024 * 1024;

    private readonly IHttpTransport _transport;
    private readonly SessionLog _log;

    public DownloadSourceProbe(IHttpTransport transport, SessionLog? log = null)
    {
        _transport = transport ?? throw new ArgumentNullException(nameof(transport));
        _log = log ?? SessionLog.Null;
    }

    /// <summary>
    /// 从计划里挑一个合适的探测样本：候选源最多、体积最小的那个。
    /// 没有合适样本时返回 null（调用方保持原顺序）。
    /// </summary>
    public static DownloadItem? PickSample(DownloadPlan plan)
    {
        DownloadItem? best = null;
        int bestSources = 1;

        for (int i = 0; i < plan.Items.Count; i++)
        {
            DownloadItem item = plan.Items[i];
            int sources = 1 + item.FallbackUrls.Count;

            if (sources < 2 || string.IsNullOrWhiteSpace(item.Url))
            {
                continue;
            }

            if (item.Size.HasValue && item.Size.Value > MaxSampleBytes)
            {
                continue;
            }

            if (best == null || sources > bestSources
                || (sources == bestSources && (item.Size ?? 0) < (best.Size ?? long.MaxValue)))
            {
                best = item;
                bestSources = sources;
            }
        }

        return best;
    }

    /// <summary>
    /// 按实测吞吐把候选源从快到慢排序。
    /// 任何一个源探测失败就把它排到最后；全部失败则返回原始顺序。
    /// </summary>
    public IReadOnlyList<string> Rank(
        DownloadItem sample,
        IReadOnlyList<string> candidates,
        CancellationToken cancellationToken)
    {
        if (sample == null || candidates == null || candidates.Count < 2)
        {
            return candidates ?? Array.Empty<string>();
        }

        List<KeyValuePair<string, double>> measured = new List<KeyValuePair<string, double>>(candidates.Count);
        int succeeded = 0;

        for (int i = 0; i < candidates.Count; i++)
        {
            cancellationToken.ThrowIfCancellationRequested();

            double bytesPerSecond = Measure(candidates[i], cancellationToken);

            if (bytesPerSecond > 0)
            {
                succeeded++;
            }

            measured.Add(new KeyValuePair<string, double>(candidates[i], bytesPerSecond));
        }

        if (succeeded < 2)
        {
            // 只有一个源能测出来，说明不了什么，别拿它去改全局行为。
            _log.Info("download", "source probe inconclusive; keeping the configured order");
            return candidates;
        }

        measured.Sort((left, right) => right.Value.CompareTo(left.Value));

        List<string> ordered = new List<string>(candidates.Count);
        for (int i = 0; i < measured.Count; i++)
        {
            ordered.Add(measured[i].Key);
            _log.Info("download", "source probe: " + Host(measured[i].Key) + " = " + (long)(measured[i].Value / 1024) + " KB/s");
        }

        return ordered;
    }

    /// <summary>取一次样本并返回字节/秒。失败返回 0。</summary>
    private double Measure(string url, CancellationToken cancellationToken)
    {
        Stopwatch watch = Stopwatch.StartNew();
        long bytes = 0;

        try
        {
            using (HttpFetchResponse response = _transport.Fetch(
                new HttpFetchRequest { Url = url, Timeout = TimeSpan.FromSeconds(15) },
                cancellationToken))
            {
                if (response.Status != HttpFetchStatus.Success && response.Status != HttpFetchStatus.PartialContent)
                {
                    return 0;
                }

                byte[] buffer = new byte[64 * 1024];
                int read;

                while ((read = response.Content.Read(buffer, 0, buffer.Length)) > 0)
                {
                    bytes += read;
                }
            }
        }
        catch (Exception ex) when (ex is IOException || ex is System.Net.WebException || ex is LauncherException)
        {
            return 0;
        }

        watch.Stop();

        if (bytes <= 0)
        {
            return 0;
        }

        double seconds = Math.Max(0.001, watch.Elapsed.TotalSeconds);
        return bytes / seconds;
    }

    private static string Host(string url)
    {
        return Uri.TryCreate(url, UriKind.Absolute, out Uri? uri) ? uri.Host : url;
    }
}
