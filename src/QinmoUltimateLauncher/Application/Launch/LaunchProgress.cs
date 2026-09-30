using System.Collections.Generic;

namespace Qul.Application.Launch;

/// <summary>
/// 启动过程的阶段。界面按它显示"解析 / 下载 / 校验 / 解压 / 启动"。
///
/// 在此之前进度只是一句自由文本，界面没法表达"现在到哪一步了"。
/// </summary>
public enum LaunchStage
{
    Idle = 0,
    Resolving = 1,
    Downloading = 2,
    Verifying = 3,
    Extracting = 4,
    Starting = 5,
    Running = 6,
    Completed = 7,
    Failed = 8,
    Cancelled = 9,
}

public sealed class LaunchProgress
{
    public LaunchProgress(
        LaunchStage stage,
        string message,
        int completed = 0,
        int total = 0,
        long bytesCompleted = 0,
        long bytesTotal = 0)
    {
        Stage = stage;
        Message = message;
        Completed = completed;
        Total = total;
        BytesCompleted = bytesCompleted;
        BytesTotal = bytesTotal;
    }

    public LaunchStage Stage { get; }

    public string Message { get; }

    public int Completed { get; }

    public int Total { get; }

    /// <summary>
    /// 已传字节。**在此之前它只以文本形式出现在 <see cref="Message"/> 里**，
    /// 界面拿不到可计算的数值，也就没法算吞吐与剩余时间——
    /// 冷装十几分钟时用户只能看到一行"开始获取所需文件…"。
    /// </summary>
    public long BytesCompleted { get; }

    /// <summary>已知总字节。0 表示未知（例如元数据里没给体积）。</summary>
    public long BytesTotal { get; }

    /// <summary>有总量时才是确定进度；否则界面应当显示不确定态。</summary>
    public bool IsDeterminate => Total > 0;

    public double Fraction
    {
        get
        {
            if (Total <= 0)
            {
                return 0;
            }

            double value = (double)Completed / Total;
            return value < 0 ? 0 : value > 1 ? 1 : value;
        }
    }
}

/// <summary>阶段的展示信息。界面与命令行共用，避免两处各写一套中文。</summary>
public static class LaunchStages
{
    /// <summary>用户看得到的五个阶段，按发生顺序。</summary>
    public static readonly IReadOnlyList<LaunchStage> Visible = new[]
    {
        LaunchStage.Resolving,
        LaunchStage.Downloading,
        LaunchStage.Verifying,
        LaunchStage.Extracting,
        LaunchStage.Starting,
    };

    public static string Label(LaunchStage stage)
    {
        switch (stage)
        {
            case LaunchStage.Resolving:
                return "解析";
            case LaunchStage.Downloading:
                return "下载";
            case LaunchStage.Verifying:
                return "校验";
            case LaunchStage.Extracting:
                return "解压";
            case LaunchStage.Starting:
                return "启动";
            case LaunchStage.Running:
                return "运行中";
            case LaunchStage.Completed:
                return "已完成";
            case LaunchStage.Failed:
                return "失败";
            case LaunchStage.Cancelled:
                return "已取消";
            default:
                return "待命";
        }
    }

    /// <summary>
    /// 某个阶段相对当前阶段处于什么位置。
    /// 用来把阶段条渲染成"已完成 / 进行中 / 未开始"。
    /// </summary>
    public static int OrderOf(LaunchStage stage)
    {
        switch (stage)
        {
            case LaunchStage.Resolving:
                return 1;
            case LaunchStage.Downloading:
                return 2;
            case LaunchStage.Verifying:
                return 3;
            case LaunchStage.Extracting:
                return 4;
            case LaunchStage.Starting:
            case LaunchStage.Running:
            case LaunchStage.Completed:
                return 5;
            default:
                return 0;
        }
    }
}
