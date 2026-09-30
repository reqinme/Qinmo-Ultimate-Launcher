using System;
using System.Collections.Generic;

namespace Qul.Domain.Diagnostics;

/// <summary>
/// 一次崩溃分析的结果：**人话结论 + 可操作的下一步**。
///
/// 刻意不是错误码。错误码回答"我们这层出了什么事"，
/// 而这里回答的是"**游戏自己挂了，最可能是什么原因、你该做什么**"——
/// 玩家拿到 <c>QUL-PROC-0001</c> 是没用的，拿到"Java 版本太低，请换 Java 17"才有用。
/// </summary>
public sealed class CrashFinding
{
    public CrashFinding(string summary, IReadOnlyList<string> suggestions, string evidence)
    {
        Summary = summary;
        Suggestions = suggestions;
        Evidence = evidence;
    }

    /// <summary>一句中文结论，直接给玩家看。</summary>
    public string Summary { get; }

    /// <summary>可操作的下一步。至少一条。</summary>
    public IReadOnlyList<string> Suggestions { get; }

    /// <summary>触发这条结论的原文片段，用于诊断报告与排障。</summary>
    public string Evidence { get; }
}

/// <summary>
/// 从 Minecraft 崩溃报告里读出**能行动**的结论。
///
/// 设计约束：
/// <list type="bullet">
/// <item><b>纯函数。</b>输入是报告正文与退出码，输出是结论——不碰文件系统，因此可以直接测。
/// 找文件是另一件事（见 <c>CrashReportLocator</c>）。</item>
/// <item><b>规则按"特异性从高到低"排列。</b>越具体的特征越先判，
/// 否则一条笼统的规则会把具体原因吃掉——本项目在错误分类上已经踩过这个坑。</item>
/// <item><b>认不出来就老实说认不出来。</b>给一句"我们没有识别出已知原因"加上去哪看日志，
/// 而不是编一个像模像样的猜测。玩家按错误的方向折腾，比不知道更糟。</item>
/// </list>
///
/// **崩溃特征取自 Minecraft 崩溃报告本身**（游戏自己写进 crash-reports 的文本），
/// 不是从任何第三方启动器的代码里取的。
/// </summary>
public static class CrashAnalysis
{
    /// <summary>报告正文里最多看多少字符。崩溃报告开头就是结论，后面是长堆栈。</summary>
    public const int MaxScanLength = 64 * 1024;

