using System;
using Qul.Domain.Metadata;

namespace Qul.Domain.Downloads;

/// <summary>
/// 旧式 natives 的分类器选择。纯逻辑，放在领域层以便穷举单测。
///
/// 这里要处理三种真实存在的形态（全部来自 P1 的实测结论）：
///   1. 普通映射：natives[windows] = "natives-windows"
///   2. **模板值**：natives[windows] = "natives-windows-${arch}"，必须先替换才能命中
///   3. **悬空引用**：声明的变体在 classifiers 里并不存在，必须跳过该库而不是抛异常
///
/// 现代版本（26.x）根本没有 natives 映射，natives 是坐标自带 classifier 的独立库条目，
/// 由 <see cref="DownloadPlanBuilder"/> 走普通库通道处理，不经过这里。
/// </summary>
public static class NativeClassifierSelector
{
    public const string ArchPlaceholder = "${arch}";

    /// <summary>该平台上应下载的 classifier 键；没有可用变体时返回 null。</summary>
    public static string? Select(LibraryRef library, EnvironmentProfile environment)
    {
        if (library == null)
        {
            throw new ArgumentNullException(nameof(library));
        }

        if (environment == null)
        {
            throw new ArgumentNullException(nameof(environment));
        }

        if (library.Natives.Count == 0)
        {
            return null;
        }

        if (!library.Natives.TryGetValue(environment.OsName, out string? declared) || string.IsNullOrEmpty(declared))
        {
            return null;
        }

        string resolved = SubstituteArch(declared!, environment.OsArch);

        // 悬空引用：官方元数据里确实存在（twitch-platform 声明了不存在的 natives-linux）。
        // 正确行为是跳过该库，而不是抛异常、也不是把 null 一路传下去。
        return library.Classifiers.ContainsKey(resolved) ? resolved : null;
    }

    /// <summary>把 <c>${arch}</c> 替换成官方当年使用的位宽标记。</summary>
    public static string SubstituteArch(string classifier, string osArch)
    {
        if (string.IsNullOrEmpty(classifier))
        {
            return string.Empty;
        }

        if (classifier.IndexOf(ArchPlaceholder, StringComparison.Ordinal) < 0)
        {
            return classifier;
        }

        return classifier.Replace(ArchPlaceholder, ArchToken(osArch));
    }

    /// <summary>
    /// <c>${arch}</c> 在 1.7 时代的取值是 32 / 64，不是 x86_64 这类标识。
    /// 因此这里必须做一次显式映射，不能直接把架构名塞进去。
    /// </summary>
    public static string ArchToken(string osArch)
    {
        return string.Equals(osArch, EnvironmentProfile.ArchX86, StringComparison.OrdinalIgnoreCase)
            ? "32"
            : "64";
    }
}
