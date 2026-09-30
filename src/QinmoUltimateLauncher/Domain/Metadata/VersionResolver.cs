using System;
using System.Collections.Generic;
using Qul.Domain.Diagnostics;

namespace Qul.Domain.Metadata;

/// <summary>
/// 版本继承合并。
/// 解析阶段绝不隐式合并——排障时必须能分清某个字段来自子版本还是父版本。
/// 合并只在这里发生，且只做单层合并；多层链由 <see cref="Resolve"/> 按 根→叶 顺序折叠。
/// </summary>
public static class VersionResolver
{
    public const int MaxInheritDepth = 16;

    /// <summary>单层合并：以 <paramref name="child"/> 为主，缺失字段回落到 <paramref name="parent"/>。</summary>
    public static VersionDetail Merge(VersionDetail child, VersionDetail parent)
    {
        if (child == null)
        {
            throw new ArgumentNullException(nameof(child));
        }

        if (parent == null)
        {
            throw new ArgumentNullException(nameof(parent));
        }

        return new VersionDetail
        {
            Id = child.Id,
            Type = child.Type != VersionType.Unknown ? child.Type : parent.Type,
            InheritsFrom = null,
            MainClass = FirstNonEmpty(child.MainClass, parent.MainClass),
            Assets = FirstNonEmpty(child.Assets, parent.Assets),
            AssetIndex = child.AssetIndex ?? parent.AssetIndex,
            ClientDownload = child.ClientDownload ?? parent.ClientDownload,
            JavaVersion = child.JavaVersion ?? parent.JavaVersion,
            Libraries = MergeLibraries(parent.Libraries, child.Libraries),
            GameArguments = Concat(parent.GameArguments, child.GameArguments),
            JvmArguments = Concat(parent.JvmArguments, child.JvmArguments),
            DefaultUserJvmArguments = Concat(parent.DefaultUserJvmArguments, child.DefaultUserJvmArguments),
            MinecraftArguments = FirstNonEmpty(child.MinecraftArguments, parent.MinecraftArguments),
            Logging = child.Logging ?? parent.Logging,
            MinimumLauncherVersion = child.MinimumLauncherVersion ?? parent.MinimumLauncherVersion,
        };
    }

    /// <summary>
    /// 沿 inheritsFrom 链折叠出最终版本。
    /// 链上出现环、父版本缺失、或超过深度上限时抛 QUL-META-0003 —— 畸形元数据必须被明确拒绝，而不是静默降级。
    /// </summary>
    public static VersionDetail Resolve(VersionDetail leaf, Func<string, VersionDetail?> parentLookup, int maxDepth = MaxInheritDepth)
    {
        if (leaf == null)
        {
            throw new ArgumentNullException(nameof(leaf));
        }

        if (parentLookup == null)
        {
            throw new ArgumentNullException(nameof(parentLookup));
        }

        List<VersionDetail> chain = new List<VersionDetail> { leaf };
        HashSet<string> seen = new HashSet<string>(StringComparer.Ordinal) { leaf.Id };

        VersionDetail current = leaf;
        while (!string.IsNullOrEmpty(current.InheritsFrom))
        {
            if (chain.Count > maxDepth)
            {
                throw new LauncherException(
                    ErrorCode.MetaInheritBroken,
                    "inherit chain exceeds depth limit " + maxDepth.ToString(System.Globalization.CultureInfo.InvariantCulture));
            }

            string parentId = current.InheritsFrom!;
            if (!seen.Add(parentId))
            {
                throw new LauncherException(ErrorCode.MetaInheritBroken, "inherit chain contains a cycle at " + parentId);
            }

            VersionDetail? parent = parentLookup(parentId);
            if (parent == null)
            {
                throw new LauncherException(ErrorCode.MetaInheritBroken, "missing parent version " + parentId);
            }

            chain.Add(parent);
            current = parent;
        }

        // chain 是 叶→根，折叠必须 根→叶。
        VersionDetail resolved = chain[chain.Count - 1];
        for (int i = chain.Count - 2; i >= 0; i--)
        {
            resolved = Merge(chain[i], resolved);
        }

        return resolved;
    }

