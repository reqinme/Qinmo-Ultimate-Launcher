using System;
using System.Collections.Generic;

namespace Qul.Domain.Downloads;

/// <summary>下载源偏好。</summary>
public enum DownloadSourcePreference
{
    /// <summary>只用官方源。</summary>
    OfficialOnly = 0,

    /// <summary>官方优先，失败切镜像。</summary>
    OfficialFirst = 1,

    /// <summary>镜像优先，失败切官方。</summary>
    MirrorFirst = 2,
}

/// <summary>
/// 下载源策略：把官方 URL 映射成"官方 + 镜像"的有序候选列表。
///
/// **安全前提：镜像只提供字节，摘要永远只来自官方元数据。**
/// 一个恶意镜像无法伪造出内容与官方 SHA-1 相符的文件，
/// 所以引入镜像不降低完整性保证——校验不过就丢弃，换下一个源重试。
///
/// **主机表由本机逐个探测确定，不是抄来的**：对每个官方主机实测镜像是否有对应路径，没有的（如 `launcher.mojang.com` 的 `/v1/objects/` 形态）一律不含——留着只会造出一个**必然失败**的备用源。设计层面（"官方优先、镜像兜底、摘要只认官方"）与 PCL2 等启动器的通行做法一致，**但不含其任何代码**。
/// </summary>
public static class DownloadSourcePolicy
{
    public const string BmclapiBase = "https://bmclapi2.bangbang93.com";

    /// <summary>
    /// 会被替换的官方主机。**长的写在前面**——`.../releases` 必须先于 `...`
    /// 匹配，否则会被短前缀吃掉。
    /// </summary>
    private static readonly string[] OfficialHosts =
    {
        "https://maven.neoforged.net/releases",
        "https://maven.neoforged.net",
        "https://maven.fabricmc.net",
        "https://maven.minecraftforge.net",
        "https://piston-data.mojang.com",
        "https://piston-meta.mojang.com",
        // **刻意不含 https://launcher.mojang.com。**
        // 它的路径形态是 /v1/objects/<hash>/<name>，而 bmclapi 没有对应的路径：
        // 实测 https://launcher.mojang.com/v1/objects/50c9…/client-1.7.xml 返回 200，
        // 而替换出来的 https://bmclapi2.bangbang93.com/v1/objects/50c9…/client-1.7.xml 返回 404。
        // 留着它只会造出一个**必然失败**的"备用源"：白占一次重试，
        // 还会让日志里的"换源"看起来发生过——那是假的。
        // AddIfMapped 能挡住"没匹配到主机时原样返回"，但挡不住映射表里的路径本身不存在。
        "https://launchermeta.mojang.com",
        "https://libraries.minecraft.net",
        "https://resources.download.minecraft.net",
        "http://resources.download.minecraft.net",
    };

    /// <summary>这些是 Mod 加载器自己的 maven：镜像有对应路径，但没有"原版源"可言。</summary>
    private static readonly string[] ModLoaderMarkers = { "minecraftforge", "fabricmc", "neoforged" };

    /// <summary>
    /// 返回按优先级排好的候选 URL。第一个是首选，其余是失败后的备用。
    /// 至少会返回一个元素；官方源为空时返回空列表。
    /// </summary>
    public static IReadOnlyList<string> Order(
        string officialUrl,
        DownloadItemKind kind,
        DownloadSourcePreference preference)
    {
        if (string.IsNullOrWhiteSpace(officialUrl))
        {
            return Array.Empty<string>();
        }

        List<string> mirror = MirrorVariants(officialUrl, kind);

        if (preference == DownloadSourcePreference.OfficialOnly || mirror.Count == 0)
        {
            // 映射不出镜像就老老实实只用原地址——绝不凭空造一个"备用源"。
            return new[] { officialUrl };
        }

        // 加载器的库只存在于各自的 maven 上，"原版源"对它们没有意义。
        if (IsModLoaderHost(officialUrl))
        {
            return mirror;
        }

        List<string> ordered = new List<string>(mirror.Count + 1);

        if (preference == DownloadSourcePreference.MirrorFirst)
        {
            ordered.AddRange(mirror);
            ordered.Add(officialUrl);
        }
        else
        {
            ordered.Add(officialUrl);
            ordered.AddRange(mirror);
        }

        return ordered;
    }

    /// <summary>
    /// 官方 URL 对应的镜像候选（按优先级）。
    /// **没有任何主机被替换时返回空列表**——那说明这个 URL 我们映射不了，
    /// 而不是"它的镜像就是它自己"。
    /// </summary>
    public static List<string> MirrorVariants(string officialUrl, DownloadItemKind kind)
    {
        List<string> result = new List<string>(2);

        if (string.IsNullOrWhiteSpace(officialUrl))
        {
            return result;
        }

        switch (kind)
        {
            case DownloadItemKind.AssetObject:
                AddIfMapped(result, Replace(officialUrl, "/assets"));
                break;

            case DownloadItemKind.Library:
            case DownloadItemKind.LibraryClassifier:
                // 两个前缀都试：bmclapi 对 maven 与 libraries 都有镜像路径，
                // 只试一个的话，没命中的那个会让整条库下载失败。
                AddIfMapped(result, Replace(officialUrl, "/maven"));
                AddIfMapped(result, Replace(officialUrl, "/libraries"));
                break;

            default:
                AddIfMapped(result, Replace(officialUrl, string.Empty));
                break;
        }

        return result;
    }

    private static bool IsModLoaderHost(string url)
    {
        for (int i = 0; i < ModLoaderMarkers.Length; i++)
        {
            if (url.IndexOf(ModLoaderMarkers[i], StringComparison.OrdinalIgnoreCase) >= 0)
            {
                return true;
            }
        }

        return false;
    }

    private static string Replace(string url, string suffix)
    {
        string result = url;

        for (int i = 0; i < OfficialHosts.Length; i++)
        {
            result = result.Replace(OfficialHosts[i], BmclapiBase + suffix);
        }

        return result;
    }

    /// <summary>
    /// 只有真正发生了替换才收进来。
    /// 早先直接收 <c>Replace</c> 的返回值，而它在没匹配到主机时原样返回输入，
    /// 结果给一个映射不了的地址凭空造出了"备用源"——等于用一个必然失败的重复尝试
    /// 占掉一次重试机会。
    /// </summary>
    private static void AddIfMapped(List<string> list, string candidate)
    {
        if (!string.IsNullOrWhiteSpace(candidate)
            && candidate.IndexOf(BmclapiBase, StringComparison.OrdinalIgnoreCase) >= 0
            && !list.Contains(candidate))
        {
            list.Add(candidate);
        }
    }
}
