using System;
using System.IO;
using System.Text;

namespace Qul.Infrastructure.Diagnostics;

/// <summary>
/// 在游戏目录下找**这一次**的崩溃报告，并读出正文。
///
/// 与 <see cref="Qul.Domain.Diagnostics.CrashAnalysis"/> 分开是分层要求：
/// 领域层禁用 <c>System.IO</c>（由反射守卫测试强制），所以"读文件"留在这一层，
/// "从文本得结论"留在领域层。两半各自可测。
/// </summary>
public static class CrashReportLocator
{
    /// <summary>崩溃报告只读开头这么多。开头就是结论，后面是几百行堆栈。</summary>
    public const int MaxReadBytes = 64 * 1024;

    /// <summary>游戏写报告的位置（相对于游戏目录）。</summary>
    public const string ReportsDirectoryName = "crash-reports";

    /// <summary>
    /// 找出 <paramref name="notBefore"/> **之后**写过的最新一份报告；没有就返回 null。
    ///
    /// **时间条件是必须的。** 没有它，上一次运行留下的旧报告会被当成本次的原因——
    /// 玩家会照着一条与本次无关的结论去折腾，那比没有结论更糟。
    /// </summary>
    public static string? FindNewest(string? gameDirectory, DateTimeOffset notBefore)
    {
        if (string.IsNullOrWhiteSpace(gameDirectory))
        {
            return null;
        }

        string directory = Path.Combine(gameDirectory!, ReportsDirectoryName);

        if (!Directory.Exists(directory))
        {
            return null;
        }

        string? best = null;
        DateTime bestTime = DateTime.MinValue;

        try
        {
            foreach (string file in Directory.GetFiles(directory, "*.txt", SearchOption.TopDirectoryOnly))
            {
                DateTime written = File.GetLastWriteTimeUtc(file);

                // 留一点余量：文件系统的写入时间与我们的起始时间可能只差几毫秒。
                if (written < notBefore.UtcDateTime.AddSeconds(-5))
                {
                    continue;
                }

                if (written > bestTime)
                {
                    bestTime = written;
                    best = file;
                }
            }
        }
        catch (Exception ex) when (
            ex is IOException || ex is UnauthorizedAccessException || ex is DirectoryNotFoundException)
        {
            return null;
        }

        return best;
    }

    /// <summary>读报告正文；读不出来返回 null（**绝不因为读不了一份报告而让整次启动失败**）。</summary>
    public static string? Read(string? path)
    {
        if (string.IsNullOrWhiteSpace(path) || !File.Exists(path))
        {
            return null;
        }

        try
        {
            using (FileStream stream = new FileStream(
                path!, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
            using (StreamReader reader = new StreamReader(stream, Encoding.UTF8, detectEncodingFromByteOrderMarks: true))
            {
                char[] buffer = new char[MaxReadBytes];
                int read = reader.Read(buffer, 0, buffer.Length);

                return read <= 0 ? null : new string(buffer, 0, read);
            }
        }
        catch (Exception ex) when (
            ex is IOException || ex is UnauthorizedAccessException || ex is DecoderFallbackException)
        {
            return null;
        }
    }
}
