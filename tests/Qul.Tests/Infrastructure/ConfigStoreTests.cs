using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Configuration;
using Qul.Infrastructure.IO;
using Qul.Infrastructure.Serialization;

namespace Qul.Tests.Infrastructure;

[TestClass]
public sealed class ConfigStoreTests
{
    private string? _sandbox;

    [TestCleanup]
    public void Cleanup()
    {
        if (_sandbox != null && Directory.Exists(_sandbox))
        {
            try
            {
                Directory.Delete(_sandbox, recursive: true);
            }
            catch (IOException)
            {
                // 清理失败不影响结论。
            }
        }
    }

    [TestMethod]
    public void EnsureCreated_CreatesEveryRequiredDirectory()
    {
        DataLayout layout = NewLayout();

        Assert.IsNull(layout.EnsureCreated(), "目录创建不应报错");
        Assert.IsTrue(Directory.Exists(layout.LogDirectory));
        Assert.IsTrue(Directory.Exists(layout.CacheMetaDirectory));
        Assert.IsTrue(Directory.Exists(layout.CacheLibrariesDirectory));
        Assert.IsTrue(Directory.Exists(layout.CacheObjectsDirectory));
        Assert.IsTrue(Directory.Exists(layout.NativesDirectory));
        Assert.IsTrue(Directory.Exists(layout.SecretsDirectory));
        Assert.IsTrue(Directory.Exists(layout.StateDirectory));
    }

    [TestMethod]
    public void Load_WithNoConfigFile_ReturnsDefaultsWithoutWarning()
    {
        ConfigStore store = new ConfigStore(NewLayout());

        ConfigLoadResult result = store.Load();

        Assert.IsNull(result.Warning);
        Assert.IsFalse(result.WasReset);
        Assert.IsFalse(result.IsReadOnly);
        Assert.IsTrue(File.Exists(store.FilePath), "首次运行必须把默认配置落盘，数据根从第一次启动起就可检查、可编辑");
        Assert.AreEqual(LauncherConfig.CurrentSchemaVersion, result.Config.SchemaVersion);
        Assert.AreEqual(IdentitySource.Offline, result.Config.Identity.Source, "默认身份来源必须是离线");
        Assert.AreEqual(JavaSelectionMode.Auto, result.Config.Java.Mode);
        Assert.IsFalse(result.Config.JavaRuntime.AutoDownload, "P10 能力默认必须关闭");
        Assert.AreEqual(ContentSourceKind.Official, result.Config.ContentSource.Kind, "MVP 只允许官方源");
        Assert.IsNull(result.Config.ContentSource.MirrorBaseUrl);
    }

    [TestMethod]
    public void Load_WithCorruptFile_BacksUpAndFallsBackToDefaults()
    {
        DataLayout layout = NewLayout();
        File.WriteAllText(layout.ConfigFile, "{ this is not json", new UTF8Encoding(false));

        ConfigLoadResult result = new ConfigStore(layout).Load();

        Assert.IsTrue(result.WasReset);
        Assert.AreEqual(ErrorCode.CfgParseFailed, result.Warning);
        Assert.IsNotNull(result.BackupPath);
        Assert.IsTrue(File.Exists(result.BackupPath!), "损坏的配置必须留档，不能直接丢掉");
        Assert.AreEqual(LauncherConfig.CurrentSchemaVersion, result.Config.SchemaVersion);
    }

    [TestMethod]
    public void Load_WithNonObjectRoot_TreatedAsCorrupt()
    {
        DataLayout layout = NewLayout();
        File.WriteAllText(layout.ConfigFile, "[1,2,3]", new UTF8Encoding(false));

        ConfigLoadResult result = new ConfigStore(layout).Load();

        Assert.IsTrue(result.WasReset);
        Assert.AreEqual(ErrorCode.CfgParseFailed, result.Warning);
    }

    [TestMethod]
    public void Load_WithNewerSchema_IsReadOnly()
    {
        DataLayout layout = NewLayout();
        File.WriteAllText(layout.ConfigFile, "{\"schemaVersion\": 99}", new UTF8Encoding(false));

        ConfigLoadResult result = new ConfigStore(layout).Load();

        Assert.IsTrue(result.IsReadOnly, "更新版本的配置必须只读，避免回写覆盖用户的新配置");
        Assert.AreEqual(ErrorCode.CfgVersionTooNew, result.Warning);
        Assert.AreEqual(99, result.Config.SchemaVersion);
    }

