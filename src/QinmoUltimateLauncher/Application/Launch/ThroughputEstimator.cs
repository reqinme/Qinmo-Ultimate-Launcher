using System;
using System.Collections.Generic;
using System.Text;

namespace Qul.Application.Launch;

/// <summary>
/// 把"累计已传字节"的时间序列，变成吞吐速率与剩余时间。
///
/// 需要它是因为冷装一个 1.7.10 可能要走十几分钟，
/// 而界面上只有一行"开始获取所需文件…"——**用户分不清"慢但在推进"和"卡住了"**。
/// 这不是带宽问题，是产品问题。
///
/// 两条刻意的取舍：
///   1. 速率取**窗口内**的平均，不是相邻两次采样的瞬时值。
///      瞬时值会随每次回调剧烈跳动，看起来像坏了。
///   2. 算不出速率或不知道总量时，<see cref="Remaining"/> 返回 null。
///      **宁可说"剩余时间未知"，也不要编一个数出来。**
/// </summary>
public sealed class ThroughputEstimator
{
    private readonly TimeSpan _window;
    private readonly int _capacity;
    private readonly List<Reading> _samples = new List<Reading>();

    private long _lastBytes;
    private DateTimeOffset _lastGrowth;

    public ThroughputEstimator(TimeSpan window, int capacity = 64)
    {
        if (window <= TimeSpan.Zero)
        {
            throw new ArgumentOutOfRangeException(nameof(window));
        }

        if (capacity < 2)
        {
            throw new ArgumentOutOfRangeException(nameof(capacity));
        }

        _window = window;
        _capacity = capacity;
    }

    /// <summary>窗口内的平均速率（字节/秒）。样本不足时为 0。</summary>
    public double BytesPerSecond { get; private set; }

    /// <summary>距离上次**字节增长**过去了多久。字节不动时它会一直变大——这正是"卡住"的信号。</summary>
    public TimeSpan SinceProgress { get; private set; }

    /// <summary>预估剩余时间。无速率或无总量时为 null。</summary>
    public TimeSpan? Remaining { get; private set; }

    /// <summary>是否已有可用的速率。</summary>
    public bool HasRate => BytesPerSecond > 0;

    /// <summary>
    /// 采一次样。
    ///
    /// 只有字节数**变大**才重置"停滞"计时；重复上报同一个数字（引擎在重试、在换源）
    /// 恰恰是应当被看成停滞的情形。
    /// </summary>
    public void Sample(long bytesCompleted, long bytesTotal, DateTimeOffset now)
    {
        if (bytesCompleted > _lastBytes)
        {
            _lastBytes = bytesCompleted;
            _lastGrowth = now;
        }
        else if (_lastGrowth == default(DateTimeOffset))
        {
            _lastGrowth = now;
        }

        SinceProgress = now - _lastGrowth;

        _samples.Add(new Reading(now, bytesCompleted));

        // 丢掉窗口之外的，以及超出容量的最老样本
        while (_samples.Count > 2 && now - _samples[0].At > _window)
        {
            _samples.RemoveAt(0);
        }

        while (_samples.Count > _capacity)
        {
            _samples.RemoveAt(0);
        }

        Recompute(bytesTotal);
    }

    /// <summary>阶段切换时调用：下载的速率不该被拿去算解压的剩余时间。</summary>
    public void Reset()
    {
        _samples.Clear();
        _lastBytes = 0;
        _lastGrowth = default(DateTimeOffset);
        BytesPerSecond = 0;
        Remaining = null;
        SinceProgress = TimeSpan.Zero;
    }

    /// <summary>
    /// 给界面用的一行说明。没有任何可说的内容时返回空串，让界面自己去决定显示什么。
    /// </summary>
    public string Describe()
    {
        if (!HasRate)
        {
            return string.Empty;
        }

        StringBuilder text = new StringBuilder();
        text.Append(FormatRate(BytesPerSecond));

        if (Remaining.HasValue)
        {
            text.Append(" · 剩余约 ").Append(FormatDuration(Remaining.Value));
        }
        else
        {
            // 没有总量时不该装作知道
            text.Append(" · 剩余时间未知");
        }

        return text.ToString();
    }

    public static string FormatRate(double bytesPerSecond)
    {
        if (bytesPerSecond >= 1024 * 1024)
        {
            return (bytesPerSecond / (1024 * 1024)).ToString("0.0") + " MB/s";
        }

        if (bytesPerSecond >= 1024)
        {
            return (bytesPerSecond / 1024).ToString("0") + " KB/s";
        }

        return Math.Max(0, (long)bytesPerSecond) + " B/s";
    }

    public static string FormatDuration(TimeSpan value)
    {
        if (value.TotalSeconds < 1)
        {
            return "不到 1 秒";
        }

        if (value.TotalHours >= 1)
        {
            return (int)value.TotalHours + " 小时 " + value.Minutes + " 分";
        }

        if (value.TotalMinutes >= 1)
        {
            return (int)value.TotalMinutes + " 分 " + value.Seconds + " 秒";
        }

        return (int)value.TotalSeconds + " 秒";
    }

    private void Recompute(long bytesTotal)
    {
        if (_samples.Count < 2)
        {
            BytesPerSecond = 0;
            Remaining = null;
            return;
        }

        Reading first = _samples[0];
        Reading last = _samples[_samples.Count - 1];

        double seconds = (last.At - first.At).TotalSeconds;
        long moved = last.Bytes - first.Bytes;

        if (seconds <= 0 || moved <= 0)
        {
            BytesPerSecond = 0;
            Remaining = null;
            return;
        }

        BytesPerSecond = moved / seconds;

        if (bytesTotal <= 0 || bytesTotal <= last.Bytes)
        {
            Remaining = null;
            return;
        }

        double remainingSeconds = (bytesTotal - last.Bytes) / BytesPerSecond;
        Remaining = remainingSeconds > TimeSpan.MaxValue.TotalSeconds
            ? (TimeSpan?)null
            : TimeSpan.FromSeconds(remainingSeconds);
    }

    private readonly struct Reading
    {
        public Reading(DateTimeOffset at, long bytes)
        {
            At = at;
            Bytes = bytes;
        }

        public DateTimeOffset At { get; }

        public long Bytes { get; }
    }
}
