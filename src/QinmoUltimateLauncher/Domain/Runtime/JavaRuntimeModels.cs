using System;
using System.Collections.Generic;
using Qul.Domain.Diagnostics;

namespace Qul.Domain.Runtime;

/// <summary>
/// 运行时来源。三种实现的边界：
///   Detected  —— 本机探测（MVP）
///   Manual    —— 用户手动指定（MVP）
///   Downloaded—— 自动下载（P10，默认关闭）
/// </summary>
public enum JavaRuntimeSource
{
    Detected = 0,
    Manual = 1,
    Downloaded = 2,
}

/// <summary>
/// 一个可用的 Java 运行时。
///
/// 所有字段都来自**实际执行 java.exe 的输出**，不来自目录名——
/// 尖刺 S5 已确认目录名不可信（本机就有 jdk-25.0.4.1 这类四段命名，
/// 以及一个没有 java.exe 的空壳目录 latest）。
/// </summary>
public sealed class JavaRuntimeCandidate
{
    public string ExecutablePath { get; set; } = string.Empty;

    public JavaVersion Version { get; set; }

    /// <summary>归一化后的 CPU 架构：x86 / x86_64 / arm64。</summary>
    public string? OsArch { get; set; }

    /// <summary>JVM 位宽（32 或 64），来自 sun.arch.data.model。</summary>
    public int Bitness { get; set; }

    public string? Vendor { get; set; }

    public string? VmName { get; set; }

    public JavaRuntimeSource Source { get; set; } = JavaRuntimeSource.Detected;

    /// <summary>
    /// 元数据里 <c>${arch}</c> 占位符需要的位宽标记。
    /// 它要的是 32 / 64，而不是 x86_64 这类架构名——这个映射由 sun.arch.data.model 直接给出，
    /// 不必从架构名猜。
    /// </summary>
    public string ArchToken => Bitness == 32 ? "32" : "64";

    public string Describe()
    {
        return "Java " + Version.Major + " (" + (OsArch ?? "?") + " / " + ArchToken + "-bit)";
    }

    /// <summary>把 os.arch / PROCESSOR_ARCHITECTURE 之类的写法归一成 Minecraft 元数据用的三个取值。</summary>
    public static string? NormalizeArch(string? osArch)
    {
        if (string.IsNullOrWhiteSpace(osArch))
        {
            return null;
        }

        switch (osArch!.Trim().ToLowerInvariant())
        {
            case "amd64":
            case "x86_64":
            case "x64":
            case "em64t":
                return "x86_64";
            case "x86":
            case "i386":
            case "i486":
            case "i586":
            case "i686":
            case "ia32":
                return "x86";
            case "aarch64":
            case "arm64":
                return "arm64";
            default:
                return osArch.Trim().ToLowerInvariant();
        }
    }
}

/// <summary>从 java 输出里解析出来的原始探测结果（还没有可执行文件路径）。</summary>
public sealed class JavaProbeResult
{
    public JavaVersion Version { get; set; }

    public string? OsArch { get; set; }

    public int Bitness { get; set; }

    public string? Vendor { get; set; }

    public string? VmName { get; set; }

    public bool IsUsable => Version.IsValid;
}

public enum JavaSelectionPolicy
{
    /// <summary>
    /// 优先精确匹配；没有精确匹配时取"满足要求里最小的"。**默认。**
    ///
    /// 这是对规划文档措辞的一处有意偏离。文档写的是"选出满足要求的最新可用者"，
    /// 但对 Minecraft 而言那是**有害**的：1.16.5 声明需要 Java 8，
    /// 而用 Java 17/25 去跑会因为模块系统与 LWJGL 本地库的限制直接失败。
    /// 元数据里的 majorVersion 是**下界**，不是"随便多新都行"。
    /// </summary>
    SmallestSatisfying = 0,

    /// <summary>满足要求里最新的。保留该策略是为了可测与可切换，不作为默认。</summary>
    Latest = 1,
}

public sealed class JavaSelectionRequest
{
    public int RequiredMajorVersion { get; set; }

    /// <summary>要求的 CPU 架构；为空表示不限制。</summary>
    public string? RequiredOsArch { get; set; }

    /// <summary>要求的 JVM 位宽（32/64）；为空表示不限制，此时优先 64。</summary>
    public int? RequiredBitness { get; set; }

    public JavaSelectionPolicy Policy { get; set; } = JavaSelectionPolicy.SmallestSatisfying;
}

/// <summary>对单个候选的判定，用于"说得清"——为什么选它、为什么没选别人。</summary>
public sealed class JavaCandidateAssessment
{
    public JavaCandidateAssessment(JavaRuntimeCandidate candidate, bool accepted, string reason)
    {
        Candidate = candidate;
        Accepted = accepted;
        Reason = reason;
    }

    public JavaRuntimeCandidate Candidate { get; }

    public bool Accepted { get; }

    public string Reason { get; }
}

public sealed class JavaSelectionResult
{
    public JavaSelectionResult(
        JavaRuntimeCandidate? selected,
        ErrorCode? error,
        string? explanation,
        IReadOnlyList<JavaCandidateAssessment> assessments,
        IReadOnlyList<JavaRuntimeCandidate> considered)
    {
        Selected = selected;
        Error = error;
        Explanation = explanation;
        Assessments = assessments;
        Considered = considered;
    }

    public JavaRuntimeCandidate? Selected { get; }

    public ErrorCode? Error { get; }

    /// <summary>给用户看的一句话结论；无可用 Java 时这里就是"可执行指引"的原料。</summary>
    public string? Explanation { get; }

    /// <summary>逐个候选的判定与理由。</summary>
    public IReadOnlyList<JavaCandidateAssessment> Assessments { get; }

    public IReadOnlyList<JavaRuntimeCandidate> Considered { get; }

    public bool Succeeded => Selected != null;
}
