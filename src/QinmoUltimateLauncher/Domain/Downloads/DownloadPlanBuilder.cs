using System;
using System.Collections.Generic;
using Qul.Domain.Assets;
using Qul.Domain.Metadata;

namespace Qul.Domain.Downloads;

/// <summary>
/// 把一份已合并的版本元数据变成下载计划。
///
/// 这里是**纯逻辑**：不做网络、不碰文件系统，产出的是相对路径与 URL。
/// 因此"某版本到底要下多少文件、每个文件的校验值是多少"可以在没有网络、没有磁盘的
/// 情况下被完整断言——这正是 P1 与 P2 之间最重要的一道交叉校验。
/// </summary>
public sealed class DownloadPlanBuilder
{
    /// <summary>官方资源对象基址。这是元数据里唯一没有、必须由启动器补全的地址。</summary>
    public const string OfficialObjectsBaseUrl = "https://resources.download.minecraft.net";

    private readonly CachePathConventions _paths;

    public DownloadPlanBuilder(CachePathConventions? paths = null)
    {
        _paths = paths ?? new CachePathConventions();
    }

    /// <summary>
    /// 资源对象基址。做成可设置项是为了让 P10 的镜像源能替换它——
    /// 但注意：它只影响**从哪取**，不影响校验基准。
    /// </summary>
    public string ObjectsBaseUrl { get; set; } = OfficialObjectsBaseUrl;

    /// <summary>
    /// 组装下载计划。
    /// <paramref name="preference"/> 决定每个条目的候选源顺序。
    /// 默认官方优先、镜像兜底。**这是实测结论，不是保守选择**：
    /// 本机实测官方 1296 ms/文件、bmclapi 10180 ms/文件，镜像反而慢约 8 倍。
    /// 而换源机制保证官方不可用时仍能走镜像——
    /// 于是"官方优先"同时拿到了两边的最大收益，不需要赌哪一个更快。
    /// 摘要始终来自官方元数据，所以走镜像不降低完整性保证。
    /// </summary>
    public DownloadPlan Build(
        VersionDetail version,
        EnvironmentProfile environment,
        AssetIndex? assetIndex = null,
        DownloadSourcePreference preference = DownloadSourcePreference.OfficialFirst)
    {
        if (version == null)
        {
            throw new ArgumentNullException(nameof(version));
        }

        if (environment == null)
        {
            throw new ArgumentNullException(nameof(environment));
        }

        string versionId = string.IsNullOrEmpty(version.Id) ? "unknown" : version.Id;
        Accumulator accumulator = new Accumulator { Preference = preference };

        AddClient(version, versionId, accumulator);
        AddAssetIndex(version, accumulator);
        AddLogging(version, accumulator);
        AddLibraries(version, environment, accumulator);
        AddAssetObjects(assetIndex, accumulator);

        return new DownloadPlan
        {
            Name = versionId,
            Items = accumulator.Items,
            DuplicatePaths = accumulator.Duplicates,
            ConflictingPaths = accumulator.Conflicts,
        };
    }

    private void AddClient(VersionDetail version, string versionId, Accumulator accumulator)
    {
        DownloadRef? client = version.ClientDownload;
        if (client == null)
        {
            return;
        }

        accumulator.Add(new DownloadItem
        {
            Kind = DownloadItemKind.ClientJar,
            Url = client.Url,
            Sha1 = client.Sha1,
            Size = client.Size,
            RelativePath = _paths.ClientJarFile(versionId),
        });
    }

    private void AddAssetIndex(VersionDetail version, Accumulator accumulator)
    {
        AssetIndexRef? index = version.AssetIndex;
        if (index == null || string.IsNullOrEmpty(index.Url))
        {
            return;
        }

        accumulator.Add(new DownloadItem
        {
            Kind = DownloadItemKind.AssetIndex,
            Url = index.Url!,
            Sha1 = index.Sha1,
            Size = index.Size,
            RelativePath = _paths.AssetIndexFile(index.Id),
        });
    }

    private void AddLogging(VersionDetail version, Accumulator accumulator)
    {
        DownloadRef? file = version.Logging?.File;
        if (file == null)
        {
            return;
        }

        // logging.client.file 用的是 id 而不是 path，这里从 URL 末段取文件名，稳定且无需猜。
        string fileName = ExtractFileName(file.Url);
        if (fileName.Length == 0)
        {
            return;
        }

        accumulator.Add(new DownloadItem
        {
            Kind = DownloadItemKind.LoggingConfig,
            Url = file.Url,
            Sha1 = file.Sha1,
            Size = file.Size,
            RelativePath = _paths.LoggingFile(fileName),
        });
    }

