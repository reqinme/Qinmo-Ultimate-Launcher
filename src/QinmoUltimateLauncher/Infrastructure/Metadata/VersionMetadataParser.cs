using System;
using System.Collections.Generic;
using Qul.Domain.Diagnostics;
using Qul.Domain.Metadata;
using Qul.Infrastructure.Serialization;

namespace Qul.Infrastructure.Metadata;

/// <summary>
/// 官方版本元数据的解析器。
/// 原则：宽松读、严格用。未知字段一律忽略（前向兼容），关键字段缺失则明确拒绝该版本。
/// 这里不做继承合并，也不做规则求值——那是领域层的事。
/// </summary>
public static class VersionMetadataParser
{
    /// <summary>已知的顶层字段。用于识别未知字段（QUL-META-0004，仅记日志，不致命）。</summary>
    private static readonly HashSet<string> KnownVersionFields = new HashSet<string>(StringComparer.Ordinal)
    {
        "id", "type", "inheritsFrom", "mainClass", "assets", "assetIndex", "downloads",
        "javaVersion", "libraries", "arguments", "minecraftArguments", "logging",
        "minimumLauncherVersion", "complianceLevel", "releaseTime", "time", "jar",
    };

    public static VersionManifest ParseManifest(string json)
    {
        JsonObject root = ParseRoot(json, ErrorCode.MetaIndexFailed);

        JsonObject? latest = root.GetObject("latest");

        VersionManifest manifest = new VersionManifest
        {
            LatestRelease = latest?.GetString("release") ?? string.Empty,
            LatestSnapshot = latest?.GetString("snapshot") ?? string.Empty,
        };

        JsonArray? versions = root.GetArray("versions");
        if (versions == null)
        {
            throw new LauncherException(ErrorCode.MetaIndexFailed, "manifest has no versions array");
        }

        List<VersionSummary> summaries = new List<VersionSummary>(versions.Count);

        foreach (JsonValue item in versions.Enumerate())
        {
            if (!(item is JsonObject entry))
            {
                continue;
            }

            string? id = entry.GetString("id");
            string? url = entry.GetString("url");

            // 单条坏数据不该毁掉整份清单：跳过它，其他版本照常可用。
            if (string.IsNullOrEmpty(id) || string.IsNullOrEmpty(url))
            {
                continue;
            }

            summaries.Add(new VersionSummary
            {
                Id = id!,
                Type = ParseVersionType(entry.GetString("type")),
                Url = url!,
                Sha1 = entry.GetString("sha1"),
                Size = entry.GetLong("size"),
                ReleaseTime = entry.GetString("releaseTime"),
                ComplianceLevel = entry.GetInt("complianceLevel"),
            });
        }

        if (summaries.Count == 0)
        {
            throw new LauncherException(ErrorCode.MetaIndexFailed, "manifest yielded no usable version entries");
        }

        manifest.Versions = summaries;
        return manifest;
    }

    /// <param name="expectedId">若提供，则版本 id 必须与之完全一致——防止拿到被替换或串号的元数据。</param>
    /// <param name="unknownTopLevelFields">若提供，则收集未知的顶层字段名（QUL-META-0004 只记日志）。</param>
    public static VersionDetail ParseVersion(
        string json,
        string? expectedId = null,
        IList<string>? unknownTopLevelFields = null)
    {
        JsonObject root = ParseRoot(json, ErrorCode.MetaVersionInvalid);

        string id = root.GetString("id") ?? expectedId ?? string.Empty;
        if (string.IsNullOrWhiteSpace(id))
        {
            throw new LauncherException(ErrorCode.MetaVersionInvalid, "version json has no id");
        }

        if (!string.IsNullOrEmpty(expectedId) && !string.Equals(id, expectedId, StringComparison.Ordinal))
        {
            throw new LauncherException(
                ErrorCode.MetaVersionInvalid,
                "version id mismatch: expected " + expectedId + " but found " + id);
        }

        if (unknownTopLevelFields != null)
        {
            foreach (string key in root.Keys)
            {
                if (!KnownVersionFields.Contains(key))
                {
                    unknownTopLevelFields.Add(key);
                }
            }
        }

        VersionDetail detail = new VersionDetail
        {
            Id = id,
            Type = ParseVersionType(root.GetString("type")),
            InheritsFrom = root.GetString("inheritsFrom"),
            MainClass = root.GetString("mainClass"),
            Assets = root.GetString("assets"),
            AssetIndex = ReadAssetIndex(root.GetObject("assetIndex")),
            ClientDownload = ReadDownloadRef(root.GetObject("downloads")?.GetObject("client")),
            JavaVersion = ReadJavaVersion(root.GetObject("javaVersion")),
            Libraries = ReadLibraries(root.GetArray("libraries")),
            MinecraftArguments = root.GetString("minecraftArguments"),
            Logging = ReadLogging(root.GetObject("logging")),
            MinimumLauncherVersion = root.GetInt("minimumLauncherVersion"),
        };

        JsonObject? arguments = root.GetObject("arguments");
        if (arguments != null)
        {
            detail.GameArguments = ReadArgumentEntries(arguments.GetArray("game"));
            detail.JvmArguments = ReadArgumentEntries(arguments.GetArray("jvm"));
            // 26.x 起的额外参数组：P1 只原样解析，怎么用留给 P4。
            detail.DefaultUserJvmArguments = ReadArgumentEntries(arguments.GetArray("default-user-jvm"));
        }

        return detail;
    }

