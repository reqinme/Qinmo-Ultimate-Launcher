using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using Qul.Domain.Diagnostics;
using Qul.Domain.Runtime;
using Qul.Infrastructure.Diagnostics;

namespace Qul.Infrastructure.Runtime;

/// <summary>
/// 通过实际执行 java.exe 来确认它的版本与架构。
///
/// **这是唯一可信的判据。** 尖刺 S5 已确认目录名不可信——本机既有 jdk-25.0.4.1 这类四段命名，
/// 也有一个不含 java.exe 的空壳目录 latest。
///
/// 用 <c>-XshowSettings:properties -version</c>：它直接给出 java.version / os.arch /
/// sun.arch.data.model，不必从 "64-Bit Server VM" 这类文案里猜。
/// 实测该输出**全部走 stderr**（stdout 为 0 字节）。
/// </summary>
public sealed class JavaExecutableProbe
{
    private static readonly TimeSpan DefaultTimeout = TimeSpan.FromSeconds(8);

    private readonly Dictionary<string, JavaRuntimeCandidate?> _cache =
        new Dictionary<string, JavaRuntimeCandidate?>(StringComparer.OrdinalIgnoreCase);

    private readonly SessionLog _log;
    private readonly object _gate = new object();

    public JavaExecutableProbe(SessionLog? log = null)
    {
        _log = log ?? SessionLog.Null;
    }

    /// <summary>带缓存地探测。同一个可执行文件在一次会话里只跑一次。</summary>
    public JavaRuntimeCandidate? ProbeCached(string executablePath, CancellationToken cancellationToken = default)
    {
        lock (_gate)
        {
            if (_cache.TryGetValue(executablePath, out JavaRuntimeCandidate? cached))
            {
                return cached;
            }
        }

        JavaRuntimeCandidate? probed = Probe(executablePath, cancellationToken);

        lock (_gate)
        {
            _cache[executablePath] = probed;
        }

        return probed;
    }

    public JavaRuntimeCandidate? Probe(string executablePath, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(executablePath) || !File.Exists(executablePath))
        {
            return null;
        }

        // 先试权威形态；个别老版本或裁剪过的运行时可能不支持，再退回 -version。
        if (TryRun(executablePath, "-XshowSettings:properties -version", cancellationToken, out string stdout, out string stderr)
            && JavaProbeOutputParser.TryParse(stdout, stderr, out JavaProbeResult detailed))
        {
            return Build(executablePath, detailed);
        }

        if (TryRun(executablePath, "-version", cancellationToken, out stdout, out stderr)
            && JavaProbeOutputParser.TryParse(stdout, stderr, out JavaProbeResult basic))
        {
            return Build(executablePath, basic);
        }

        _log.Warn("java", "probe failed", ErrorCode.JavaInvalid, executablePath);
        return null;
    }

    private static JavaRuntimeCandidate Build(string executablePath, JavaProbeResult probe)
    {
        return new JavaRuntimeCandidate
        {
            ExecutablePath = executablePath,
            Version = probe.Version,
            OsArch = probe.OsArch,
            Bitness = probe.Bitness,
            Vendor = probe.Vendor,
            VmName = probe.VmName,
            Source = JavaRuntimeSource.Detected,
        };
    }

    private bool TryRun(
        string executablePath,
        string arguments,
        CancellationToken cancellationToken,
        out string stdout,
        out string stderr)
    {
        stdout = string.Empty;
        stderr = string.Empty;

        ProcessStartInfo startInfo = new ProcessStartInfo
        {
            FileName = executablePath,
            Arguments = arguments,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            WorkingDirectory = Path.GetDirectoryName(executablePath) ?? Environment.CurrentDirectory,
        };

        Process? process = null;

        try
        {
            process = Process.Start(startInfo);
            if (process == null)
            {
                return false;
            }

            // 两个流必须并发读：属性输出有 5KB 左右，超过管道缓冲区，
            // 顺序读完一个再读另一个会死锁。
            Task<string> outTask = process.StandardOutput.ReadToEndAsync();
            Task<string> errTask = process.StandardError.ReadToEndAsync();

            if (!process.WaitForExit((int)DefaultTimeout.TotalMilliseconds))
            {
                Kill(process);
                return false;
            }

            Task.WaitAll(outTask, errTask);
            stdout = outTask.Result;
            stderr = errTask.Result;
            return true;
        }
        catch (Exception ex)
        {
            _log.Warn("java", "cannot execute java", ErrorCode.JavaInvalid, ex.GetType().Name);
            Kill(process);
            return false;
        }
        finally
        {
            process?.Dispose();
            cancellationToken.ThrowIfCancellationRequested();
        }
    }

    private static void Kill(Process? process)
    {
        try
        {
            if (process != null && !process.HasExited)
            {
                process.Kill();
            }
        }
        catch (InvalidOperationException)
        {
        }
        catch (System.ComponentModel.Win32Exception)
        {
        }
    }
}
