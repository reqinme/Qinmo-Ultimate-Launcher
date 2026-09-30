using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using System.Text.RegularExpressions;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.IO;

namespace Qul.Infrastructure.Diagnostics;

/// <summary>
/// 脱敏管道。日志写入与诊断导出**必须**共用它——这是设计上的单点，
/// 不允许任何调用点绕过管道直接落盘。
/// </summary>
public static class Redactor
{
    public const string Mask = "<redacted>";
    public const string UuidMask = "<uuid>";
    public const string IpMask = "<ip>";
    public const string UserProfileMask = "%USERPROFILE%";
    public const string DataRootMask = "%DATA%";

    private static readonly object Gate = new object();

    private static readonly List<string> Secrets = new List<string>();

    private static readonly string?[] PathPrefixes = new string?[2];

    /// <summary>
    /// 账号标识。**两种形式都要认。**
    ///
    /// 先前只认带连字符的 36 位形式，而本项目实际用的是
    /// **32 位无连字符**十六进制（见 AuthSession.Uuid 与 MicrosoftAuthProvider），
    /// 于是这条掩码规则从来没生效过——掩码形同虚设比没有掩码更糟，
    /// 因为它让人以为已经保护了。
    /// </summary>
    private static readonly Regex UuidPattern = new Regex(
        @"\b(?:[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}|[0-9a-fA-F]{32})\b",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    private static readonly Regex Ipv4Pattern = new Regex(
        @"\b(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)\b",
        RegexOptions.Compiled | RegexOptions.CultureInvariant);

    /// <summary>
    /// 登记必须精确匹配替换的秘密值（例如访问令牌）。
    /// 登记进来的值在脱敏后只剩掩码，长度信息也不保留。
    /// </summary>
    public static void RegisterSecret(string? value)
    {
        if (string.IsNullOrEmpty(value) || value!.Length < 8)
        {
            return;
        }

        lock (Gate)
        {
            if (!Secrets.Contains(value))
            {
                Secrets.Add(value);
            }
        }
    }

    /// <summary>登记需要折叠成占位符的路径前缀，长的优先匹配。</summary>
    public static void RegisterPathPrefix(string? path, bool isDataRoot)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return;
        }

        lock (Gate)
        {
            PathPrefixes[isDataRoot ? 1 : 0] = path;
        }
    }

    /// <summary>
    /// 对任意文本做脱敏：秘密值 → 掩码；UUID → 占位符；IP → 占位符；已知路径前缀 → 占位符。
    /// 顺序重要：先处理长秘密值，再处理结构化模式。
    /// </summary>
    public static string Scrub(string? text)
    {
        if (string.IsNullOrEmpty(text))
        {
            return string.Empty;
        }

        string result = text!;

        lock (Gate)
        {
            foreach (string secret in Secrets)
            {
                result = result.Replace(secret, Mask);
            }

            // 更长的前缀必须先折叠：数据根通常位于用户目录之下，
            // 先折叠用户目录会把数据根一并吃掉，%DATA% 就永远匹配不到。
            string? dataRoot = PathPrefixes[1];
            if (!string.IsNullOrEmpty(dataRoot))
            {
                result = ReplaceOrdinalIgnoreCase(result, dataRoot, DataRootMask);
            }

            string? userProfile = PathPrefixes[0];
            if (!string.IsNullOrEmpty(userProfile))
            {
                result = ReplaceOrdinalIgnoreCase(result, userProfile, UserProfileMask);
            }
        }

        result = UuidPattern.Replace(result, UuidMask);
        result = Ipv4Pattern.Replace(result, IpMask);
        return result;
    }

    /// <summary>仅对路径做前缀折叠，不做 UUID/IP 替换。</summary>
    public static string ScrubPath(string? path)
    {
        if (string.IsNullOrEmpty(path))
        {
            return string.Empty;
        }

        string result = path!;

        lock (Gate)
        {
            string? dataRoot = PathPrefixes[1];
            if (!string.IsNullOrEmpty(dataRoot))
            {
                result = ReplaceOrdinalIgnoreCase(result, dataRoot, DataRootMask);
            }

            string? userProfile = PathPrefixes[0];
            if (!string.IsNullOrEmpty(userProfile))
            {
                result = ReplaceOrdinalIgnoreCase(result, userProfile, UserProfileMask);
            }
        }

        return result;
    }

    private static string ReplaceOrdinalIgnoreCase(string source, string? oldValue, string newValue)
    {
        // 前缀可能还没登记（引导早期），这里必须自己兜住，而不是指望调用点的判空能被编译器接受。
        if (oldValue == null || oldValue.Length == 0)
        {
            return source;
        }

        int index = source.IndexOf(oldValue, StringComparison.OrdinalIgnoreCase);
        if (index < 0)
        {
            return source;
        }

        StringBuilder sb = new StringBuilder(source.Length);
        int cursor = 0;

        while (index >= 0)
        {
            sb.Append(source, cursor, index - cursor);
            sb.Append(newValue);
            cursor = index + oldValue.Length;
            index = source.IndexOf(oldValue, cursor, StringComparison.OrdinalIgnoreCase);
        }

        sb.Append(source, cursor, source.Length - cursor);
        return sb.ToString();
    }
}