    [TestMethod]
    public void Load_IgnoresInvalidCertificateOptIn()
    {
        DataLayout layout = NewLayout();
        File.WriteAllText(
            layout.ConfigFile,
            "{\"schemaVersion\":1,\"network\":{\"proxyMode\":\"manual\",\"proxyAddress\":\"http://127.0.0.1:8080\",\"allowInvalidCertificate\":true}}",
            new UTF8Encoding(false));

        LauncherConfig config = new ConfigStore(layout).Load().Config;

        Assert.AreEqual(ProxyMode.Manual, config.Network.ProxyMode);
        Assert.AreEqual("http://127.0.0.1:8080", config.Network.ProxyAddress);
        Assert.IsFalse(config.Network.AllowInvalidCertificate, "证书绕过不存在开启入口，文件里写了 true 也必须被忽略");
    }

    [TestMethod]
    public void Save_ThenLoad_RoundTripsEveryValue()
    {
        DataLayout layout = NewLayout();
        ConfigStore store = new ConfigStore(layout);
        store.Load();

        LauncherConfig config = LauncherConfig.CreateDefault();
        config.Identity.Source = IdentitySource.Microsoft;
        config.Identity.OfflineUserName = "测试玩家 名";
        config.Java.Mode = JavaSelectionMode.Manual;
        config.Java.ManualPath = @"C:\Program Files\Java\jdk-21.0.12.1\bin\java.exe";
        config.Memory.MaxMb = 4096;
        config.Launch.GameDirectory = @"D:\games\我的 世界";
        config.Launch.ExtraJvmArgs.Add("-XX:+UseG1GC");
        config.Launch.ExtraJvmArgs.Add("-Dfile.encoding=UTF-8");
        config.Launch.ServerQuickConnect = "example.org";
        config.Launch.RecentServers.Add("example.org");
        config.Diagnostics.LogLevel = LogLevel.Debug;

        Assert.IsNull(store.Save(config));

        LauncherConfig reloaded = new ConfigStore(layout).Load().Config;

        Assert.AreEqual(IdentitySource.Microsoft, reloaded.Identity.Source);
        Assert.AreEqual("测试玩家 名", reloaded.Identity.OfflineUserName);
        Assert.AreEqual(JavaSelectionMode.Manual, reloaded.Java.Mode);
        Assert.AreEqual(config.Java.ManualPath, reloaded.Java.ManualPath);
        Assert.AreEqual(4096, reloaded.Memory.MaxMb);
        Assert.AreEqual(config.Launch.GameDirectory, reloaded.Launch.GameDirectory);
        CollectionAssert.AreEqual(config.Launch.ExtraJvmArgs, reloaded.Launch.ExtraJvmArgs);
        Assert.AreEqual("example.org", reloaded.Launch.ServerQuickConnect);
        CollectionAssert.AreEqual(config.Launch.RecentServers, reloaded.Launch.RecentServers);
        Assert.AreEqual(LogLevel.Debug, reloaded.Diagnostics.LogLevel);
    }

    [TestMethod]
    public void Save_PreservesUnknownFieldsForForwardCompatibility()
    {
        DataLayout layout = NewLayout();
        File.WriteAllText(
            layout.ConfigFile,
            "{\"schemaVersion\":1,\"futureFeature\":{\"flag\":true},\"identity\":{\"source\":\"offline\"}}",
            new UTF8Encoding(false));

        ConfigStore store = new ConfigStore(layout);
        LauncherConfig config = store.Load().Config;

        Assert.IsNull(store.Save(config));

        JsonObject written = JsonValue.Parse(File.ReadAllText(layout.ConfigFile, Encoding.UTF8)).RequireObject();

        Assert.IsTrue(written.ContainsKey("futureFeature"), "本程序不认识的字段必须原样保留");
        Assert.IsTrue(written.GetObject("futureFeature")!.GetBoolean("flag"));
    }

    [TestMethod]
    public void Save_IsAtomicAndLeavesNoTempFile()
    {
        DataLayout layout = NewLayout();
        ConfigStore store = new ConfigStore(layout);
        store.Load();

        Assert.IsNull(store.Save(LauncherConfig.CreateDefault()));

        Assert.IsTrue(File.Exists(layout.ConfigFile));
        Assert.IsFalse(File.Exists(layout.ConfigFile + ".tmp"), "原子写不应留下临时文件");
    }

