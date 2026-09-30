using System;
using System.Collections.Generic;
using System.IO;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Domain.Metadata;
using Qul.Infrastructure.Archives;

namespace Qul.Infrastructure.Runtime;

public sealed class NativeExtractionResult
{
    public NativeExtractionResult(
        int archives,
        int files,
        IReadOnlyList<string> missing,
        IReadOnlyList<string> rejected,
        ErrorCode? error)
    {
        ArchivesExtracted = archives;
        FilesExtracted = files;
        MissingArchives = missing;
        RejectedArchives = rejected;
        Error = error;
    }

    public int ArchivesExtracted { get; }

    public int FilesExtracted { get; }

    /// <summary>元数据声明了、但本地不存在的包（下载步骤没拿到）。</summary>
    public IReadOnlyList<string> MissingArchives { get; }

    /// <summary>被路径穿越防护整包拒绝的包。</summary>
    public IReadOnlyList<string> RejectedArchives { get; }

    public ErrorCode? Error { get; }

    public bool Succeeded => Error == null && RejectedArchives.Count == 0;
}

/// <summary>
/// 把本地库解压到 natives 目录，供 <c>-Djava.library.path</c> 使用。
///
/// 必须同时支持两代机制（P1 的实测结论）：
///   第一代（1.7–1.16）：库带 <c>natives</c> 映射，值可能是 <c>natives-windows-${arch}</c> 这样的模板，
///                       也可能指向一个 classifiers 里根本不存在的悬空变体。
///   第二代（26.x）：没有 natives 映射，natives 是坐标自带 classifier 的独立库条目。
///
/// 两代都解压，是因为解压本身无害，而漏解压会导致 <c>java.library.path</c> 空着、游戏找不到本地库。
/// 该决定需要在真实启动验收中确认（见 P4 规范"未决"一节）。
/// </summary>
public static class NativeExtractor
{
    private const string NativeClassifierPrefix = "natives-";

    public static NativeExtractionResult Extract(
        VersionDetail version,
        EnvironmentProfile environment,
        string cacheRoot,
        string nativesDirectory,
        CachePathConventions? pathConventions = null,
        bool cleanTarget = true)
    {
        if (version == null)
        {
            throw new ArgumentNullException(nameof(version));
        }

        if (environment == null)
        {
            throw new ArgumentNullException(nameof(environment));
        }

        CachePathConventions paths = pathConventions ?? new CachePathConventions();

        List<string> missing = new List<string>();
        List<string> rejected = new List<string>();

        if (cleanTarget && Directory.Exists(nativesDirectory))
        {
            try
            {
                Directory.Delete(nativesDirectory, recursive: true);
            }
            catch (IOException)
            {
                // 删不掉就留着；下面按文件覆盖，不会因为残留而崩。
            }
            catch (UnauthorizedAccessException)
            {
            }
        }

        try
        {
            Directory.CreateDirectory(nativesDirectory);
        }
        catch (IOException)
        {
            return new NativeExtractionResult(0, 0, missing, rejected, ErrorCode.IoDataRootNotWritable);
        }
        catch (UnauthorizedAccessException)
        {
            return new NativeExtractionResult(0, 0, missing, rejected, ErrorCode.IoDataRootNotWritable);
        }

        int archives = 0;
        int files = 0;

        for (int i = 0; i < version.Libraries.Count; i++)
        {
            LibraryRef library = version.Libraries[i];

            if (!RuleEvaluator.IsAllowed(library.Rules, environment))
            {
                continue;
            }

            if (!TryResolveArchive(library, environment, out DownloadRef? archive, out bool isLegacy))
            {
                continue;
            }

            string relative = paths.LibraryFile(archive!.Path, library.Name, library.RawName);
            string absolute = Combine(cacheRoot, relative);

            if (!File.Exists(absolute))
            {
                missing.Add(relative);
                continue;
            }

            ExtractionReport report = SafeZipExtractor.Extract(absolute, nativesDirectory, library.ExtractExclude);

            if (!report.Succeeded)
            {
                if (report.Error == ErrorCode.ZipEntryEscape)
                {
                    rejected.Add(relative);
                    continue;
                }

                return new NativeExtractionResult(archives, files, missing, rejected, report.Error);
            }

            archives++;
            files += report.ExtractedFiles;

            // isLegacy 只用于说明；两代都解压。
            _ = isLegacy;
        }

        return new NativeExtractionResult(archives, files, missing, rejected, null);
    }

    /// <summary>
    /// 找出这个库在该平台上应当解压的 natives 包。
    /// 旧式走 natives 映射（含 ${arch} 模板替换与悬空引用跳过），新式走坐标里的 classifier。
    /// </summary>
    public static bool TryResolveArchive(
        LibraryRef library,
        EnvironmentProfile environment,
        out DownloadRef? archive,
        out bool isLegacy)
    {
        archive = null;
        isLegacy = false;

        // 第一代：natives 映射。
        string? classifierKey = NativeClassifierSelector.Select(library, environment);
        if (classifierKey != null && library.Classifiers.TryGetValue(classifierKey, out DownloadRef? fromClassifiers))
        {
            archive = fromClassifiers;
            isLegacy = true;
            return true;
        }

        // 第二代：坐标自带 natives-* 的 classifier。
        string? nameClassifier = library.Name?.Classifier;
        if (nameClassifier != null
            && nameClassifier.StartsWith(NativeClassifierPrefix, StringComparison.OrdinalIgnoreCase)
            && library.Artifact != null)
        {
            archive = library.Artifact;
            isLegacy = false;
            return true;
        }

        return false;
    }

    private static string Combine(string root, string relative)
    {
        string left = root.Replace('/', '\\').TrimEnd('\\');
        string right = relative.Replace('/', '\\').TrimStart('\\');
        return left + "\\" + right;
    }
}
