using System;
using System.Globalization;

namespace Qul.Application.Update;

/// <summary>发布清单里的一条版本记录。</summary>
public sealed class UpdateRelease
{
    public string Version { get; set; } = string.Empty;

    public string DownloadUrl { get; set; } = string.Empty;

    /// <summary>发布方给出的 SHA-256（小写十六进制）。**没有它就不允许更新。**</summary>
    public string Sha256 { get; set; } = string.Empty;

    public long SizeBytes { get; set; }

    public string? Notes { get; set; }

    /// <summary>低于这个版本不能直接升级（需要用户手动处理）。空表示无限制。</summary>
    public string? MinimumVersion { get; set; }
}

/// <summary>
/// 待应用更新的标记（磁盘上的 pending.json）。
///
/// **它同时是回退的依据**：只有当新版本成功启动之后才会删掉它。
/// 因此"下次启动时它还在"就等于"上次替换之后从未成功启动过"。
/// </summary>
public sealed class PendingUpdate
{
    public const string StageSwapping = "swapping";
    public const string StageApplied = "applied";

    public string TargetVersion { get; set; } = string.Empty;

    /// <summary>待应用的下载文件（相对 updates 目录的文件名）。</summary>
    public string PendingFileName { get; set; } = string.Empty;

    /// <summary>原程序备份的文件名。</summary>
    public string BackupFileName { get; set; } = string.Empty;

    public string Sha256 { get; set; } = string.Empty;

    public string Stage { get; set; } = StageSwapping;

    public DateTimeOffset StartedAt { get; set; }

    public bool IsSwapping => string.Equals(Stage, StageSwapping, StringComparison.Ordinal);

    public string Describe()
    {
        return TargetVersion + " / " + Stage + " / " + StartedAt.ToString("u", CultureInfo.InvariantCulture);
    }
}

public enum UpdateStage
{
    Idle = 0,
    Checking = 1,
    Downloading = 2,
    Verifying = 3,
    Ready = 4,
    Failed = 5,
}

/// <summary>替换自身可执行文件的结果。</summary>
public sealed class SelfReplaceOutcome
{
    private SelfReplaceOutcome(bool succeeded, string? failureReason, bool rolledBack)
    {
        Succeeded = succeeded;
        FailureReason = failureReason;
        RolledBack = rolledBack;
    }

    public bool Succeeded { get; }

    public string? FailureReason { get; }

    /// <summary>失败时是否已经在进程内把原文件改回原名。</summary>
    public bool RolledBack { get; }

    public static SelfReplaceOutcome Ok()
    {
        return new SelfReplaceOutcome(true, null, false);
    }

    public static SelfReplaceOutcome Failed(string reason)
    {
        return new SelfReplaceOutcome(false, reason, false);
    }

    public static SelfReplaceOutcome FailedAndRestored(string reason)
    {
        return new SelfReplaceOutcome(false, reason, true);
    }
}

/// <summary>
/// 版本号比较。只支持 <c>主.次.修订</c> 加可选 <c>-预发布</c> 后缀。
/// 够用就好——启动器不需要完整的 SemVer 语义（构建元数据一律忽略）。
/// </summary>
public static class LauncherVersion
{
    public static int Compare(string? left, string? right)
    {
        int[] a = ParseParts(left);
        int[] b = ParseParts(right);

        for (int i = 0; i < 3; i++)
        {
            int diff = a[i] - b[i];
            if (diff != 0)
            {
                return diff > 0 ? 1 : -1;
            }
        }

        // 正式版高于同号预发布版：1.0.0 > 1.0.0-rc1
        bool aPre = HasPrerelease(left);
        bool bPre = HasPrerelease(right);

        if (aPre == bPre)
        {
            return 0;
        }

        return aPre ? -1 : 1;
    }

    public static bool IsNewer(string? candidate, string? current)
    {
        return Compare(candidate, current) > 0;
    }

    private static bool HasPrerelease(string? text)
    {
        return !string.IsNullOrEmpty(text) && text!.IndexOf('-') >= 0;
    }

    private static int[] ParseParts(string? text)
    {
        int[] parts = new int[3];

        if (string.IsNullOrWhiteSpace(text))
        {
            return parts;
        }

        string head = text!.Trim();
        int dash = head.IndexOf('-');
        if (dash >= 0)
        {
            head = head.Substring(0, dash);
        }

        string[] pieces = head.Split('.');
        for (int i = 0; i < 3 && i < pieces.Length; i++)
        {
            int.TryParse(pieces[i], NumberStyles.Integer, CultureInfo.InvariantCulture, out parts[i]);
        }

        return parts;
    }
}