    [TestMethod]
    public void ConfigFile_NeverContainsCredentialFields()
    {
        // 这是红线级别的约束：配置文件里永远不许出现凭据字段。
        DataLayout layout = NewLayout();
        ConfigStore store = new ConfigStore(layout);
        store.Load();
        store.Save(LauncherConfig.CreateDefault());

        string text = File.ReadAllText(layout.ConfigFile, Encoding.UTF8);

        foreach (string forbidden in new[] { "accessToken", "refreshToken", "password", "secret", "clientSecret" })
        {
            Assert.IsFalse(
                text.IndexOf(forbidden, StringComparison.OrdinalIgnoreCase) >= 0,
                "配置文件出现了凭据字段：" + forbidden);
        }
    }

    private DataLayout NewLayout()
    {
        _sandbox = Path.Combine(Path.GetTempPath(), "qinmo-tests", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_sandbox);

        DataLayout layout = DataLayout.Resolve(_sandbox, Path.GetTempPath());
        layout.EnsureCreated();
        return layout;
    }
    [TestMethod]
    public void RoundTrip_KeepsTheDownloadConcurrency()
    {
        // 并发数既然是配置项，就必须真的能存下来、读回来。
        // 漏掉映射的话它会静静地回到默认值，而用户改过的设置看起来"没生效"。
        // 走真实的 Save/Load，而不是直接调映射函数——
        // 要证明的是"用户改过的设置真的落盘、下次启动还在"。
        DataLayout layout = NewLayout();
        layout.EnsureCreated();

        ConfigStore store = new ConfigStore(layout);
        LauncherConfig config = store.Load().Config;
        config.Network.MaxConcurrency = 12;

        Assert.IsNull(store.Save(config), "保存不该报错");

        LauncherConfig reloaded = new ConfigStore(layout).Load().Config;

        Assert.AreEqual(12, reloaded.Network.MaxConcurrency);
    }

    [TestMethod]
    public void Default_KeepsTheMeasuredDownloadConcurrency()
    {
        // 64 是量出来的，不是拍的。
        // 26.3（5224 项 / 586 MB）三明治式对照：32 得 1667、1760 KB/s，
        // 64 夹在中间得 2215 KB/s。
        // 谁要改这个默认值，请先重跑一次那个 A/B。
        Assert.AreEqual(64, new LauncherConfig().Network.MaxConcurrency);
    }
    [TestMethod]
    public void ProxyPolicy_MapsTheThreeModesToTheTransportsThreeStates()
    {
        // **这条用例守的是一个"用户设置被静默忽略"的缺陷。**
        //
        // 配置里有三种选择，而传输层需要的是另一种三态字符串：
        //   null = 跟随系统、空串 = 直连、其他 = 显式地址
        // 先前这段换算写死在下载选项里，而那个选项**从没有人从配置赋值**——
        // 于是"手动代理"与"直连"静默失效，连"代理地址非法"的错误码都不可达。
        Assert.IsNull(ProxyPolicy.Describe(ProxyMode.System, null), "跟随系统必须映射成 null");
        Assert.IsNull(ProxyPolicy.Describe(ProxyMode.System, "http://ignored"), "跟随系统时地址应被忽略");

        Assert.AreEqual(string.Empty, ProxyPolicy.Describe(ProxyMode.Direct, null), "直连必须映射成空串");
        Assert.AreEqual(string.Empty, ProxyPolicy.Describe(ProxyMode.Direct, "http://ignored"), "直连时必须忽略地址");

        Assert.AreEqual("http://127.0.0.1:8080", ProxyPolicy.Describe(ProxyMode.Manual, "  http://127.0.0.1:8080  "), "手动代理应保留并去掉首尾空白");

        // 选了手动却没填地址：当作直连，而不是静默回落到系统代理。
        Assert.AreEqual(string.Empty, ProxyPolicy.Describe(ProxyMode.Manual, null));
        Assert.AreEqual(string.Empty, ProxyPolicy.Describe(ProxyMode.Manual, "   "));

        // **三态必须互不相同**：只有 null 与空串区分开，"关掉代理"这个意图才表达得出来。
        Assert.AreNotEqual(ProxyPolicy.Describe(ProxyMode.System, null), ProxyPolicy.Describe(ProxyMode.Direct, null));
    }
}