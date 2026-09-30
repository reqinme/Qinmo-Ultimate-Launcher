using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using Qul.Domain.Diagnostics;
using Qul.Domain.Security;

namespace Qul.Infrastructure.Archives;

public sealed class ExtractionReport
{
    public ExtractionReport(int extractedFiles, int extractedDirectories, IReadOnlyList<string> rejectedEntries, ErrorCode? error, ArchivePathVerdict? firstVerdict)
    {
        ExtractedFiles = extractedFiles;
        ExtractedDirectories = extractedDirectories;
        RejectedEntries = rejectedEntries;
        Error = error;
        FirstVerdict = firstVerdict;
    }

    public int ExtractedFiles { get; }

    public int ExtractedDirectories { get; }

    /// <summary>被拒绝的条目名。整包拒绝时这里列出全部越界条目，便于排障。</summary>
    public IReadOnlyList<string> RejectedEntries { get; }

    public ErrorCode? Error { get; }

    public ArchivePathVerdict? FirstVerdict { get; }

    public bool Succeeded => Error == null;
}

/// <summary>
/// 带路径穿越防护的解压器。
///
/// 执行策略是**整包拒绝**：先对全部条目做一遍判定，只要有一条越界，
/// 就一个字节都不写。理由是先解压再补救是做不到的——恶意条目一旦落盘就已经生效了。
///
/// 判定规则本身在领域层（<see cref="ArchiveEntryPolicy"/>），这里只负责照判定执行。
/// </summary>
public static class SafeZipExtractor
{
    private const int CopyBufferSize = 81920;

    public static ExtractionReport Extract(
        string archivePath,
        string targetRoot,
        IReadOnlyList<string>? excludePrefixes = null)
    {
        if (string.IsNullOrWhiteSpace(archivePath))
        {
            throw new ArgumentException("archive path is required", nameof(archivePath));
        }

        if (string.IsNullOrWhiteSpace(targetRoot))
        {
            throw new ArgumentException("target root is required", nameof(targetRoot));
        }

        List<string> rejected = new List<string>();
        ArchivePathVerdict? firstVerdict = null;

        ZipArchive archive;
        try
        {
            archive = ZipFile.OpenRead(archivePath);
        }
        catch (InvalidDataException)
        {
            return new ExtractionReport(0, 0, rejected, ErrorCode.ZipInvalid, null);
        }
        catch (IOException)
        {
            return new ExtractionReport(0, 0, rejected, ErrorCode.IoDataRootNotWritable, null);
        }

        using (archive)
        {
            // 第一遍：全部条目先判定，一条越界就整包拒绝，绝不落盘。
            List<ZipArchiveEntry> accepted = new List<ZipArchiveEntry>(archive.Entries.Count);

            foreach (ZipArchiveEntry entry in archive.Entries)
            {
                ArchivePathVerdict verdict = ArchiveEntryPolicy.Evaluate(
                    entry.FullName, targetRoot, IsSymbolicLink(entry));

                if (verdict == ArchivePathVerdict.Allow)
                {
                    accepted.Add(entry);
                    continue;
                }

                if (verdict == ArchivePathVerdict.EmptyName)
                {
                    // 无名条目无法映射到任何文件，忽略即可，不构成攻击。
                    continue;
                }

                rejected.Add(entry.FullName);
                if (!firstVerdict.HasValue)
                {
                    firstVerdict = verdict;
                }
            }

            if (rejected.Count > 0)
            {
                return new ExtractionReport(0, 0, rejected, ErrorCode.ZipEntryEscape, firstVerdict);
            }

            // 第二遍：真正解压。
            try
            {
                Directory.CreateDirectory(targetRoot);
            }
            catch (IOException)
            {
                return new ExtractionReport(0, 0, rejected, ErrorCode.IoDataRootNotWritable, null);
            }
            catch (UnauthorizedAccessException)
            {
                return new ExtractionReport(0, 0, rejected, ErrorCode.IoDataRootNotWritable, null);
            }

            int files = 0;
            int directories = 0;

            try
            {
                foreach (ZipArchiveEntry entry in accepted)
                {
                    string? destination = ArchiveEntryPolicy.TryResolveTarget(
                        entry.FullName, targetRoot, out ArchivePathVerdict verdict2, IsSymbolicLink(entry));

                    // 第一遍已判定通过，这里再次确认是纵深防御；不通过就当作异常终止整包。
                    if (destination == null || verdict2 != ArchivePathVerdict.Allow)
                    {
                        return new ExtractionReport(files, directories, new[] { entry.FullName }, ErrorCode.ZipEntryEscape, verdict2);
                    }

                    if (IsExcluded(entry.FullName, excludePrefixes))
                    {
                        continue;
                    }

                    bool isDirectory = string.IsNullOrEmpty(entry.Name);
                    if (isDirectory)
                    {
                        Directory.CreateDirectory(destination);
                        directories++;
                        continue;
                    }

                    string? parent = Path.GetDirectoryName(destination);
                    if (!string.IsNullOrEmpty(parent))
                    {
                        Directory.CreateDirectory(parent!);
                    }

                    using (Stream source = entry.Open())
                    using (FileStream target = new FileStream(destination, FileMode.Create, FileAccess.Write, FileShare.None, CopyBufferSize))
                    {
                        source.CopyTo(target, CopyBufferSize);
                    }

                    files++;
                }
            }
            catch (InvalidDataException)
            {
                return new ExtractionReport(files, directories, rejected, ErrorCode.ZipInvalid, null);
            }
            catch (IOException)
            {
                return new ExtractionReport(files, directories, rejected, ErrorCode.IoDataRootNotWritable, null);
            }
            catch (UnauthorizedAccessException)
            {
                return new ExtractionReport(files, directories, rejected, ErrorCode.IoDataRootNotWritable, null);
            }

            return new ExtractionReport(files, directories, rejected, null, null);
        }
    }

    /// <summary>
    /// zip 条目可以通过 Unix 外部属性把自己标成符号链接，从而让解压把写入引到目录之外。
    /// 这类条目一律拒绝。
    /// </summary>
    private static bool IsSymbolicLink(ZipArchiveEntry entry)
    {
        int unixMode = (entry.ExternalAttributes >> 16) & 0xFFFF;
        return (unixMode & 0xF000) == 0xA000;
    }

    private static bool IsExcluded(string entryName, IReadOnlyList<string>? excludePrefixes)
    {
        if (excludePrefixes == null || excludePrefixes.Count == 0)
        {
            return false;
        }

        string normalized = entryName.Replace('\\', '/');

        for (int i = 0; i < excludePrefixes.Count; i++)
        {
            string prefix = excludePrefixes[i].Replace('\\', '/');
            if (prefix.Length == 0)
            {
                continue;
            }

            if (normalized.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }

            // 元数据里的排除项形如 "META-INF/"，但也要容忍写成 "META-INF" 的情形。
            if (normalized.Length > prefix.Length
                && normalized[prefix.Length] == '/'
                && normalized.StartsWith(prefix.TrimEnd('/'), StringComparison.OrdinalIgnoreCase))
            {
                return true;
            }
        }

        return false;
    }
}
