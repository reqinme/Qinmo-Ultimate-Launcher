using System;
using System.Collections.Generic;

namespace Qul.Domain.Diagnostics;

/// <summary>日志级别。数值越大越严重，便于按阈值过滤。</summary>
public enum LogLevel
{
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

public static class LogLevels
{
    public static bool TryParse(string? text, out LogLevel level)
    {
        level = LogLevel.Info;
        if (string.IsNullOrWhiteSpace(text))
        {
            return false;
        }

        switch (text!.Trim().ToLowerInvariant())
        {
            case "debug":
                level = LogLevel.Debug;
                return true;
            case "info":
                level = LogLevel.Info;
                return true;
            case "warn":
            case "warning":
                level = LogLevel.Warn;
                return true;
            case "error":
                level = LogLevel.Error;
                return true;
            default:
                return false;
        }
    }

    public static string ToText(LogLevel level)
    {
        switch (level)
        {
            case LogLevel.Debug: return "debug";
            case LogLevel.Warn: return "warn";
            case LogLevel.Error: return "error";
            default: return "info";
        }
    }
}

/// <summary>
/// 启动器错误码。每个码只表达一种原因，对应唯一的人话提示与处置建议。
/// 与 docs/P0-地基规范.md §4 的表格一一对应，两边必须同步修改。
/// </summary>
public enum ErrorCode
{
    None = 0,

    CfgReadFailed,
    CfgParseFailed,
    CfgVersionTooNew,

    IoDataRootNotWritable,
    IoDiskFull,
    IoPathTooLong,

    NetUnreachable,
    NetProxyInvalid,
    NetCertificateInvalid,
    NetTimeout,
    NetHttpStatus,

    MetaIndexFailed,
    MetaVersionInvalid,
    MetaInheritBroken,
    MetaUnknownField,

    DlFailed,
    DlChecksumMismatch,
    DlChecksumMissing,

    ZipEntryEscape,
    ZipInvalid,

    JavaNotFound,
    JavaVersionMismatch,
    JavaInvalid,

    AuthUserCancelled,
    AuthTokenRefreshFailed,
    AuthNoOwnership,
    AuthThirdPartyUnavailable,
    AuthPrerequisiteMissing,

    PlanUnresolvedPlaceholder,
    PlanSkeletonMismatch,

    ProcStartFailed,
    ProcNonZeroExit,
    ProcStillRunning,

    UpdFailed,
}

/// <summary>错误码的稳定对外表示（编号与人话提示）。</summary>
public static class ErrorCodes
{
    /// <summary>未登记的错误码统一落到此编号，绝不复用已登记编号。</summary>
    public const string UnknownId = "QUL-GEN-0000";

    private static readonly Dictionary<ErrorCode, string> Ids = new Dictionary<ErrorCode, string>
    {
        { ErrorCode.CfgReadFailed, "QUL-CFG-0001" },
        { ErrorCode.CfgParseFailed, "QUL-CFG-0002" },
        { ErrorCode.CfgVersionTooNew, "QUL-CFG-0003" },
        { ErrorCode.IoDataRootNotWritable, "QUL-IO-0001" },
        { ErrorCode.IoDiskFull, "QUL-IO-0002" },
        { ErrorCode.IoPathTooLong, "QUL-IO-0003" },
        { ErrorCode.NetUnreachable, "QUL-NET-0001" },
        { ErrorCode.NetProxyInvalid, "QUL-NET-0002" },
        { ErrorCode.NetCertificateInvalid, "QUL-NET-0003" },
        { ErrorCode.NetTimeout, "QUL-NET-0004" },
        { ErrorCode.NetHttpStatus, "QUL-NET-0005" },
        { ErrorCode.MetaIndexFailed, "QUL-META-0001" },
        { ErrorCode.MetaVersionInvalid, "QUL-META-0002" },
        { ErrorCode.MetaInheritBroken, "QUL-META-0003" },
        { ErrorCode.MetaUnknownField, "QUL-META-0004" },
        { ErrorCode.DlFailed, "QUL-DL-0001" },
        { ErrorCode.DlChecksumMismatch, "QUL-DL-0002" },
        { ErrorCode.DlChecksumMissing, "QUL-DL-0003" },
        { ErrorCode.ZipEntryEscape, "QUL-ZIP-0001" },
        { ErrorCode.ZipInvalid, "QUL-ZIP-0002" },
        { ErrorCode.JavaNotFound, "QUL-JAVA-0001" },
        { ErrorCode.JavaVersionMismatch, "QUL-JAVA-0002" },
        { ErrorCode.JavaInvalid, "QUL-JAVA-0003" },
        { ErrorCode.AuthUserCancelled, "QUL-AUTH-0001" },
        { ErrorCode.AuthTokenRefreshFailed, "QUL-AUTH-0002" },
        { ErrorCode.AuthNoOwnership, "QUL-AUTH-0003" },
        { ErrorCode.AuthThirdPartyUnavailable, "QUL-AUTH-0004" },
        { ErrorCode.AuthPrerequisiteMissing, "QUL-AUTH-0005" },
        { ErrorCode.PlanUnresolvedPlaceholder, "QUL-PLAN-0001" },
        { ErrorCode.PlanSkeletonMismatch, "QUL-PLAN-0002" },
        { ErrorCode.ProcStartFailed, "QUL-PROC-0001" },
        { ErrorCode.ProcNonZeroExit, "QUL-PROC-0002" },
        { ErrorCode.ProcStillRunning, "QUL-PROC-0003" },
        { ErrorCode.UpdFailed, "QUL-UPD-0001" },
    };

