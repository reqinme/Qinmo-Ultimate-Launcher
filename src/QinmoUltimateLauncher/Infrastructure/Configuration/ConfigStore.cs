using System;
using System.Globalization;
using System.IO;
using System.Text;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.IO;
using Qul.Infrastructure.Serialization;

namespace Qul.Infrastructure.Configuration;

/// <summary>配置装载结果。永远返回一份可用配置——配置问题不得阻止启动。</summary>
public sealed class ConfigLoadResult
{
    public ConfigLoadResult(LauncherConfig config, bool wasReset, bool isReadOnly, string? backupPath, ErrorCode? warning)
    {
        Config = config;
        WasReset = wasReset;
        IsReadOnly = isReadOnly;
        BackupPath = backupPath;
        Warning = warning;
    }

    public LauncherConfig Config { get; }

    /// <summary>配置损坏，已备份原文件并重置为默认值。</summary>
    public bool WasReset { get; }

    /// <summary>配置来自更新版本，本次只读，保存被禁止。</summary>
    public bool IsReadOnly { get; }

    public string? BackupPath { get; }

    public ErrorCode? Warning { get; }
}

/// <summary>config.json 与领域模型之间的手写映射。未知字段在装回时原样保留。</summary>
internal static class ConfigMapper
{
    public static JsonObject ToJson(LauncherConfig config)
    {
        JsonObject root = new JsonObject();

        root.Set("schemaVersion", config.SchemaVersion);

        root.Set("identity", new JsonObject()
            .Set("source", IdentitySourceText(config.Identity.Source))
            .Set("offlineUserName", config.Identity.OfflineUserName)
            .Set("lastAccountKey", config.Identity.LastAccountKey));

        root.Set("java", new JsonObject()
            .Set("mode", config.Java.Mode == JavaSelectionMode.Manual ? "manual" : "auto")
            .Set("manualPath", config.Java.ManualPath));

        root.Set("memory", new JsonObject()
            .SetNullableInt("maxMb", config.Memory.MaxMb));

        JsonArray extraArgs = new JsonArray();
        foreach (string arg in config.Launch.ExtraJvmArgs)
        {
            extraArgs.Add(arg);
        }

        JsonArray recentServers = new JsonArray();
        foreach (string server in config.Launch.RecentServers)
        {
            recentServers.Add(server);
        }

        root.Set("launch", new JsonObject()
            .Set("gameDirectory", config.Launch.GameDirectory)
            .Set("extraJvmArgs", extraArgs)
            .Set("serverQuickConnect", config.Launch.ServerQuickConnect)
            .Set("recentServers", recentServers));

        root.Set("network", new JsonObject()
            .Set("proxyMode", ProxyModeText(config.Network.ProxyMode))
            .Set("proxyAddress", config.Network.ProxyAddress)
            .Set("allowInvalidCertificate", false));

        root.Set("contentSource", new JsonObject()
            .Set("kind", config.ContentSource.Kind == ContentSourceKind.Mirror ? "mirror" : "official")
            .Set("mirrorBaseUrl", config.ContentSource.MirrorBaseUrl));

        root.Set("javaRuntime", new JsonObject()
            .Set("autoDownload", config.JavaRuntime.AutoDownload));

        root.Set("diagnostics", new JsonObject()
            .Set("logLevel", LogLevels.ToText(config.Diagnostics.LogLevel)));

        return root;
    }

