using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;
using Qul.Application.Diagnostics;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Runtime;
using Qul.Infrastructure.Diagnostics;

namespace Qul.Infrastructure.Diagnostics;

public sealed class DiagnosticReportInput
{
    public string LauncherVersion { get; set; } = string.Empty;

    public string DataPlacement { get; set; } = string.Empty;

    public IdentitySource? IdentitySource { get; set; }

    public bool IsOnlineVerified { get; set; }

    public string? VersionId { get; set; }

    public string? SkeletonHash { get; set; }

    public JavaRuntimeCandidate? SelectedJava { get; set; }

    public IReadOnlyList<JavaRuntimeCandidate> JavaCandidates { get; set; } = Array.Empty<JavaRuntimeCandidate>();

    public IReadOnlyList<PreflightItem> Preflight { get; set; } = Array.Empty<PreflightItem>();

    public ErrorCode? Error { get; set; }

    public string? ErrorDetail { get; set; }

    public IReadOnlyList<string> Notes { get; set; } = Array.Empty<string>();

    public IReadOnlyList<string> LogTail { get; set; } = Array.Empty<string>();
}

/// <summary>
/// 脱敏诊断报告。
///
/// 目标只有一个：**用户点一下就能把"能定位问题"的信息交出来，而且交出来的东西不泄露任何秘密。**
/// 因此每一行都必须过脱敏管道——不是"记得脱敏"，而是结构上只有一条出口。
/// </summary>
public static class DiagnosticReportBuilder
{
    private const int LogTailLines = 80;

    public static string Build(DiagnosticReportInput input)
    {
        if (input == null)
        {
            throw new ArgumentNullException(nameof(input));
        }

        StringBuilder builder = new StringBuilder(4096);

        Line(builder, "# Qinmo Ultimate Launcher 诊断报告");
        Line(builder, "生成时间=" + DateTimeOffset.Now.ToString("u", CultureInfo.InvariantCulture));
        Line(builder, "启动器版本=" + input.LauncherVersion);
        Line(builder, "操作系统=" + Environment.OSVersion.VersionString);
        Line(builder, "进程架构=" + (Environment.Is64BitProcess ? "64-bit" : "32-bit"));
        Line(builder, "数据目录方式=" + input.DataPlacement);
        Line(builder, string.Empty);

        Line(builder, "## 身份");
        Line(builder, "来源=" + (input.IdentitySource?.ToString() ?? "未选择"));
        Line(builder, "在线验证=" + (input.IsOnlineVerified ? "是" : "否"));
        Line(builder, string.Empty);

        Line(builder, "## 版本与计划");
        Line(builder, "版本=" + (input.VersionId ?? "未确定"));
        Line(builder, "骨架指纹=" + (input.SkeletonHash ?? "未生成"));
        Line(builder, string.Empty);

        Line(builder, "## Java");
        Line(builder, "选定=" + (input.SelectedJava == null ? "未选定" : input.SelectedJava.Describe()));
        Line(builder, "选定路径=" + (input.SelectedJava?.ExecutablePath ?? "无"));

        if (input.JavaCandidates.Count > 0)
        {
            Line(builder, "候选 " + input.JavaCandidates.Count + " 个：");

            for (int i = 0; i < input.JavaCandidates.Count; i++)
            {
                Line(builder, "  - " + input.JavaCandidates[i].Describe() + " @ " + input.JavaCandidates[i].ExecutablePath);
            }
        }

        Line(builder, string.Empty);

        Line(builder, "## 启动前预检");
        if (input.Preflight.Count == 0)
        {
            Line(builder, "（无）");
        }
        else
        {
            for (int i = 0; i < input.Preflight.Count; i++)
            {
                PreflightItem item = input.Preflight[i];
                Line(builder, "[" + item.Scope + "/" + item.Severity + "] " + item.Id + " " + item.Title);

                string[] detailLines = (item.Detail ?? string.Empty).Split('\n');
                for (int j = 0; j < detailLines.Length; j++)
                {
                    Line(builder, "    " + detailLines[j]);
                }
            }
        }

        Line(builder, string.Empty);

        Line(builder, "## 结果");
        if (input.Error.HasValue)
        {
            Line(builder, "错误码=" + ErrorCodes.Id(input.Error.Value));
            Line(builder, "人话提示=" + ErrorCodes.Hint(input.Error.Value));
            Line(builder, "细节=" + (input.ErrorDetail ?? "无"));
        }
        else
        {
            Line(builder, "错误码=无");
        }

        if (input.Notes.Count > 0)
        {
            Line(builder, "过程记录：");

            for (int i = 0; i < input.Notes.Count; i++)
            {
                Line(builder, "  " + input.Notes[i]);
            }
        }

        Line(builder, string.Empty);

        Line(builder, "## 日志尾部（最近 " + LogTailLines + " 行）");
        int from = Math.Max(0, input.LogTail.Count - LogTailLines);

        for (int i = from; i < input.LogTail.Count; i++)
        {
            Line(builder, input.LogTail[i]);
        }

        Line(builder, string.Empty);

        Line(builder, "## 复现步骤");
        Line(builder, "1. 打开启动器（版本见上）");
        Line(builder, "2. 选择版本 " + (input.VersionId ?? "（见上）"));
        Line(builder, "3. 选择身份来源 " + (input.IdentitySource?.ToString() ?? "（见上）"));
        Line(builder, "4. 点击启动");
        Line(builder, "5. 观察上方的错误码与日志尾部");

        return builder.ToString();
    }

    /// <summary>
    /// 唯一出口：**任何进入报告的内容都必须经过脱敏**。
    /// 这不是一句约定，而是结构上只有这一个方法会写内容。
    /// </summary>
    private static void Line(StringBuilder builder, string text)
    {
        builder.Append(Redactor.Scrub(text)).Append('\n');
    }
}