    /// <summary>
    /// 库列表合并。语义是**跨父/子边界覆盖，单份列表内部绝不去重**：
    /// 1) 子列表原样保留，顺序与内容一个字都不改；
    /// 2) 父列表里，凡是身份（<see cref="LibraryRef.IdentityKey"/>，不含版本号）已被子列表覆盖的条目丢弃；
    /// 3) 其余父条目按原顺序排在前面。
    ///
    /// 不做"单份列表内部去重"是硬要求：1.16.5 里 org.lwjgl:lwjgl:3.2.1 与 3.2.2 服务不同平台、
    /// 在同一列表内合法共存（实测 57 条库只对应 41 个唯一坐标）。早期版本的去重实现把这类条目误杀，
    /// 结果是运行时缺本地库。
    ///
    /// 该覆盖语义在 P1 无真实继承样本可验，P9 接入加载器时必须以 Fabric/Forge 的真实数据复核。
    /// </summary>
    public static IReadOnlyList<LibraryRef> MergeLibraries(
        IReadOnlyList<LibraryRef> parentLibraries,
        IReadOnlyList<LibraryRef> childLibraries)
    {
        if (childLibraries == null || childLibraries.Count == 0)
        {
            return parentLibraries ?? Array.Empty<LibraryRef>();
        }

        if (parentLibraries == null || parentLibraries.Count == 0)
        {
            return childLibraries;
        }

        HashSet<string> overridden = new HashSet<string>(StringComparer.Ordinal);
        for (int i = 0; i < childLibraries.Count; i++)
        {
            overridden.Add(childLibraries[i].IdentityKey);
        }

        List<LibraryRef> merged = new List<LibraryRef>(parentLibraries.Count + childLibraries.Count);

        for (int i = 0; i < parentLibraries.Count; i++)
        {
            LibraryRef candidate = parentLibraries[i];
            if (!overridden.Contains(candidate.IdentityKey))
            {
                merged.Add(candidate);
            }
        }

        merged.AddRange(childLibraries);
        return merged;
    }

    /// <summary>合并后的必备字段校验。返回 null 表示可用。</summary>
    public static LauncherException? Validate(VersionDetail resolved)
    {
        if (resolved == null)
        {
            throw new ArgumentNullException(nameof(resolved));
        }

        if (string.IsNullOrWhiteSpace(resolved.Id))
        {
            return new LauncherException(ErrorCode.MetaVersionInvalid, "version id is missing");
        }

        if (string.IsNullOrWhiteSpace(resolved.MainClass))
        {
            return new LauncherException(ErrorCode.MetaVersionInvalid, "mainClass is missing for " + resolved.Id);
        }

        if (resolved.ClientDownload == null || string.IsNullOrWhiteSpace(resolved.ClientDownload.Url))
        {
            return new LauncherException(ErrorCode.MetaVersionInvalid, "client download is missing for " + resolved.Id);
        }

        if (resolved.UsesLegacyArguments && resolved.GameArguments.Count > 0)
        {
            return new LauncherException(
                ErrorCode.MetaVersionInvalid,
                "version " + resolved.Id + " declares both minecraftArguments and arguments.game");
        }

        return null;
    }

    private static IReadOnlyList<T> Concat<T>(IReadOnlyList<T> first, IReadOnlyList<T> second)
    {
        if (first.Count == 0)
        {
            return second;
        }

        if (second.Count == 0)
        {
            return first;
        }

        List<T> result = new List<T>(first.Count + second.Count);
        result.AddRange(first);
        result.AddRange(second);
        return result;
    }

    private static string? FirstNonEmpty(string? preferred, string? fallback)
    {
        return string.IsNullOrEmpty(preferred) ? fallback : preferred;
    }
}
