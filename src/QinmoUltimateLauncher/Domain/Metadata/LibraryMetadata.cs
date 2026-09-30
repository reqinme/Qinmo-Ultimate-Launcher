using System;
using System.Collections.Generic;

namespace Qul.Domain.Metadata;

public enum RuleAction
{
    Allow,
    Disallow,
}

/// <summary>
/// 一条元数据规则。字段刻意保持扁平：真实样本里 os 只有 name/arch/version 三个可选键，
/// 为一个三字段结构再套一层类型得不偿失。
/// </summary>
public sealed class Rule
{
    public RuleAction Action { get; set; } = RuleAction.Allow;

    public string? OsName { get; set; }

    public string? OsArch { get; set; }

    public string? OsVersion { get; set; }

    /// <summary>特性开关条件。空表示该规则与特性无关。</summary>
    public IReadOnlyDictionary<string, bool> Features { get; set; } = EmptyFeatures;

    public static readonly IReadOnlyDictionary<string, bool> EmptyFeatures =
        new Dictionary<string, bool>(0, StringComparer.Ordinal);
}

/// <summary>
/// 一条（可能带规则的）命令行参数项。
/// 元数据里 value 既可能是字符串也可能是字符串数组，解析阶段统一摊平成 Values。
/// </summary>
public sealed class ArgumentEntry
{
    public static readonly IReadOnlyList<Rule> NoRules = Array.Empty<Rule>();

    public IReadOnlyList<Rule> Rules { get; set; } = NoRules;

    public IReadOnlyList<string> Values { get; set; } = Array.Empty<string>();

    public bool HasRules => Rules.Count > 0;
}

/// <summary>
/// Maven 坐标：group:artifact:version[:classifier][@extension]。
/// 保留 classifier 是必须的——26.x 的 natives 就是靠 classifier 出现在 name 里表达的。
/// </summary>
public sealed class LibraryName
{
    public const string DefaultExtension = "jar";

    private LibraryName(string groupId, string artifactId, string version, string? classifier, string extension)
    {
        GroupId = groupId;
        ArtifactId = artifactId;
        Version = version;
        Classifier = classifier;
        Extension = extension;
    }

    public string GroupId { get; }

    public string ArtifactId { get; }

    public string Version { get; }

    public string? Classifier { get; }

    public string Extension { get; }

    /// <summary>
    /// 去重键：group:artifact[:classifier]。
    /// classifier 必须参与：26.x 的 natives 是与主 artifact 同坐标、仅 classifier 不同的独立条目，
    /// 若只用 group:artifact 做键，两者会互相顶掉。
    /// </summary>
    public string Key => HasClassifier ? GroupId + ":" + ArtifactId + ":" + Classifier : GroupId + ":" + ArtifactId;

    public bool HasClassifier => !string.IsNullOrEmpty(Classifier);

    public static bool TryParse(string? raw, out LibraryName? name)
    {
        name = null;

        if (string.IsNullOrWhiteSpace(raw))
        {
            return false;
        }

        string text = raw!.Trim();

        string extension = DefaultExtension;
        int at = text.IndexOf('@');
        if (at >= 0)
        {
            string ext = text.Substring(at + 1);
            text = text.Substring(0, at);
            if (ext.Length > 0)
            {
                extension = ext;
            }
        }

        string[] parts = text.Split(':');
        if (parts.Length < 3 || parts.Length > 4)
        {
            return false;
        }

        for (int i = 0; i < parts.Length; i++)
        {
            if (parts[i].Length == 0)
            {
                return false;
            }
        }

        string? classifier = parts.Length == 4 ? parts[3] : null;
        name = new LibraryName(parts[0], parts[1], parts[2], classifier, extension);
        return true;
    }

    public override string ToString()
    {
        string text = GroupId + ":" + ArtifactId + ":" + Version;
        if (HasClassifier)
        {
            text += ":" + Classifier;
        }

        if (!string.Equals(Extension, DefaultExtension, StringComparison.Ordinal))
        {
            text += "@" + Extension;
        }

        return text;
    }
}

public sealed class LibraryRef
{
    /// <summary>原始坐标文本，排障时按原样呈现，不做归一。</summary>
    public string RawName { get; set; } = string.Empty;

    public LibraryName? Name { get; set; }

    public DownloadRef? Artifact { get; set; }

    /// <summary>classifier 键 → 资源。键形如 natives-windows / natives-macos-arm64。</summary>
    public IReadOnlyDictionary<string, DownloadRef> Classifiers { get; set; } =
        new Dictionary<string, DownloadRef>(0, StringComparer.Ordinal);

    /// <summary>os 名 → classifier 键。这是旧式（1.7–1.16 时代）的 natives 表达方式。</summary>
    public IReadOnlyDictionary<string, string> Natives { get; set; } =
        new Dictionary<string, string>(0, StringComparer.Ordinal);

    /// <summary>解压时需要排除的前缀（通常是 META-INF/）。</summary>
    public IReadOnlyList<string> ExtractExclude { get; set; } = Array.Empty<string>();

    public IReadOnlyList<Rule> Rules { get; set; } = Array.Empty<Rule>();

    /// <summary>库坐标里直接带 classifier，例如 com.mojang:jtracy:1.14.38:natives-linux。</summary>
    public bool HasClassifierInName => Name != null && Name.HasClassifier;

    /// <summary>旧式 natives 库：有 Natives 映射但没有主 artifact。</summary>
    public bool IsLegacyNativeHolder => Natives.Count > 0;

    /// <summary>
    /// 库身份键：group:artifact + 变体判别（坐标自带 classifier、natives 键集合、classifiers 键集合）。
    /// **刻意不含版本号**——版本号正是子版本要覆盖的东西。
    ///
    /// 为什么不能只用 Maven 坐标：1.12–1.16 时代的官方元数据里，同一坐标会按 OS 出现多条。
    /// 实测 1.16.5 有 16 组、1.12.2 有 2 组；例如 org.lwjgl:lwjgl:3.2.1 同时存在通用变体与 natives-macos 变体，
    /// 坐标字符串一模一样。只按坐标去重会把一半本地库变体静默吃掉，症状要到运行时缺本地库才暴露。
    ///
    /// 为什么不能把版本号也纳入"同一实体内比较"：1.16.5 里 org.lwjgl:lwjgl:3.2.1 与 3.2.2
    /// 服务于不同平台，在同一份库列表里合法共存；一旦按身份在**单份列表内部**去重就会误杀。
    /// 因此本键只用于**跨父/子边界**判断"子版本是否覆盖了父版本的同一个库"。
    /// </summary>
    public string IdentityKey => BuildIdentityKey();

    private string BuildIdentityKey()
    {
        string baseKey = Name != null ? Name.GroupId + ":" + Name.ArtifactId : RawName;

        List<string> variants = new List<string>();

        if (Name != null && Name.HasClassifier)
        {
            variants.Add("c=" + Name.Classifier);
        }

        foreach (string key in SortedKeys(Natives))
        {
            variants.Add("n=" + key);
        }

        foreach (string key in SortedKeys(Classifiers))
        {
            variants.Add("d=" + key);
        }

        if (variants.Count == 0)
        {
            return baseKey;
        }

        variants.Sort(StringComparer.Ordinal);
        return baseKey + "|" + string.Join(",", variants);
    }

    private static List<string> SortedKeys<TValue>(IReadOnlyDictionary<string, TValue> map)
    {
        List<string> keys = new List<string>(map.Keys);
        keys.Sort(StringComparer.Ordinal);
        return keys;
    }
}
