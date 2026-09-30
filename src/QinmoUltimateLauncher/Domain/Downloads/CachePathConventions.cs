using System;
using Qul.Domain.Metadata;

namespace Qul.Domain.Downloads;

/// <summary>
/// 缓存目录的相对路径约定。
/// 这里是**纯字符串规则**，不做任何文件操作，因此可以被完整单测。
///
/// 分隔符统一用 <c>/</c>：<see cref="DownloadItem.RelativePath"/> 是逻辑路径，
/// 不是平台路径；Windows 的文件 API 同样接受 <c>/</c>。
/// 统一分隔符也是启动计划骨架能逐字节比对的前提之一。
/// </summary>
public sealed class CachePathConventions
{
    public string MetaPrefix { get; set; } = "meta";

    public string LibrariesPrefix { get; set; } = "libraries";

    public string ObjectsPrefix { get; set; } = "objects";

    public string LoggingPrefix { get; set; } = "logging";

    public string VersionManifestFile()
    {
        return MetaPrefix + "/version_manifest_v2.json";
    }

    public string VersionDetailFile(string versionId)
    {
        return MetaPrefix + "/version-" + Sanitize(versionId) + ".json";
    }

    public string ClientJarFile(string versionId)
    {
        return MetaPrefix + "/client-" + Sanitize(versionId) + ".jar";
    }

    public string AssetIndexFile(string assetIndexId)
    {
        return MetaPrefix + "/assets-" + Sanitize(assetIndexId) + ".json";
    }

    public string LoggingFile(string fileName)
    {
        return LoggingPrefix + "/" + Sanitize(fileName);
    }

    /// <summary>
    /// 资源对象按哈希前两位分目录，与官方布局一致——这样目录不会因为对象过多而退化。
    /// </summary>
    public string ObjectFile(string hash)
    {
        if (string.IsNullOrEmpty(hash) || hash.Length < 2)
        {
            return ObjectsPrefix + "/" + Sanitize(hash ?? string.Empty);
        }

        return ObjectsPrefix + "/" + hash.Substring(0, 2) + "/" + hash;
    }

    /// <summary>
    /// 库文件路径。优先采用元数据给出的 <c>downloads.artifact.path</c>——
    /// 它是权威布局，自己推导只作为元数据缺路径时的回落。
    /// </summary>
    public string LibraryFile(string? metadataPath, LibraryName? name, string rawName)
    {
        if (!string.IsNullOrEmpty(metadataPath))
        {
            return LibrariesPrefix + "/" + TrimSeparators(metadataPath!);
        }

        if (name == null)
        {
            // 坐标无法解析时退化为原样文件名：宁可路径难看，也不要凭空猜一个可能撞车的布局。
            return LibrariesPrefix + "/" + Sanitize(rawName);
        }

        string directory = name.GroupId.Replace('.', '/') + "/" + name.ArtifactId + "/" + name.Version;
        string file = name.ArtifactId + "-" + name.Version;

        if (name.HasClassifier)
        {
            file += "-" + name.Classifier;
        }

        file += "." + name.Extension;

        return LibrariesPrefix + "/" + directory + "/" + file;
    }

    private static string TrimSeparators(string value)
    {
        string normalized = value.Replace('\\', '/');
        return normalized.TrimStart('/');
    }

    private static string Sanitize(string value)
    {
        return string.IsNullOrEmpty(value) ? "unknown" : value.Replace('\\', '/').TrimStart('/');
    }
}
