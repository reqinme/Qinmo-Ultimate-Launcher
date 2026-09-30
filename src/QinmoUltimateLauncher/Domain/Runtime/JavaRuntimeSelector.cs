using System;
using System.Collections.Generic;
using System.Globalization;
using Qul.Domain.Diagnostics;

namespace Qul.Domain.Runtime;

/// <summary>
/// 从候选里选出该版本要用的 Java。
///
/// 纯函数，因此"多个 Java 时到底选哪个"可以被完整断言，不需要真的装 7 套 JDK。
/// 每个候选都会得到一条判定理由——这是验收里"说得清"那一条的落点。
/// </summary>
public static class JavaRuntimeSelector
{
    public static JavaSelectionResult Select(
        IReadOnlyList<JavaRuntimeCandidate>? candidates,
        JavaSelectionRequest request)
    {
        if (request == null)
        {
            throw new ArgumentNullException(nameof(request));
        }

        List<JavaRuntimeCandidate> all = new List<JavaRuntimeCandidate>();
        if (candidates != null)
        {
            for (int i = 0; i < candidates.Count; i++)
            {
                if (candidates[i] != null)
                {
                    all.Add(candidates[i]);
                }
            }
        }

        if (all.Count == 0)
        {
            return new JavaSelectionResult(
                null,
                ErrorCode.JavaNotFound,
                DescribeMissing(request),
                Array.Empty<JavaCandidateAssessment>(),
                all);
        }

        int required = request.RequiredMajorVersion;
        string? requiredArch = JavaRuntimeCandidate.NormalizeArch(request.RequiredOsArch);

        List<JavaCandidateAssessment> assessments = new List<JavaCandidateAssessment>(all.Count);
        List<JavaRuntimeCandidate> accepted = new List<JavaRuntimeCandidate>();

        for (int i = 0; i < all.Count; i++)
        {
            JavaRuntimeCandidate candidate = all[i];
            string? rejection = Reject(candidate, required, requiredArch, request.RequiredBitness);

            if (rejection == null)
            {
                assessments.Add(new JavaCandidateAssessment(candidate, true, "满足要求"));
                accepted.Add(candidate);
            }
            else
            {
                assessments.Add(new JavaCandidateAssessment(candidate, false, rejection));
            }
        }

        if (accepted.Count == 0)
        {
            return new JavaSelectionResult(
                null,
                ErrorCode.JavaVersionMismatch,
                DescribeMismatch(all, required),
                assessments,
                all);
        }

        accepted.Sort((left, right) => Compare(left, right, required, request));

        JavaRuntimeCandidate selected = accepted[0];

        return new JavaSelectionResult(
            selected,
            null,
            "选中 " + selected.Describe() + "（" + (selected.Vendor ?? "厂商未知") + "）："
                + (required > 0 ? "该版本要求主版本 ≥ " + required.ToString(CultureInfo.InvariantCulture) : "未指定主版本要求")
                + "，共评估 " + all.Count.ToString(CultureInfo.InvariantCulture) + " 个候选。",
            assessments,
            all);
    }

    private static string? Reject(JavaRuntimeCandidate candidate, int required, string? requiredArch, int? requiredBitness)
    {
        if (!candidate.Version.IsValid)
        {
            return "版本号无法识别";
        }

        if (required > 0 && candidate.Version.Major < required)
        {
            return "主版本 " + candidate.Version.Major.ToString(CultureInfo.InvariantCulture)
                   + " 低于要求的 " + required.ToString(CultureInfo.InvariantCulture);
        }

        if (requiredArch != null)
        {
            string? actualArch = JavaRuntimeCandidate.NormalizeArch(candidate.OsArch);
            if (!string.Equals(actualArch, requiredArch, StringComparison.OrdinalIgnoreCase))
            {
                return "架构 " + (actualArch ?? "未知") + " 与要求的 " + requiredArch + " 不符";
            }
        }

        // 位宽未知（0）时不据此拒绝：那是"探测不到"，不是"不匹配"。
        if (requiredBitness.HasValue && candidate.Bitness != 0 && candidate.Bitness != requiredBitness.Value)
        {
            return "位宽 " + candidate.Bitness.ToString(CultureInfo.InvariantCulture)
                   + " 与要求的 " + requiredBitness.Value.ToString(CultureInfo.InvariantCulture) + " 不符";
        }

        return null;
    }

    private static int Compare(
        JavaRuntimeCandidate left,
        JavaRuntimeCandidate right,
        int required,
        JavaSelectionRequest request)
    {
        if (request.Policy == JavaSelectionPolicy.Latest)
        {
            int byVersionDesc = right.Version.CompareTo(left.Version);
            if (byVersionDesc != 0)
            {
                return byVersionDesc;
            }
        }
        else
        {
            // SmallestSatisfying：先看"比要求高多少"，越接近越优先（精确匹配排最前）。
            int leftDelta = required > 0 ? Math.Max(0, left.Version.Major - required) : 0;
            int rightDelta = required > 0 ? Math.Max(0, right.Version.Major - required) : 0;

            int byDelta = leftDelta.CompareTo(rightDelta);
            if (byDelta != 0)
            {
                return byDelta;
            }

            // 同一档内取更高的版本（同主版本的多个安装里挑更新的）。
            int byVersionDesc = right.Version.CompareTo(left.Version);
            if (byVersionDesc != 0)
            {
                return byVersionDesc;
            }
        }

        // 位宽偏好：用户没指定时优先 64 位。
        int preferredBitness = request.RequiredBitness ?? 64;
        int leftPenalty = left.Bitness == preferredBitness ? 0 : 1;
        int rightPenalty = right.Bitness == preferredBitness ? 0 : 1;

        int byBitness = leftPenalty.CompareTo(rightPenalty);
        if (byBitness != 0)
        {
            return byBitness;
        }

        // 全序收尾：路径序。没有它，并列候选的选出结果会随输入顺序漂移。
        return string.CompareOrdinal(left.ExecutablePath, right.ExecutablePath);
    }

    private static string DescribeMissing(JavaSelectionRequest request)
    {
        string requiredText = request.RequiredMajorVersion > 0
            ? " Java " + request.RequiredMajorVersion.ToString(CultureInfo.InvariantCulture) + " 或更高版本"
            : " Java";

        return "本机未找到可用的 Java。请安装" + requiredText
               + "，或在设置中手动指定 java.exe 的路径。";
    }

    private static string DescribeMismatch(IReadOnlyList<JavaRuntimeCandidate> all, int required)
    {
        int lowest = int.MaxValue;
        for (int i = 0; i < all.Count; i++)
        {
            int major = all[i].Version.Major;
            if (major > 0 && major < lowest)
            {
                lowest = major;
            }
        }

        string lowestText = lowest == int.MaxValue ? "未知" : lowest.ToString(CultureInfo.InvariantCulture);

        return "本机有 " + all.Count.ToString(CultureInfo.InvariantCulture) + " 个 Java，"
               + "但没有一个满足主版本 ≥ " + required.ToString(CultureInfo.InvariantCulture)
               + "（最低的是 " + lowestText + "）。请安装符合要求的 Java，或在设置中手动指定。";
    }
}
