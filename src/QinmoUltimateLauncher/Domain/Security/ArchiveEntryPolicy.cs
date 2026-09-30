using System;
using System.Collections.Generic;
using System.Text;

namespace Qul.Domain.Security;

/// <summary>压缩包条目路径的判定结论。</summary>
public enum ArchivePathVerdict
{
    Allow,

    /// <summary>条目名为空或全是空白。</summary>
    EmptyName,

    /// <summary>以分隔符或 UNC 前缀开头（绝对路径）。</summary>
    AbsolutePath,

    /// <summary>含盘符或冒号——Windows 下冒号本就是非法文件名字符。</summary>
    DriveLetter,

    /// <summary>含 <c>..</c> 段，会向上越出目标目录。</summary>
    ParentTraversal,

    /// <summary>拼接后的路径不在目标根目录之下。</summary>
    OutsideTargetRoot,

    /// <summary>条目声明为符号链接。zip 条目可以通过外部属性把自己标成软链，从而把写入引到别处。</summary>
    SymbolicLink,

    /// <summary>拼接后的绝对路径过长。</summary>
    TooLong,
}

/// <summary>
/// 压缩包条目的路径安全策略（zip slip / 路径穿越防护）。
///
/// 这是**纯字符串逻辑**，因此放在领域层：判定规则可以被穷举单测，
/// 基础设施只负责"照判定执行"。把安全规则和执行混在一起，是这类漏洞最常见的成因。
///
/// 判定与平台无关地按 Windows 语义做（本项目只面向 Windows），
/// 刻意不使用 <c>System.IO</c>——领域层被禁止引用文件系统。
/// </summary>
public static class ArchiveEntryPolicy
{
    /// <summary>与 DataLayout 保持一致的保守长度上限，留出文件名扩展余量。</summary>
    public const int MaxPathLength = 240;

    private static readonly char[] Separators = { '/', '\\' };

    /// <summary>
    /// 判定条目是否可以安全解压到 <paramref name="targetRoot"/> 之下。
    /// </summary>
    /// <param name="isSymbolicLink">由调用方从压缩包条目的外部属性读出。</param>
    public static ArchivePathVerdict Evaluate(string? entryName, string targetRoot, bool isSymbolicLink = false)
    {
        if (string.IsNullOrWhiteSpace(entryName))
        {
            return ArchivePathVerdict.EmptyName;
        }

        if (string.IsNullOrWhiteSpace(targetRoot))
        {
            // 没有受控根目录就谈不上"安全解压"。
            return ArchivePathVerdict.OutsideTargetRoot;
        }

        if (isSymbolicLink)
        {
            return ArchivePathVerdict.SymbolicLink;
        }

        string name = entryName!.Replace('\\', '/');

        // UNC（\\server\share）与根路径（\ 或 /）都在 Replace 后表现为以 '/' 开头。
        if (name[0] == '/')
        {
            return ArchivePathVerdict.AbsolutePath;
        }

        // 盘符：C:/... 或任何位置出现的冒号（Windows 文件名非法字符，一律拒绝）。
        if (name.IndexOf(':') >= 0)
        {
            return ArchivePathVerdict.DriveLetter;
        }

        List<string> segments = new List<string>();
        foreach (string raw in name.Split(Separators))
        {
            if (raw.Length == 0 || raw == ".")
            {
                // 空段与当前目录段无害，跳过。
                continue;
            }

            if (raw == "..")
            {
                return ArchivePathVerdict.ParentTraversal;
            }

            segments.Add(raw);
        }

        if (segments.Count == 0)
        {
            return ArchivePathVerdict.EmptyName;
        }

        string combined = CombineSegments(targetRoot, segments);

        if (combined.Length > MaxPathLength)
        {
            return ArchivePathVerdict.TooLong;
        }

        // 纵深防御：段扫描已经排除了 .. 与绝对路径，这里再确认一次拼接结果确实在根目录之下。
        if (!IsUnderRoot(combined, targetRoot))
        {
            return ArchivePathVerdict.OutsideTargetRoot;
        }

        return ArchivePathVerdict.Allow;
    }

    /// <summary>
    /// 在判定通过时给出规范化后的目标路径；不通过时返回 null 并输出判定结论。
    /// 调用方必须先看结论再决定是否落盘。
    /// </summary>
    public static string? TryResolveTarget(
        string? entryName,
        string targetRoot,
        out ArchivePathVerdict verdict,
        bool isSymbolicLink = false)
    {
        verdict = Evaluate(entryName, targetRoot, isSymbolicLink);
        if (verdict != ArchivePathVerdict.Allow)
        {
            return null;
        }

        string name = entryName!.Replace('\\', '/');
        List<string> segments = new List<string>();
        foreach (string raw in name.Split(Separators))
        {
            if (raw.Length == 0 || raw == ".")
            {
                continue;
            }

            segments.Add(raw);
        }

        return CombineSegments(targetRoot, segments);
    }

    private static string CombineSegments(string targetRoot, List<string> segments)
    {
        StringBuilder sb = new StringBuilder(targetRoot.Length + 32);
        sb.Append(targetRoot);
        if (sb.Length > 0 && sb[sb.Length - 1] != '\\' && sb[sb.Length - 1] != '/')
        {
            sb.Append('\\');
        }

        for (int i = 0; i < segments.Count; i++)
        {
            if (i > 0)
            {
                sb.Append('\\');
            }

            sb.Append(segments[i]);
        }

        return sb.ToString();
    }

    private static bool IsUnderRoot(string candidate, string targetRoot)
    {
        string root = targetRoot.Replace('/', '\\');
        if (root.Length > 0 && root[root.Length - 1] != '\\')
        {
            root += "\\";
        }

        string normalized = candidate.Replace('/', '\\');
        return normalized.StartsWith(root, StringComparison.OrdinalIgnoreCase);
    }
}
