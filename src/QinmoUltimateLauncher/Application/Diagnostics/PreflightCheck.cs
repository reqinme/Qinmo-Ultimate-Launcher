using System;
using System.Collections.Generic;
using Qul.Domain.Configuration;

namespace Qul.Application.Diagnostics;

/// <summary>预检项的适用范围。</summary>
public enum PreflightScope
{
    /// <summary>每次启动都执行。</summary>
    Always = 0,

    /// <summary>**仅当用户配置了目标服务器时**才执行。</summary>
    WhenTargetConfigured = 1,
}

public enum PreflightSeverity
{
    Info = 0,
    Warning = 1,
    Blocking = 2,
}

public sealed class PreflightItem
{
    public PreflightItem(
        string id,
        PreflightScope scope,
        PreflightSeverity severity,
        string title,
        string detail,
        bool requiresAcknowledgement = false)
    {
        Id = id;
        Scope = scope;
        Severity = severity;
        Title = title;
        Detail = detail;
        RequiresAcknowledgement = requiresAcknowledgement;
    }

    /// <summary>稳定标识，便于排障时对照文档。</summary>
    public string Id { get; }

    public PreflightScope Scope { get; }

    public PreflightSeverity Severity { get; }

    public string Title { get; }

    public string Detail { get; }

    /// <summary>是否需要用户显式确认后才能启动。身份来源的能力告知为真——它不可默认跳过。</summary>
    public bool RequiresAcknowledgement { get; }

    public bool BlocksLaunch => Severity == PreflightSeverity.Blocking;
}

public sealed class PreflightContext
{
    public IdentitySource IdentitySource { get; set; } = IdentitySource.Offline;

    /// <summary>身份来源的能力限制说明。离线来源必须非空。</summary>
    public IReadOnlyList<string> IdentityNotices { get; set; } = Array.Empty<string>();

    public bool IsOnlineVerified { get; set; }

    /// <summary>用户**显式配置**的目标服务器。为空即"用户没说要去哪"。</summary>
    public string? ServerTarget { get; set; }

    public bool JavaAvailable { get; set; }

    /// <summary>客户端是否已就位。未就位只是"需要下载"，不是错误。</summary>
    public bool ClientInstalled { get; set; }

    public long FreeDiskBytes { get; set; } = long.MaxValue;

    /// <summary>本次启动预估需要的字节数。</summary>
    public long EstimatedBytes { get; set; }
}

/// <summary>
/// 启动前预检。
///
/// **最要紧的一条纪律**：启动器通常不知道用户要连哪个服务器。
/// 因此服务器相关结论只在用户**显式配置了目标**时才产生——
/// 没配置就什么都别说，绝不能推断"大概是要连正版服务器吧"。
/// 一个凭空冒出来的服务器警告，比没有警告更糟。
/// </summary>
public static class PreflightCheck
{
    public const string IdIdentityCapability = "identity.capability";
    public const string IdJavaAvailable = "java.available";
    public const string IdClientInstalled = "client.installed";
    public const string IdDiskSpace = "disk.space";
    public const string IdServerCompatibility = "server.compatibility";
    public const string IdServerAddress = "server.address";

    private const long DiskMarginBytes = 256L * 1024 * 1024;

    public static IReadOnlyList<PreflightItem> Evaluate(PreflightContext context)
    {
        if (context == null)
        {
            throw new ArgumentNullException(nameof(context));
        }

        List<PreflightItem> items = new List<PreflightItem>();

        EvaluateAlways(context, items);

        // 只有用户给了目标，才谈得上"这个服务器能不能进"。
        if (HasExplicitTarget(context.ServerTarget))
        {
            EvaluateTarget(context, items);
        }

        return items;
    }

    /// <summary>用户是否显式给过目标。空、空白都算没给。</summary>
    public static bool HasExplicitTarget(string? serverTarget)
    {
        return !string.IsNullOrWhiteSpace(serverTarget);
    }

    public static bool NeedsAcknowledgement(IReadOnlyList<PreflightItem> items)
    {
        for (int i = 0; i < items.Count; i++)
        {
            if (items[i].RequiresAcknowledgement)
            {
                return true;
            }
        }

        return false;
    }

