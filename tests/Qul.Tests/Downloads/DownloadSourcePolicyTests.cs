using System.Collections.Generic;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Downloads;

namespace Qul.Tests.Downloads;

/// <summary>
/// 下载源策略：官方与镜像的候选顺序，以及换源后的完整性保证。
///
/// **前提**：镜像只提供字节，SHA-1 永远来自官方元数据。
/// 所以"用镜像"不改变完整性——校验不过就丢弃并换下一个源。
/// </summary>
[TestClass]
public sealed class DownloadSourcePolicyTests
{
    private const string AssetUrl = "https://resources.download.minecraft.net/bd/bdf48ef6b5d0d23bbb02e17d04865216179f510a";
    private const string LibraryUrl = "https://libraries.minecraft.net/com/google/guava/guava/21.0/guava-21.0.jar";
    private const string MetaUrl = "https://piston-meta.mojang.com/v1/packages/abc/26.3.json";

    // ================= 顺序 =================

    [TestMethod]
    public void OfficialFirst_PutsOfficialAheadOfTheMirror()
    {
        IReadOnlyList<string> ordered = DownloadSourcePolicy.Order(
            AssetUrl, DownloadItemKind.AssetObject, DownloadSourcePreference.OfficialFirst);

        Assert.AreEqual(2, ordered.Count);
        Assert.AreEqual(AssetUrl, ordered[0], "官方优先时首选必须是官方");
        StringAssert.Contains(ordered[1], "bmclapi2.bangbang93.com");
    }

    [TestMethod]
    public void MirrorFirst_PutsTheMirrorAheadOfOfficial()
    {
        IReadOnlyList<string> ordered = DownloadSourcePolicy.Order(
            AssetUrl, DownloadItemKind.AssetObject, DownloadSourcePreference.MirrorFirst);

        StringAssert.Contains(ordered[0], "bmclapi2.bangbang93.com");
        Assert.AreEqual(AssetUrl, ordered[ordered.Count - 1], "官方必须仍在列表里作为兜底");
    }

    [TestMethod]
    public void OfficialOnly_YieldsExactlyOneSource()
    {
        IReadOnlyList<string> ordered = DownloadSourcePolicy.Order(
            AssetUrl, DownloadItemKind.AssetObject, DownloadSourcePreference.OfficialOnly);

        Assert.AreEqual(1, ordered.Count);
        Assert.AreEqual(AssetUrl, ordered[0]);
    }

    // ================= 路径映射 =================

    [TestMethod]
    public void AssetObjects_MapUnderTheAssetsPrefix()
    {
        List<string> mirror = DownloadSourcePolicy.MirrorVariants(AssetUrl, DownloadItemKind.AssetObject);

        Assert.AreEqual(1, mirror.Count);
        Assert.AreEqual(
            "https://bmclapi2.bangbang93.com/assets/bd/bdf48ef6b5d0d23bbb02e17d04865216179f510a",
            mirror[0]);
    }

    [TestMethod]
    public void Libraries_OfferBothMavenAndLibrariesPrefixes()
    {
        // bmclapi 对 maven 与 libraries 都做了镜像，两个都得试——
        // 只试一个的话，命中不了的那个前缀会让整条库下载失败。
        List<string> mirror = DownloadSourcePolicy.MirrorVariants(LibraryUrl, DownloadItemKind.Library);

        Assert.AreEqual(2, mirror.Count);
        StringAssert.Contains(mirror[0], "/maven/");
        StringAssert.Contains(mirror[1], "/libraries/");
    }

    [TestMethod]
    public void MetaFiles_MapToTheMirrorRoot()
    {
        List<string> mirror = DownloadSourcePolicy.MirrorVariants(MetaUrl, DownloadItemKind.AssetIndex);

        Assert.AreEqual(1, mirror.Count);
        StringAssert.StartsWith(mirror[0], "https://bmclapi2.bangbang93.com/");
        StringAssert.Contains(mirror[0], "/v1/packages/abc/26.3.json");
    }

    // ================= 边界 =================

    [TestMethod]
    public void ModLoaderLibraries_DropTheOfficialSourceEntirely()
    {
        // Forge / Fabric / NeoForge 的库在各自的 maven 上，官方源对它们没有意义。
        // 把它们留在候选里只会浪费一次注定失败的尝试。
        string fabric = "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.15.0/fabric-loader-0.15.0.jar";

        IReadOnlyList<string> ordered = DownloadSourcePolicy.Order(
            fabric, DownloadItemKind.Library, DownloadSourcePreference.OfficialFirst);

        Assert.IsTrue(ordered.Count > 0);
        for (int i = 0; i < ordered.Count; i++)
        {
            Assert.AreNotEqual(fabric, ordered[i], "加载器自身的库不该把原地址当候选");
        }
    }

    [TestMethod]
    public void UnknownHosts_AreLeftAlone()
    {
        const string custom = "https://example.invalid/some/file.jar";

        IReadOnlyList<string> ordered = DownloadSourcePolicy.Order(
            custom, DownloadItemKind.Library, DownloadSourcePreference.MirrorFirst);

        Assert.AreEqual(1, ordered.Count, "映射不出镜像时不该凭空造一个");
        Assert.AreEqual(custom, ordered[0]);
    }

    [TestMethod]
    public void EmptyUrl_YieldsNothing()
    {
        Assert.AreEqual(0, DownloadSourcePolicy.Order(string.Empty, DownloadItemKind.Library, DownloadSourcePreference.MirrorFirst).Count);
    }
    [TestMethod]
    public void LauncherMojang_ProducesNoMirrorCandidateBecauseBmclapiHasNoSuchPath()
    {
        // **实测出来的事实，不是推测。**
        //   https://launcher.mojang.com/v1/objects/50c9…/client-1.7.xml          -> 200
        //   https://bmclapi2.bangbang93.com/v1/objects/50c9…/client-1.7.xml      -> 404
        //
        // bmclapi 没有 /v1/objects/ 这个路径。映射表里留着这个主机，
        // 只会造出一个**必然失败**的"备用源"：白占一次重试，
        // 还让日志里的"换源"看起来发生过——那是假的。
        const string Url =
            "https://launcher.mojang.com/v1/objects/50c9cc4af6d853d9fc137c84bcd153e2bd3a9a82/client-1.7.xml";

        List<string> mirror = DownloadSourcePolicy.MirrorVariants(Url, DownloadItemKind.LoggingConfig);

        Assert.AreEqual(0, mirror.Count, "bmclapi 没有对应路径，就不该造备用源");

        IReadOnlyList<string> ordered = DownloadSourcePolicy.Order(
            Url, DownloadItemKind.LoggingConfig, DownloadSourcePreference.MirrorFirst);

        Assert.AreEqual(1, ordered.Count);
        Assert.AreEqual(Url, ordered[0], "唯一候选必须还是官方地址");
    }
}