    private void AddLibraries(VersionDetail version, EnvironmentProfile environment, Accumulator accumulator)
    {
        for (int i = 0; i < version.Libraries.Count; i++)
        {
            LibraryRef library = version.Libraries[i];

            // 规则先过滤：不适用于本平台的库连下载都不该发生。
            if (!RuleEvaluator.IsAllowed(library.Rules, environment))
            {
                continue;
            }

            if (library.Artifact != null)
            {
                accumulator.Add(new DownloadItem
                {
                    Kind = DownloadItemKind.Library,
                    Url = library.Artifact.Url,
                    Sha1 = library.Artifact.Sha1,
                    Size = library.Artifact.Size,
                    RelativePath = _paths.LibraryFile(library.Artifact.Path, library.Name, library.RawName),
                });
            }

            // 旧式 natives：由 natives 映射选出本平台的 classifier。
            // 模板值会被替换，悬空引用会被跳过（返回 null）。
            // 注意 26.x 完全不经过这条路径——它的 natives 是坐标自带 classifier 的独立库条目，
            // 已经由上面的 artifact 分支处理掉了。
            string? classifierKey = NativeClassifierSelector.Select(library, environment);
            if (classifierKey == null
                || !library.Classifiers.TryGetValue(classifierKey, out DownloadRef? classifier))
            {
                continue;
            }

            accumulator.Add(new DownloadItem
            {
                Kind = DownloadItemKind.LibraryClassifier,
                Url = classifier.Url,
                Sha1 = classifier.Sha1,
                Size = classifier.Size,
                RelativePath = _paths.LibraryFile(classifier.Path, library.Name, library.RawName),
            });
        }
    }

    private void AddAssetObjects(AssetIndex? assetIndex, Accumulator accumulator)
    {
        if (assetIndex == null)
        {
            return;
        }

        // 按名字排序：计划必须可复现，字典枚举顺序不能参与其中。
        List<AssetObject> ordered = new List<AssetObject>(assetIndex.Objects.Values);
        ordered.Sort(CompareByName);

        string baseUrl = (ObjectsBaseUrl ?? OfficialObjectsBaseUrl).TrimEnd('/');

        for (int i = 0; i < ordered.Count; i++)
        {
            AssetObject obj = ordered[i];
            if (obj.Hash.Length < 2)
            {
                continue;
            }

            string prefix = obj.Hash.Substring(0, 2);

            accumulator.Add(new DownloadItem
            {
                Kind = DownloadItemKind.AssetObject,
                Url = baseUrl + "/" + prefix + "/" + obj.Hash,
                Sha1 = obj.Hash,
                Size = obj.Size,
                RelativePath = _paths.ObjectFile(obj.Hash),
            });
        }
    }

    private static int CompareByName(AssetObject left, AssetObject right)
    {
        return string.CompareOrdinal(left.Name, right.Name);
    }

    private static string ExtractFileName(string? url)
    {
        if (string.IsNullOrWhiteSpace(url))
        {
            return string.Empty;
        }

        string text = url!;
        int cut = text.IndexOfAny(new[] { '?', '#' });
        if (cut >= 0)
        {
            text = text.Substring(0, cut);
        }

        int slash = text.LastIndexOf('/');
        return slash >= 0 ? text.Substring(slash + 1) : text;
    }

    /// <summary>
    /// 计划累加器。按相对路径折叠重复项，并区分"无害重复"与"内容冲突"。
    ///
    /// 重复是真实存在的，不能一律当错误：
    ///   - 资源对象内容寻址，不同逻辑名可能同哈希（实测 1.7.10 的 686 个名字只对应 674 份内容）；
    ///   - 官方元数据里有指向同一 artifact 的重复库条目（实测 1.12.2 的 text2speech:1.10.3）。
    /// 但**同路径不同内容**就是真问题：并发下载会互相覆盖，留下哪个全看运气。
    /// </summary>
    private sealed class Accumulator
    {
        private readonly Dictionary<string, DownloadItem> _byPath =
            new Dictionary<string, DownloadItem>(StringComparer.Ordinal);

        public List<DownloadItem> Items { get; } = new List<DownloadItem>();

        public List<string> Duplicates { get; } = new List<string>();

        public List<string> Conflicts { get; } = new List<string>();

        public DownloadSourcePreference Preference { get; set; } = DownloadSourcePreference.OfficialFirst;

        public void Add(DownloadItem item)
        {
            ApplySourcePolicy(item);
            AddCore(item);
        }

        private void ApplySourcePolicy(DownloadItem item)
        {
            IReadOnlyList<string> ordered = DownloadSourcePolicy.Order(item.Url, item.Kind, Preference);

            if (ordered.Count == 0)
            {
                return;
            }

            item.Url = ordered[0];

            if (ordered.Count > 1)
            {
                List<string> rest = new List<string>(ordered.Count - 1);
                for (int i = 1; i < ordered.Count; i++)
                {
                    rest.Add(ordered[i]);
                }

                item.FallbackUrls = rest;
            }
        }

        private void AddCore(DownloadItem item)
        {
            if (_byPath.TryGetValue(item.RelativePath, out DownloadItem? existing))
            {
                Duplicates.Add(item.RelativePath);

                bool sameHash = string.Equals(existing.Sha1, item.Sha1, StringComparison.OrdinalIgnoreCase);
                bool sameUrl = string.Equals(existing.Url, item.Url, StringComparison.Ordinal);

                if (!sameHash || !sameUrl)
                {
                    Conflicts.Add(item.RelativePath);
                }

                return;
            }

            _byPath[item.RelativePath] = item;
            Items.Add(item);
        }
    }
}