    private static JsonObject ParseRoot(string json, ErrorCode errorCode)
    {
        if (json == null)
        {
            throw new ArgumentNullException(nameof(json));
        }

        JsonValue value;
        try
        {
            value = JsonValue.Parse(json);
        }
        catch (JsonFormatException ex)
        {
            throw new LauncherException(errorCode, "malformed json", ex);
        }

        if (!(value is JsonObject root))
        {
            throw new LauncherException(errorCode, "expected a json object at the root but found " + value.Kind);
        }

        return root;
    }

    private static VersionType ParseVersionType(string? text)
    {
        switch (text)
        {
            case "release": return VersionType.Release;
            case "snapshot": return VersionType.Snapshot;
            case "old_beta": return VersionType.OldBeta;
            case "old_alpha": return VersionType.OldAlpha;
            default: return VersionType.Unknown;
        }
    }

    private static DownloadRef? ReadDownloadRef(JsonObject? node)
    {
        if (node == null)
        {
            return null;
        }

        string? url = node.GetString("url");
        if (string.IsNullOrWhiteSpace(url))
        {
            return null;
        }

        return new DownloadRef(url!, node.GetString("sha1"), node.GetLong("size"), node.GetString("path"));
    }

    private static AssetIndexRef? ReadAssetIndex(JsonObject? node)
    {
        if (node == null)
        {
            return null;
        }

        string? id = node.GetString("id");
        if (string.IsNullOrWhiteSpace(id))
        {
            return null;
        }

        return new AssetIndexRef
        {
            Id = id!,
            Sha1 = node.GetString("sha1"),
            Size = node.GetLong("size"),
            TotalSize = node.GetLong("totalSize"),
            Url = node.GetString("url"),
        };
    }

    private static JavaVersionRequirement? ReadJavaVersion(JsonObject? node)
    {
        if (node == null)
        {
            return null;
        }

        int? major = node.GetInt("majorVersion");
        if (!major.HasValue)
        {
            return null;
        }

        return new JavaVersionRequirement
        {
            MajorVersion = major.Value,
            Component = node.GetString("component"),
        };
    }

    private static LoggingConfig? ReadLogging(JsonObject? node)
    {
        JsonObject? client = node?.GetObject("client");
        if (client == null)
        {
            return null;
        }

        return new LoggingConfig
        {
            Argument = client.GetString("argument"),
            File = ReadDownloadRef(client.GetObject("file")),
        };
    }

    private static IReadOnlyList<LibraryRef> ReadLibraries(JsonArray? array)
    {
        if (array == null || array.Count == 0)
        {
            return Array.Empty<LibraryRef>();
        }

        List<LibraryRef> libraries = new List<LibraryRef>(array.Count);

        foreach (JsonValue item in array.Enumerate())
        {
            if (!(item is JsonObject node))
            {
                continue;
            }

            string rawName = node.GetString("name") ?? string.Empty;
            LibraryName? name = null;
            if (!string.IsNullOrEmpty(rawName))
            {
                LibraryName.TryParse(rawName, out name);
            }

            LibraryRef library = new LibraryRef
            {
                RawName = rawName,
                Name = name,
                Artifact = ReadDownloadRef(node.GetObject("downloads")?.GetObject("artifact")),
                Classifiers = ReadClassifiers(node.GetObject("downloads")?.GetObject("classifiers")),
                Natives = ReadNatives(node.GetObject("natives")),
                ExtractExclude = ReadExclude(node.GetObject("extract")?.GetArray("exclude")),
                Rules = ReadRules(node.GetArray("rules")),
            };

            // 既没有可解析坐标、也没有任何可下载内容的条目毫无用处，直接跳过。
            if (library.Name == null && library.Artifact == null && library.Classifiers.Count == 0)
            {
                continue;
            }

            libraries.Add(library);
        }

        return libraries;
    }