    /// <summary>
    /// 分析崩溃报告。<paramref name="reportText"/> 为空时，
    /// 仅按退出码给出力所能及的结论（这在"游戏根本没来得及写报告"时很常见）。
    /// </summary>
    public static CrashFinding? Analyze(string? reportText, int exitCode)
    {
        string text = reportText ?? string.Empty;

        if (text.Length > MaxScanLength)
        {
            text = text.Substring(0, MaxScanLength);
        }

        // ---------- 按特异性从高到低 ----------

        // Java 版本：类文件版本不兼容。这是"选错 Java"最典型的形态。
        string? evidence = Find(text, "UnsupportedClassVersionError");
        if (evidence != null)
        {
            return new CrashFinding(
                "游戏用它不支持的 Java 版本启动了（类文件版本不兼容）。",
                new List<string>
                {
                    "在「身份与版本」区域确认选中的 Java 与这个版本匹配；",
                    "1.16.5 及更早需要 Java 8，1.17–1.20 需要 Java 17，更新的版本需要 Java 21 或更高；",
                    "如果本机装了多个 Java，本启动器默认会挑「刚好够用」的那个，手动指定时请对照上一条。",
                },
                evidence);
        }

        // natives 缺失：LWJGL 找不到本地库。解压失败或被安全软件删掉都会长这样。
        evidence = Find(text, "UnsatisfiedLinkError") ?? Find(text, "NoClassDefFoundError: org/lwjgl");
        if (evidence != null)
        {
            return new CrashFinding(
                "游戏缺少本地库（natives）——通常是解压没成功，或被安全软件删掉了。",
                new List<string>
                {
                    "在「运行日志」里点「复制诊断信息」，把报告留一份；",
                    "删掉数据目录下的缓存后重新启动一次，让启动器重新解压 natives；",
                    "如果反复出现，检查安全软件是否拦截了 .dll 文件。",
                },
                evidence);
        }

        // 内存不足。
        evidence = Find(text, "OutOfMemoryError") ?? Find(text, "Java heap space");
        if (evidence != null)
        {
            return new CrashFinding(
                "游戏内存不足（Java 堆耗尽）。",
                new List<string>
                {
                    "把「最大内存」调大一些（例如从 2048 MB 调到 4096 MB）；",
                    "同时确认系统本身还有空闲物理内存，以及启动器用的是 64 位 Java；",
                    "装了大量模组时，内存需求会明显高于原版。",
                },
                evidence);
        }

        // 显卡 / OpenGL：驱动过旧或根本没有可用的 3D 加速。
        evidence = Find(text, "Pixel format not accelerated")
                ?? Find(text, "EXCEPTION_ACCESS_VIOLATION")
                ?? Find(text, "The driver does not appear to support OpenGL");
        if (evidence != null)
        {
            return new CrashFinding(
                "显卡驱动有问题，游戏拿不到可用的 OpenGL 环境。",
                new List<string>
                {
                    "更新显卡驱动（尤其是从官网装的完整驱动，而不是系统自带的基础驱动）；",
                    "在虚拟机或远程桌面里通常没有可用的 3D 加速，这类环境无法正常运行；",
                    "如果刚换过硬件，先重启一次再试。",
                },
                evidence);
        }

        // 模组：只有装了加载器才可能出现。
        evidence = Find(text, "ModLoadingException") ?? Find(text, "net.minecraftforge") ?? Find(text, "fabricmc");
        if (evidence != null)
        {
            return new CrashFinding(
                "看起来与模组加载有关。",
                new List<string>
                {
                    "先只留一个模组试，确认是哪一个引起的；",
                    "确认模组版本与游戏版本匹配（这是最常见的原因）；",
                    "把完整的崩溃报告贴到对应模组的反馈渠道——它能看出具体是哪个模组。",
                },
                evidence);
        }

        // 认不出来：如实说，并给出下一步。**不编猜测。**
        if (text.Length > 0)
        {
            return new CrashFinding(
                "游戏异常退出，但崩溃报告里没有我们认得的原因。",
                new List<string>
                {
                    "在「运行日志」里点「复制诊断信息」——报告里包含版本、Java、显卡与完整日志路径；",
                    "崩溃报告原文通常在游戏目录的 crash-reports 文件夹里，可以直接打开看。",
                },
                FirstMeaningfulLine(text));
        }

        // 连报告都没有：只凭退出码说话。
        if (exitCode != 0)
        {
            return new CrashFinding(
                "游戏异常退出（退出码 " + exitCode.ToString(System.Globalization.CultureInfo.InvariantCulture)
                + "），但没有留下崩溃报告。",
                new List<string>
                {
                    "这种情况多半是游戏在写出报告之前就被终止了（内存被系统回收、被安全软件拦截等）；",
                    "在「运行日志」里点「复制诊断信息」，报告里包含完整的游戏输出；",
                    "试着把最大内存调小一些再启动——分配过大在物理内存不足时反而会立刻失败。",
                },
                "exitCode=" + exitCode.ToString(System.Globalization.CultureInfo.InvariantCulture));
        }

        // 正常退出：没有可分析的。
        return null;
    }

    /// <summary>找到特征串就返回它所在的那一整行（截断），否则返回 null。</summary>
    private static string? Find(string text, string needle)
    {
        int index = text.IndexOf(needle, StringComparison.Ordinal);
        if (index < 0)
        {
            return null;
        }

        return LineAt(text, index);
    }

    /// <summary>取某个位置所在的整行，去掉首尾空白并限制长度。</summary>
    private static string LineAt(string text, int index)
    {
        int start = text.LastIndexOf('\n', Math.Max(0, index - 1));
        start = start < 0 ? 0 : start + 1;

        int end = text.IndexOf('\n', index);
        end = end < 0 ? text.Length : end;

        string line = text.Substring(start, end - start).Trim();

        if (line.Length == 0)
        {
            line = text.Substring(index, Math.Min(120, text.Length - index)).Trim();
        }

        return line.Length > 200 ? line.Substring(0, 200) + "…" : line;
    }

    /// <summary>报告的第一行有内容的文本，通常就是"Minecraft has crashed!"那句概括。</summary>
    private static string FirstMeaningfulLine(string text)
    {
        string[] lines = text.Split('\n');

        for (int i = 0; i < lines.Length && i < 40; i++)
        {
            string line = lines[i].Trim();

            if (line.Length > 0 && !line.StartsWith("----", StringComparison.Ordinal)
                && !line.StartsWith("//", StringComparison.Ordinal))
            {
                return line.Length > 200 ? line.Substring(0, 200) + "…" : line;
            }
        }

        return string.Empty;
    }
}