    /// <summary>面向用户的人话提示要点。不含任何用户数据。</summary>
    private static readonly Dictionary<ErrorCode, string> Hints = new Dictionary<ErrorCode, string>
    {
        { ErrorCode.CfgReadFailed, "无法读取配置，已按默认值启动。" },
        { ErrorCode.CfgParseFailed, "配置文件已损坏，已备份原文件并重置为默认值。" },
        { ErrorCode.CfgVersionTooNew, "配置来自更新版本的启动器，本次只读使用。" },
        { ErrorCode.IoDataRootNotWritable, "程序目录不可写，数据已改用用户目录存放。" },
        { ErrorCode.IoDiskFull, "磁盘空间不足，请释放空间后重试。" },
        { ErrorCode.IoPathTooLong, "路径过长，请把游戏目录改到更短的位置。" },
        { ErrorCode.NetUnreachable, "网络不可达，请检查网络连接。" },
        { ErrorCode.NetProxyInvalid, "代理配置不可用，请检查代理设置或改为直连。" },
        { ErrorCode.NetCertificateInvalid, "服务器证书校验未通过，连接已中止。" },
        { ErrorCode.NetTimeout, "请求超时，重试后仍未成功。" },
        { ErrorCode.NetHttpStatus, "服务器返回了失败状态。" },
        { ErrorCode.MetaIndexFailed, "无法获取版本列表，请稍后重试。" },
        { ErrorCode.MetaVersionInvalid, "该版本的元数据异常，已拒绝使用。" },
        { ErrorCode.MetaInheritBroken, "该版本的继承关系异常。" },
        { ErrorCode.MetaUnknownField, "版本元数据包含未知字段，已忽略。" },
        { ErrorCode.DlFailed, "下载失败，正在重试。" },
        { ErrorCode.DlChecksumMismatch, "文件损坏，正在重新下载。" },
        { ErrorCode.DlChecksumMissing, "元数据未提供校验值，已拒绝下载该文件。" },
        { ErrorCode.ZipEntryEscape, "压缩包内含越界路径，已整包拒绝。" },
        { ErrorCode.ZipInvalid, "压缩包结构非法，文件可能已损坏。" },
        { ErrorCode.JavaNotFound, "未找到可用的 Java，请安装或手动指定。" },
        { ErrorCode.JavaVersionMismatch, "Java 版本不满足该游戏版本的要求。" },
        { ErrorCode.JavaInvalid, "指定的 Java 可执行文件无效。" },
        { ErrorCode.AuthUserCancelled, "已取消登录。" },
        { ErrorCode.AuthTokenRefreshFailed, "登录状态已失效，请重新登录。" },
        { ErrorCode.AuthNoOwnership, "该账户不拥有此游戏。" },
        { ErrorCode.AuthThirdPartyUnavailable, "第三方验证服务不可用；不影响其他身份来源。" },
        { ErrorCode.AuthPrerequisiteMissing, "微软正版登录当前不可用。" },
        { ErrorCode.PlanUnresolvedPlaceholder, "启动计划存在未替换的占位符，已阻止启动。" },
        { ErrorCode.PlanSkeletonMismatch, "启动计划一致性校验失败，已阻止启动。" },
        { ErrorCode.ProcStartFailed, "无法启动游戏进程。" },
        { ErrorCode.ProcNonZeroExit, "游戏异常退出。" },
        { ErrorCode.ProcStillRunning, "启动器即将退出，但游戏仍在运行。" },
        { ErrorCode.UpdFailed, "更新失败，已回退到旧版本。" },
    };

    public static string Id(ErrorCode code)
    {
        return Ids.TryGetValue(code, out string? id) ? id : UnknownId;
    }

    public static string Hint(ErrorCode code)
    {
        return Hints.TryGetValue(code, out string? hint) ? hint : "发生了未登记的错误。";
    }
}

/// <summary>
/// 带错误码的业务异常。凡是能被用户看见的失败，都必须携带错误码。
/// </summary>
public sealed class LauncherException : Exception
{
    public ErrorCode Code { get; }

    public LauncherException(ErrorCode code, string message)
        : base(message)
    {
        Code = code;
    }

    public LauncherException(ErrorCode code, string message, Exception innerException)
        : base(message, innerException)
    {
        Code = code;
    }

    /// <summary>包装底层异常，保留错误码。日志侧只记录错误码与内部异常类型，不记录可能含敏感内容的消息。</summary>
    public static LauncherException Wrap(ErrorCode code, Exception innerException)
    {
        return new LauncherException(code, ErrorCodes.Id(code), innerException);
    }
}