    private static IReadOnlyDictionary<string, DownloadRef> ReadClassifiers(JsonObject? node)
    {
        if (node == null || node.Count == 0)
        {
            return new Dictionary<string, DownloadRef>(0, StringComparer.Ordinal);
        }

        Dictionary<string, DownloadRef> map = new Dictionary<string, DownloadRef>(node.Count, StringComparer.Ordinal);

        foreach (string key in node.Keys)
        {
            DownloadRef? reference = ReadDownloadRef(node.GetObject(key));
            if (reference != null)
            {
                map[key] = reference;
            }
        }

        return map;
    }

    private static IReadOnlyDictionary<string, string> ReadNatives(JsonObject? node)
    {
        if (node == null || node.Count == 0)
        {
            return new Dictionary<string, string>(0, StringComparer.Ordinal);
        }

        Dictionary<string, string> map = new Dictionary<string, string>(node.Count, StringComparer.Ordinal);

        foreach (string key in node.Keys)
        {
            string? classifier = node.GetString(key);
            if (!string.IsNullOrEmpty(classifier))
            {
                map[key] = classifier!;
            }
        }

        return map;
    }

    private static IReadOnlyList<string> ReadExclude(JsonArray? array)
    {
        if (array == null || array.Count == 0)
        {
            return Array.Empty<string>();
        }

        List<string> values = new List<string>(array.Count);
        foreach (JsonValue item in array.Enumerate())
        {
            if (item is JsonString text && text.Value.Length > 0)
            {
                values.Add(text.Value);
            }
        }

        return values;
    }

    private static IReadOnlyList<Rule> ReadRules(JsonArray? array)
    {
        if (array == null || array.Count == 0)
        {
            return Array.Empty<Rule>();
        }

        List<Rule> rules = new List<Rule>(array.Count);

        foreach (JsonValue item in array.Enumerate())
        {
            if (!(item is JsonObject node))
            {
                continue;
            }

            string? action = node.GetString("action");
            bool isAllow = string.Equals(action, "allow", StringComparison.OrdinalIgnoreCase);
            bool isDisallow = string.Equals(action, "disallow", StringComparison.OrdinalIgnoreCase);

            // 动作无法识别时整条规则跳过：既不能当 allow（会放行不该放行的），
            // 也不能当 disallow（会拦掉本该生效的），只能判定为这条规则不可用。
            if (!isAllow && !isDisallow)
            {
                continue;
            }

            Rule rule = new Rule
            {
                Action = isAllow ? RuleAction.Allow : RuleAction.Disallow,
            };

            JsonObject? os = node.GetObject("os");
            if (os != null)
            {
                rule.OsName = os.GetString("name");
                rule.OsArch = os.GetString("arch");
                rule.OsVersion = os.GetString("version");
            }

            JsonObject? features = node.GetObject("features");
            if (features != null && features.Count > 0)
            {
                Dictionary<string, bool> map = new Dictionary<string, bool>(features.Count, StringComparer.Ordinal);
                foreach (string key in features.Keys)
                {
                    map[key] = features.GetBoolean(key, false);
                }

                rule.Features = map;
            }

            rules.Add(rule);
        }

        return rules;
    }

    private static IReadOnlyList<ArgumentEntry> ReadArgumentEntries(JsonArray? array)
    {
        if (array == null || array.Count == 0)
        {
            return Array.Empty<ArgumentEntry>();
        }

        List<ArgumentEntry> entries = new List<ArgumentEntry>(array.Count);

        foreach (JsonValue item in array.Enumerate())
        {
            if (item is JsonString text)
            {
                entries.Add(new ArgumentEntry { Values = new[] { text.Value } });
                continue;
            }

            if (!(item is JsonObject node))
            {
                continue;
            }

            // value 既可能是字符串，也可能是字符串数组——两种形态在真实元数据里都存在。
            List<string> values = new List<string>();
            JsonValue? value = node["value"];

            if (value is JsonString single)
            {
                values.Add(single.Value);
            }
            else if (value is JsonArray many)
            {
                foreach (JsonValue element in many.Enumerate())
                {
                    if (element is JsonString valueText)
                    {
                        values.Add(valueText.Value);
                    }
                }
            }

            if (values.Count == 0)
            {
                continue;
            }

            entries.Add(new ArgumentEntry
            {
                Rules = ReadRules(node.GetArray("rules")),
                Values = values,
            });
        }

        return entries;
    }
}