/// <summary>
/// 会话日志。每行一条结构化记录，字段见 docs/P0-地基规范.md §5.1。
/// 落盘前一律经 <see cref="Redactor"/>；本地留存，不上传。
/// </summary>
public sealed class SessionLog : IDisposable
{
    private const int RetainedSessionFiles = 20;

    /// <summary>日志系统本身不可用时的兜底：写入被静默丢弃。启动流程不得因日志失败而中断。</summary>
    public static readonly SessionLog Null = new SessionLog(string.Empty, LogLevel.Error, "------");

    private readonly object _gate = new object();
    private readonly string _filePath;
    private LogLevel _minimum;
    private StreamWriter? _writer;
    private bool _disposed;

    private SessionLog(string filePath, LogLevel minimum, string sessionId)
    {
        _filePath = filePath;
        _minimum = minimum;
        SessionId = sessionId;
    }

    public string SessionId { get; }

    public string FilePath => _filePath;

    /// <summary>日志级别阈值。引导期按 info 兜底，配置装载完成后再按用户设置调整。</summary>
    public LogLevel Minimum
    {
        get { lock (_gate) { return _minimum; } }
        set { lock (_gate) { _minimum = value; } }
    }

    public static SessionLog Open(DataLayout layout, LogLevel minimum)
    {
        if (layout == null)
        {
            throw new ArgumentNullException(nameof(layout));
        }

        string sessionId = Guid.NewGuid().ToString("N").Substring(0, 6);
        string stamp = DateTime.Now.ToString("yyyyMMdd-HHmmss", CultureInfo.InvariantCulture);
        string file = Path.Combine(layout.LogDirectory, "session-" + stamp + "-" + sessionId + ".log");

        SessionLog log = new SessionLog(file, minimum, sessionId);

        try
        {
            log.OpenWriter();
        }
        catch (Exception)
        {
            // 写不了日志也必须能启动：退化为静默丢弃，而不是把启动流程一起带崩。
            log._writer = null;
        }

        Rotate(layout.LogDirectory);
        return log;
    }