    public static bool HasBlocking(IReadOnlyList<PreflightItem> items)
    {
        for (int i = 0; i < items.Count; i++)
        {
            if (items[i].BlocksLaunch)
            {
                return true;
            }
        }

        return false;
    }

    private static void EvaluateAlways(PreflightContext context, List<PreflightItem> items)
    {
        // 身份来源的能力限制：**始终执行，且必须被确认**。
        if (context.IdentityNotices.Count > 0)
        {
            items.Add(new PreflightItem(
                IdIdentityCapability,
                PreflightScope.Always,
                context.IsOnlineVerified ? PreflightSeverity.Info : PreflightSeverity.Warning,
                "身份来源的能力限制",
                string.Join("\n", context.IdentityNotices),
                requiresAcknowledgement: true));
        }

        if (!context.JavaAvailable)
        {
            items.Add(new PreflightItem(
                IdJavaAvailable,
                PreflightScope.Always,
                PreflightSeverity.Blocking,
                "找不到可用的 Java",
                "请安装符合该版本要求的 Java，或在设置中手动指定 java.exe 的路径。"));
        }

        if (!context.ClientInstalled)
        {
            items.Add(new PreflightItem(
                IdClientInstalled,
                PreflightScope.Always,
                PreflightSeverity.Info,
                "该版本尚未就位",
                "首次启动会先下载所需文件；这会花一些时间。"));
        }

        long required = context.EstimatedBytes + DiskMarginBytes;
        if (context.FreeDiskBytes < required)
        {
            items.Add(new PreflightItem(
                IdDiskSpace,
                PreflightScope.Always,
                PreflightSeverity.Blocking,
                "磁盘空间不足",
                "需要约 " + Megabytes(required) + "，当前可用 " + Megabytes(context.FreeDiskBytes) + "。"));
        }
    }

    private static void EvaluateTarget(PreflightContext context, List<PreflightItem> items)
    {
        string target = context.ServerTarget!.Trim();

        if (!LooksLikeAddress(target))
        {
            items.Add(new PreflightItem(
                IdServerAddress,
                PreflightScope.WhenTargetConfigured,
                PreflightSeverity.Warning,
                "目标地址看起来不完整",
                "已配置的目标是「" + target + "」，它不像一个服务器地址。请确认是否需要补上端口或域名。"));
        }

        // 离线账户进不了正版验证服务器——用户既然给了目标，就必须把这句话说在前头。
        if (!context.IsOnlineVerified)
        {
            items.Add(new PreflightItem(
                IdServerCompatibility,
                PreflightScope.WhenTargetConfigured,
                PreflightSeverity.Warning,
                "该身份来源可能无法进入已配置的目标",
                "当前身份来源未经在线验证，若「" + target + "」要求正版验证，将无法进入。"));
        }
    }

    /// <summary>
    /// 只做"看起来像不像 主机:端口"的判断，不解析、不探测、不连接。
    ///
    /// 按**字符集**判断，而不是按"有没有空格"：一个中文串、一段带协议的 URL、
    /// 一句粘贴错的说明文字，都该被认出来不是地址。
    /// 这个判断只用来提醒，绝不用来拦人。
    /// </summary>
    private static bool LooksLikeAddress(string target)
    {
        if (target.Length < 3)
        {
            return false;
        }

        bool hasLetterOrDigit = false;

        for (int i = 0; i < target.Length; i++)
        {
            char c = target[i];

            bool allowed = (c >= 'a' && c <= 'z')
                           || (c >= 'A' && c <= 'Z')
                           || (c >= '0' && c <= '9')
                           || c == '.' || c == '-' || c == ':' || c == '_'
                           || c == '[' || c == ']';

            if (!allowed)
            {
                return false;
            }

            if ((c >= '0' && c <= '9') || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'))
            {
                hasLetterOrDigit = true;
            }
        }

        if (!hasLetterOrDigit)
        {
            return false;
        }

        // 不能以分隔符开头或结尾。
        return target[0] != ':' && target[0] != '.' && target[0] != '-'
               && target[target.Length - 1] != ':' && target[target.Length - 1] != '.'
               && target[target.Length - 1] != '-';
    }

    private static string Megabytes(long bytes)
    {
        double mb = bytes / 1024.0 / 1024.0;
        return mb.ToString("F1", System.Globalization.CultureInfo.InvariantCulture) + " MB";
    }
}
