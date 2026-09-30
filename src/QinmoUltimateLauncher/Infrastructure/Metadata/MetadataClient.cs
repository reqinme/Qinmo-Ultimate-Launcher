using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using System.Threading;
using Qul.Application.Ports;
using Qul.Domain.Assets;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Domain.Metadata;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.Serialization;

namespace Qul.Infrastructure.Metadata;

/// <summary>官方地址。MVP 只有官方源；镜像在 P10 作为可选能力引入。</summary>
public static class OfficialEndpoints
{
    public const string VersionManifest = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

    public const string ResourcesBase = "https://resources.download.minecraft.net";
}

/// <summary>
/// 取回并解析元数据，带磁盘缓存。
///
/// 与下载引擎的分工：引擎负责**文件内容**（带校验、续传、并发），
/// 这里负责**清单本身**——因为要先拿到清单才能算出该下哪些文件，属于先有鸡还是先有蛋的那一环。
/// 因此它只用最朴素的 GET，但保留重试与缓存。
/// </summary>
public sealed class MetadataClient
{
    private const int MaxAttempts = 3;

    private readonly IHttpTransport _transport;
    private readonly SessionLog _log;

    public MetadataClient(IHttpTransport transport, SessionLog? log = null)
    {
        _transport = transport ?? throw new ArgumentNullException(nameof(transport));
        _log = log ?? SessionLog.Null;
    }

    public string VersionManifestUrl { get; set; } = OfficialEndpoints.VersionManifest;

    public VersionManifest FetchManifest(string cacheFilePath, bool forceRefresh = false)
    {
        return VersionMetadataParser.ParseManifest(Load(VersionManifestUrl, cacheFilePath, forceRefresh));
    }

    public VersionDetail FetchVersion(string url, string versionId, string cacheFilePath, bool forceRefresh = false)
    {
        return VersionMetadataParser.ParseVersion(Load(url, cacheFilePath, forceRefresh), versionId);
    }

    public AssetIndex FetchAssetIndex(string url, string assetIndexId, string cacheFilePath, bool forceRefresh = false)
    {
        return VersionMetadataParser.ParseAssetIndex(Load(url, cacheFilePath, forceRefresh), assetIndexId);
    }

    /// <summary>缓存优先；缓存缺失或为空时下载并原子落盘。</summary>
    public string Load(string url, string cacheFilePath, bool forceRefresh)
    {
        if (!forceRefresh && File.Exists(cacheFilePath))
        {
            try
            {
                string cached = File.ReadAllText(cacheFilePath, Encoding.UTF8);
                if (cached.Trim().Length > 0)
                {
                    _log.Info("meta", "cache hit", cacheFilePath);
                    return cached;
                }
            }
            catch (IOException)
            {
                // 读不了就当下没有，重新下载。
            }
        }

        string text = Download(url);

        try
        {
            string? directory = Path.GetDirectoryName(cacheFilePath);
            if (!string.IsNullOrEmpty(directory))
            {
                Directory.CreateDirectory(directory!);
            }

            string temp = cacheFilePath + ".tmp";
            File.WriteAllText(temp, text, new UTF8Encoding(false));

            if (File.Exists(cacheFilePath))
            {
                File.Delete(cacheFilePath);
            }

            File.Move(temp, cacheFilePath);
        }
        catch (IOException)
        {
            // 写不进缓存不影响本次使用。
        }
        catch (UnauthorizedAccessException)
        {
        }

        return text;
    }

    private string Download(string url)
    {
        ErrorCode lastError = ErrorCode.NetUnreachable;

        for (int attempt = 1; attempt <= MaxAttempts; attempt++)
        {
            try
            {
                using (HttpFetchResponse response = _transport.Fetch(
                           new HttpFetchRequest { Url = url, Timeout = TimeSpan.FromSeconds(60) },
                           CancellationToken.None))
                {
                    if (response.Status != HttpFetchStatus.Success && response.Status != HttpFetchStatus.PartialContent)
                    {
                        throw new LauncherException(
                            response.Status == HttpFetchStatus.NotFound
                                ? ErrorCode.NetResourceMissing
                                : ErrorCode.NetHttpStatus,
                            "metadata fetch failed with status " + response.StatusCode);
                    }

                    using (StreamReader reader = new StreamReader(response.Content, Encoding.UTF8))
                    {
                        return reader.ReadToEnd();
                    }
                }
            }
            catch (LauncherException ex)
            {
                lastError = ex.Code;

                // 资源不存在重试没有意义。
                if (ex.Code == ErrorCode.NetResourceMissing || attempt == MaxAttempts)
                {
                    throw;
                }
            }

            Thread.Sleep(300 * attempt);
        }

        throw new LauncherException(lastError, "metadata fetch exhausted retries");
    }
}
