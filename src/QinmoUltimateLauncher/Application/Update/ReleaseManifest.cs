using System;
using System.Collections.Generic;

namespace Qul.Application.Update;

/// <summary>
/// 发布清单。格式一经冻结就不再随意变动——它是启动器与发布侧之间唯一的契约。
///
/// <code>
/// {
///   "formatVersion": 1,
///   "releases": [
///     { "version": "0.2.0", "downloadUrl": "https://…/QinmoUltimateLauncher-0.2.0.exe",
///       "sha256": "64 位十六进制", "sizeBytes": 269312,
///       "minimumVersion": "0.1.0", "notes": "…" }
///   ]
/// }
/// </code>
///
/// **sha256 是必填项。** 没有摘要就不允许更新——宁可让用户手动换文件，
/// 也不能把一个来源不明的可执行文件放到用户机器上执行。
/// </summary>
public sealed class ReleaseManifest
{
    public const int SupportedFormatVersion = 1;

    public int FormatVersion { get; set; } = SupportedFormatVersion;

    public IReadOnlyList<UpdateRelease> Releases { get; set; } = Array.Empty<UpdateRelease>();

    /// <summary>
    /// 选出应当升级到的那个版本。三条规则：
    /// 必须比当前新；必须满足 <c>minimumVersion</c>（否则需要用户手动处理）；
    /// 必须带摘要。<see cref="UpdateRelease.Sha256"/> 为空的一律跳过。
    /// </summary>
    public UpdateRelease? Pick(string currentVersion)
    {
        UpdateRelease? best = null;

        for (int i = 0; i < Releases.Count; i++)
        {
            UpdateRelease candidate = Releases[i];

            if (string.IsNullOrWhiteSpace(candidate.Sha256))
            {
                continue;
            }

            if (string.IsNullOrWhiteSpace(candidate.DownloadUrl))
            {
                continue;
            }

            if (!LauncherVersion.IsNewer(candidate.Version, currentVersion))
            {
                continue;
            }

            if (!SatisfiesMinimum(candidate, currentVersion))
            {
                continue;
            }

            if (best == null || LauncherVersion.IsNewer(candidate.Version, best.Version))
            {
                best = candidate;
            }
        }

        return best;
    }

    private static bool SatisfiesMinimum(UpdateRelease release, string currentVersion)
    {
        if (string.IsNullOrWhiteSpace(release.MinimumVersion))
        {
            return true;
        }

        // 当前版本必须 >= minimumVersion，否则这次升级会踩到不兼容的旧数据。
        return LauncherVersion.Compare(currentVersion, release.MinimumVersion) >= 0;
    }
}