    /// <summary>
    /// 从 JSON 映射到领域模型。未知字段忽略（这是前向兼容要求）；
    /// 字段类型不符时退回默认值，而不是让整个配置失败。
    /// </summary>
    public static LauncherConfig FromJson(JsonObject root)
    {
        LauncherConfig config = LauncherConfig.CreateDefault();

        config.SchemaVersion = root.GetInt("schemaVersion", LauncherConfig.CurrentSchemaVersion) ?? LauncherConfig.CurrentSchemaVersion;

        JsonObject? identity = root.GetObject("identity");
        if (identity != null)
        {
            config.Identity.Source = ParseIdentitySource(identity.GetString("source"));
            config.Identity.OfflineUserName = identity.GetString("offlineUserName");
            config.Identity.LastAccountKey = identity.GetString("lastAccountKey");
        }

        JsonObject? java = root.GetObject("java");
        if (java != null)
        {
            config.Java.Mode = string.Equals(java.GetString("mode"), "manual", StringComparison.OrdinalIgnoreCase)
                ? JavaSelectionMode.Manual
                : JavaSelectionMode.Auto;
            config.Java.ManualPath = java.GetString("manualPath");
        }

        JsonObject? memory = root.GetObject("memory");
        if (memory != null)
        {
            int? maxMb = memory.GetInt("maxMb");
            config.Memory.MaxMb = maxMb.HasValue && maxMb.Value > 0 ? maxMb : null;
        }

        JsonObject? launch = root.GetObject("launch");
        if (launch != null)
        {
            config.Launch.GameDirectory = launch.GetString("gameDirectory");
            config.Launch.ServerQuickConnect = launch.GetString("serverQuickConnect");
            config.Launch.ExtraJvmArgs = ReadStringArray(launch.GetArray("extraJvmArgs"));
            config.Launch.RecentServers = ReadStringArray(launch.GetArray("recentServers"));
        }

        JsonObject? network = root.GetObject("network");
        if (network != null)
        {
            config.Network.ProxyMode = ParseProxyMode(network.GetString("proxyMode"));
            config.Network.ProxyAddress = network.GetString("proxyAddress");
            // allowInvalidCertificate 恒为 false：配置文件里写了 true 也一律忽略。
        }

        JsonObject? contentSource = root.GetObject("contentSource");
        if (contentSource != null)
        {
            config.ContentSource.Kind = string.Equals(contentSource.GetString("kind"), "mirror", StringComparison.OrdinalIgnoreCase)
                ? ContentSourceKind.Mirror
                : ContentSourceKind.Official;
            config.ContentSource.MirrorBaseUrl = contentSource.GetString("mirrorBaseUrl");
        }

        JsonObject? javaRuntime = root.GetObject("javaRuntime");
        if (javaRuntime != null)
        {
            config.JavaRuntime.AutoDownload = javaRuntime.GetBoolean("autoDownload", false);
        }

        JsonObject? diagnostics = root.GetObject("diagnostics");
        if (diagnostics != null && LogLevels.TryParse(diagnostics.GetString("logLevel"), out LogLevel level))
        {
            config.Diagnostics.LogLevel = level;
        }

        return config;
    }

    private static System.Collections.Generic.List<string> ReadStringArray(JsonArray? array)
    {
        System.Collections.Generic.List<string> result = new System.Collections.Generic.List<string>();
        if (array == null)
        {
            return result;
        }

        foreach (JsonValue item in array.Enumerate())
        {
            if (item is JsonString str && !string.IsNullOrEmpty(str.Value))
            {
                result.Add(str.Value);
            }
        }

        return result;
    }

    private static IdentitySource ParseIdentitySource(string? text)
    {
        if (string.Equals(text, "microsoft", StringComparison.OrdinalIgnoreCase))
        {
            return IdentitySource.Microsoft;
        }

        if (string.Equals(text, "thirdparty", StringComparison.OrdinalIgnoreCase))
        {
            return IdentitySource.ThirdParty;
        }

        return IdentitySource.Offline;
    }

    private static ProxyMode ParseProxyMode(string? text)
    {
        if (string.Equals(text, "manual", StringComparison.OrdinalIgnoreCase))
        {
            return ProxyMode.Manual;
        }

        if (string.Equals(text, "direct", StringComparison.OrdinalIgnoreCase))
        {
            return ProxyMode.Direct;
        }

        return ProxyMode.System;
    }

    private static string IdentitySourceText(IdentitySource source)
    {
        switch (source)
        {
            case IdentitySource.Microsoft: return "microsoft";
            case IdentitySource.ThirdParty: return "thirdparty";
            default: return "offline";
        }
    }

    private static string ProxyModeText(ProxyMode mode)
    {
        switch (mode)
        {
            case ProxyMode.Manual: return "manual";
            case ProxyMode.Direct: return "direct";
            default: return "system";
        }
    }
}

/// <summary>
/// 配置读写。铁律：无论配置文件发生什么，都必须返回一份可用配置。
/// </summary>
public sealed class ConfigStore
{
    private readonly DataLayout _layout;

    /// <summary>上次装载的原始 JSON。保存时以它为基底，未知字段因此得以保留。</summary>
    private JsonObject? _raw;

    public ConfigStore(DataLayout layout)
    {
        _layout = layout ?? throw new ArgumentNullException(nameof(layout));
    }

    public string FilePath => _layout.ConfigFile;