    public void Write(LogLevel level, string phase, string message, ErrorCode? code = null, string? detail = null, long? elapsedMs = null)
    {
        if (level < _minimum)
        {
            return;
        }

        StringBuilder sb = new StringBuilder(160);
        sb.Append("ts=").Append(DateTimeOffset.Now.ToString("yyyy-MM-ddTHH:mm:ss.fffzzz", CultureInfo.InvariantCulture));
        sb.Append(" level=").Append(LogLevels.ToText(level));
        sb.Append(" session=").Append(SessionId);
        sb.Append(" phase=").Append(Escape(phase));

        if (code.HasValue)
        {
            sb.Append(" code=").Append(ErrorCodes.Id(code.Value));
        }

        // **msg 与 detail 一样必须脱敏。**
        // 先前只有 detail 走了 Redactor，而 msg 直接落盘——
        // 于是 App.xaml.cs 里把异常 message 与**完整堆栈**经 msg 写盘时，
        // 堆栈里的 `C:\Users\<用户名>\...` 就明文进了日志，
        // 而 BootContext 明明登记了 %USERPROFILE% 期望把它掩掉。
        sb.Append(" msg=").Append(Escape(Redactor.Scrub(message)));

        if (!string.IsNullOrEmpty(detail))
        {
            sb.Append(" detail=").Append(Escape(Redactor.Scrub(detail)));
        }

        if (elapsedMs.HasValue)
        {
            sb.Append(" elapsedMs=").Append(elapsedMs.Value.ToString(CultureInfo.InvariantCulture));
        }

        lock (_gate)
        {
            if (_writer == null)
            {
                return;
            }

            _writer.WriteLine(sb.ToString());
            _writer.Flush();
        }
    }

    public void Info(string phase, string message, string? detail = null)
    {
        Write(LogLevel.Info, phase, message, null, detail);
    }

    public void Warn(string phase, string message, ErrorCode? code = null, string? detail = null)
    {
        Write(LogLevel.Warn, phase, message, code, detail);
    }

    /// <summary>
    /// 异常出口。只记录错误码与内部异常类型，不记录 inner message——
    /// 底层异常消息里经常夹带路径、地址甚至令牌，属于必须挡在日志之外的内容。
    /// </summary>
    public void Failure(string phase, ErrorCode code, Exception? exception = null, string? detail = null)
    {
        string type = exception == null ? "none" : exception.GetType().FullName ?? "unknown";
        Write(LogLevel.Error, phase, ErrorCodes.Id(code) + " " + ExceptionSummary(exception), code, detail);
        Write(LogLevel.Debug, phase, "exception-type=" + type);
    }

    private static string ExceptionSummary(Exception? exception)
    {
        if (exception == null)
        {
            return "no-exception";
        }

        string type = exception.GetType().Name;
        return type + " hresult=0x" + exception.HResult.ToString("x8", CultureInfo.InvariantCulture);
    }

    public void Dispose()
    {
        lock (_gate)
        {
            if (_disposed)
            {
                return;
            }

            _disposed = true;
            if (_writer != null)
            {
                _writer.Flush();
                _writer.Dispose();
                _writer = null;
            }
        }
    }

    private void OpenWriter()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(_filePath) ?? ".");
        _writer = new StreamWriter(new FileStream(_filePath, FileMode.Create, FileAccess.Write, FileShare.Read), new UTF8Encoding(false));
        _writer.AutoFlush = false;
    }

    private static void Rotate(string logDirectory)
    {
        try
        {
            string[] files = Directory.GetFiles(logDirectory, "session-*.log");
            if (files.Length <= RetainedSessionFiles)
            {
                return;
            }

            Array.Sort(files, StringComparer.Ordinal);
            int removeCount = files.Length - RetainedSessionFiles;
            for (int i = 0; i < removeCount; i++)
            {
                try
                {
                    File.Delete(files[i]);
                }
                catch (IOException)
                {
                    // 轮转失败不影响启动：旧日志留着就留着。
                }
            }
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    private static string Escape(string? value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return "\"\"";
        }

        StringBuilder sb = new StringBuilder(value!.Length + 2);
        sb.Append('"');

        foreach (char c in value)
        {
            switch (c)
            {
                case '"': sb.Append("\\\""); break;
                case '\\': sb.Append("\\\\"); break;
                case '\r': sb.Append("\\r"); break;
                case '\n': sb.Append("\\n"); break;
                case '\t': sb.Append("\\t"); break;
                default: sb.Append(c); break;
            }
        }

        sb.Append('"');
        return sb.ToString();
    }
}
