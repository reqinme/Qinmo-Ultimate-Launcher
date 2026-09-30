using System;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Launch;

namespace Qul.Tests.Launch;

/// <summary>
/// 吞吐与剩余时间估算。存在的理由是冷装可能要走十几分钟，
/// 而用户需要能分清"慢但在推进"和"卡住了"。
/// </summary>
[TestClass]
public sealed class ThroughputEstimatorTests
{
    private static readonly DateTimeOffset T0 = new DateTimeOffset(2026, 1, 1, 0, 0, 0, TimeSpan.Zero);

    [TestMethod]
    public void Rate_IsTheAverageOverTheWindow()
    {
        ThroughputEstimator estimator = new ThroughputEstimator(TimeSpan.FromSeconds(10));

        for (int i = 0; i <= 10; i++)
        {
            estimator.Sample(i * 1000L, 100000L, T0.AddSeconds(i));
        }

        Assert.AreEqual(1000.0, estimator.BytesPerSecond, 1.0);
        Assert.IsTrue(estimator.Remaining.HasValue);
        Assert.AreEqual(90.0, estimator.Remaining!.Value.TotalSeconds, 1.0, "剩 90000 字节 / 1000 每秒");
    }

    [TestMethod]
    public void Rate_ForgetsSamplesOlderThanTheWindow()
    {
        // 开头很慢、后面变快时，用户应该看到**当前**的速度，
        // 否则一条慢的旧记录会把速率长期压在低位，看起来像永远好不了。
        ThroughputEstimator estimator = new ThroughputEstimator(TimeSpan.FromSeconds(5));

        estimator.Sample(0, 0, T0);
        estimator.Sample(1000, 0, T0.AddSeconds(10));

        for (int i = 1; i <= 4; i++)
        {
            estimator.Sample(1000 + (i * 10000L), 0, T0.AddSeconds(10 + i));
        }

        Assert.AreEqual(10000.0, estimator.BytesPerSecond, 1.0);
    }

    [TestMethod]
    public void SinceProgress_GrowsWhenBytesStopMoving()
    {
        // 引擎在重试、在换源时会上报同一个字节数。那正是"卡住"该被看见的时刻。
        ThroughputEstimator estimator = new ThroughputEstimator(TimeSpan.FromSeconds(30));

        estimator.Sample(5000, 100000, T0);
        estimator.Sample(5000, 100000, T0.AddSeconds(20));

        Assert.AreEqual(20.0, estimator.SinceProgress.TotalSeconds, 1.0);
        Assert.IsFalse(estimator.HasRate, "字节没动就不该报出速率");
        Assert.IsFalse(estimator.Remaining.HasValue);
    }

    [TestMethod]
    public void Remaining_IsNullWhenTheTotalIsUnknown()
    {
        // **宁可说"剩余时间未知"，也不要编一个数出来。**
        ThroughputEstimator estimator = new ThroughputEstimator(TimeSpan.FromSeconds(30));

        estimator.Sample(0, 0, T0);
        estimator.Sample(20000, 0, T0.AddSeconds(10));

        Assert.IsTrue(estimator.HasRate);
        Assert.IsFalse(estimator.Remaining.HasValue);

        string text = estimator.Describe();
        Assert.IsTrue(text.Contains("KB/s"), text);
        Assert.IsTrue(text.Contains("未知"), text);
    }

    [TestMethod]
    public void Remaining_IsNullOnceTheTotalIsReached()
    {
        ThroughputEstimator estimator = new ThroughputEstimator(TimeSpan.FromSeconds(30));

        estimator.Sample(0, 10000, T0);
        estimator.Sample(10000, 10000, T0.AddSeconds(10));

        Assert.IsFalse(estimator.Remaining.HasValue, "已经传完了就不该再显示剩余时间");
    }

    [TestMethod]
    public void Describe_IsEmptyBeforeThereIsAnyRate()
    {
        ThroughputEstimator estimator = new ThroughputEstimator(TimeSpan.FromSeconds(30));

        Assert.AreEqual(string.Empty, estimator.Describe());

        estimator.Sample(100, 1000, T0);
        Assert.AreEqual(string.Empty, estimator.Describe(), "一个样本算不出速率，就该什么都不说");
    }

    [TestMethod]
    public void Reset_DropsEverythingIncludingTheStallClock()
    {
        ThroughputEstimator estimator = new ThroughputEstimator(TimeSpan.FromSeconds(30));

        estimator.Sample(0, 100000, T0);
        estimator.Sample(50000, 100000, T0.AddSeconds(10));
        Assert.IsTrue(estimator.HasRate);

        estimator.Reset();

        Assert.IsFalse(estimator.HasRate);
        Assert.IsFalse(estimator.Remaining.HasValue);
        Assert.AreEqual(TimeSpan.Zero, estimator.SinceProgress);
        Assert.AreEqual(string.Empty, estimator.Describe());
    }

    [TestMethod]
    public void Sample_RefusesNonPositiveWindowOrTinyCapacity()
    {
        Assert.ThrowsException<ArgumentOutOfRangeException>(() => new ThroughputEstimator(TimeSpan.Zero));
        Assert.ThrowsException<ArgumentOutOfRangeException>(() => new ThroughputEstimator(TimeSpan.FromSeconds(1), 1));
    }

    [TestMethod]
    public void FormatRate_UsesReadableUnits()
    {
        Assert.AreEqual("512 B/s", ThroughputEstimator.FormatRate(512));
        Assert.AreEqual("2 KB/s", ThroughputEstimator.FormatRate(2048));
        Assert.AreEqual("1.5 MB/s", ThroughputEstimator.FormatRate(1.5 * 1024 * 1024));
        Assert.AreEqual("0 B/s", ThroughputEstimator.FormatRate(-5), "负数只可能来自异常输入，不该显示成负值");
    }

    [TestMethod]
    public void FormatDuration_UsesReadableUnits()
    {
        Assert.AreEqual("不到 1 秒", ThroughputEstimator.FormatDuration(TimeSpan.FromMilliseconds(400)));
        Assert.AreEqual("45 秒", ThroughputEstimator.FormatDuration(TimeSpan.FromSeconds(45)));
        Assert.AreEqual("3 分 20 秒", ThroughputEstimator.FormatDuration(TimeSpan.FromSeconds(200)));
        Assert.AreEqual("2 小时 5 分", ThroughputEstimator.FormatDuration(TimeSpan.FromMinutes(125)));
    }
}