    public ConfigLoadResult Load()
    {
        string path = _layout.ConfigFile;

        if (!File.Exists(path))
        {
            LauncherConfig defaults = LauncherConfig.CreateDefault();
            _raw = ConfigMapper.ToJson(defaults);

            // 首次运行就把默认配置落盘：数据根从第一次启动起就是可检查、可编辑的，
            // 也让"配置损坏降级"这条验收有真实的文件可损坏。
            // 写失败不阻断启动——内存里的默认值照样能用。
            Save(defaults);

            return new ConfigLoadResult(defaults, false, false, null, null);
        }

        string text;
        try
        {
            text = File.ReadAllText(path, Encoding.UTF8);
        }
        catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
        {
            return new ConfigLoadResult(
                LauncherConfig.CreateDefault(),
                false,
                false,
                null,
                ErrorCode.CfgReadFailed);
        }

        JsonValue root;
        try
        {
            root = JsonValue.Parse(text);
        }
        catch (Exception ex) when (ex is JsonFormatException || ex is ArgumentNullException)
        {
            string? backup = TryBackup(path);
            _raw = ConfigMapper.ToJson(LauncherConfig.CreateDefault());
            return new ConfigLoadResult(
                LauncherConfig.CreateDefault(),
                true,
                false,
                backup,
                ErrorCode.CfgParseFailed);
        }

        if (!(root is JsonObject obj))
        {
            string? backup = TryBackup(path);
            _raw = ConfigMapper.ToJson(LauncherConfig.CreateDefault());
            return new ConfigLoadResult(
                LauncherConfig.CreateDefault(),
                true,
                false,
                backup,
                ErrorCode.CfgParseFailed);
        }

        _raw = obj;
        LauncherConfig config = ConfigMapper.FromJson(obj);

        if (config.SchemaVersion > LauncherConfig.CurrentSchemaVersion)
        {
            // 来自更新版本的配置：只读使用，绝不回写覆盖用户的新配置。
            return new ConfigLoadResult(config, false, true, null, ErrorCode.CfgVersionTooNew);
        }

        return new ConfigLoadResult(config, false, false, null, null);
    }

    /// <summary>保存配置。原子写：先写临时文件再替换，避免断电留下半个文件。</summary>
    public LauncherException? Save(LauncherConfig config)
    {
        if (config == null)
        {
            throw new ArgumentNullException(nameof(config));
        }

        try
        {
            // 以原始 JSON 为基底，保留本程序尚不认识的字段。
            JsonObject root = _raw != null ? MergeInto(_raw, ConfigMapper.ToJson(config)) : ConfigMapper.ToJson(config);
            string json = root.ToJson(true);

            string target = _layout.ConfigFile;
            string temp = target + ".tmp";

            using (FileStream stream = new FileStream(temp, FileMode.Create, FileAccess.Write, FileShare.None))
            using (StreamWriter writer = new StreamWriter(stream, new UTF8Encoding(false)))
            {
                writer.Write(json);
                writer.Write('\n');
            }

            if (File.Exists(target))
            {
                File.Replace(temp, target, null);
            }
            else
            {
                File.Move(temp, target);
            }

            _raw = root;
            return null;
        }
        catch (PathTooLongException ex)
        {
            return LauncherException.Wrap(ErrorCode.IoPathTooLong, ex);
        }
        catch (UnauthorizedAccessException ex)
        {
            return LauncherException.Wrap(ErrorCode.IoDataRootNotWritable, ex);
        }
        catch (IOException ex)
        {
            return LauncherException.Wrap(ErrorCode.IoDataRootNotWritable, ex);
        }
    }

    /// <summary>把当前认识的值写进基底对象，成员顺序以基底为准，新增字段追加在末尾。</summary>
    private static JsonObject MergeInto(JsonObject basis, JsonObject current)
    {
        JsonObject merged = new JsonObject();

        foreach (string key in basis.Keys)
        {
            JsonValue? incoming = current[key];
            merged.Set(key, incoming ?? basis[key]);
        }

        foreach (string key in current.Keys)
        {
            if (!merged.ContainsKey(key))
            {
                merged.Set(key, current[key]);
            }
        }

        return merged;
    }

    private static string? TryBackup(string path)
    {
        try
        {
            string stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss", CultureInfo.InvariantCulture);
            string backup = path + ".corrupt-" + stamp;
            File.Copy(path, backup, overwrite: true);
            return backup;
        }
        catch (Exception)
        {
            return null;
        }
    }
}
