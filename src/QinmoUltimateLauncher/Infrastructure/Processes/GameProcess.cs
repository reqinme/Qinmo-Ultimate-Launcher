using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading.Tasks;
using Qul.Domain.Diagnostics;
using Qul.Domain.Launch;
using Qul.Infrastructure.Diagnostics;

namespace Qul.Infrastructure.Processes;

public sealed class GameProcessOptions
{
    public string ExecutablePath { get; set; } = string.Empty;

    public IReadOnlyList<string> Arguments { get; set; } = Array.Empty<string>();

    public string WorkingDirectory { get; set; } = string.Empty;

    public IReadOnlyDictionary<string, string> Environment { get; set; } =
        new Dictionary<string, string>(0, StringComparer.Ordinal);

    /// <summary>游戏 stdout/stderr 落盘的位置。输出是排障时最有用的东西，绝不能丢。</summary>
    public string LogFilePath { get; set; } = string.Empty;
}

public sealed class GameProcessResult
{
    public GameProcessResult(int exitCode, TimeSpan duration, string logFilePath, ErrorCode? error)
    {
        ExitCode = exitCode;
        Duration = duration;
        LogFilePath = logFilePath;
        Error = error;
    }

    public int ExitCode { get; }

    public TimeSpan Duration { get; }

    public string LogFilePath { get; }

    public ErrorCode? Error { get; }

    public bool Succeeded => Error == null;
}

/// <summary>
/// 拉起游戏进程并回收结果。
///
/// 三件事必须做对：
///   1) 命令行引号（类路径里含空格路径，引号错了就是"启动失败且看不出原因"）；
///   2) 两个输出流并发读并落盘，否则管道写满会把游戏卡死；
///   3) 退出码要被翻译成可读结论，而不是原样抛给用户。
/// </summary>
public sealed class GameProcess : IDisposable
{
    private readonly Process _process;
    private readonly StreamWriter? _logWriter;
    private readonly Task[] _pumps;
    private readonly object _writerGate = new object();
    private readonly DateTimeOffset _startedAt;
    private bool _disposed;

    private GameProcess(Process process, StreamWriter? logWriter, Task[] pumps, string logFilePath)
    {
        _process = process;
        _logWriter = logWriter;
        _pumps = pumps;
        LogFilePath = logFilePath;
        _startedAt = DateTimeOffset.Now;
    }

    public int ProcessId
    {
        get
        {
            try
            {
                return _process.Id;
            }
            catch (InvalidOperationException)
            {
                return 0;
            }
        }
    }

    public string LogFilePath { get; }

    public bool HasExited
    {
        get
        {
            try
            {
                return _process.HasExited;
            }
            catch (InvalidOperationException)
            {
                return true;
            }
        }
    }

    public static bool TryStart(
        GameProcessOptions options,
        SessionLog? log,
        out GameProcess? process,
        out ErrorCode error)
    {
        process = null;
        error = ErrorCode.None;

        if (options == null)
        {
            throw new ArgumentNullException(nameof(options));
        }

        if (string.IsNullOrWhiteSpace(options.ExecutablePath) || !File.Exists(options.ExecutablePath))
        {
            error = ErrorCode.JavaInvalid;
            return false;
        }

        // 工作目录可能还没被创建过——首次启动恰好就是这种情况。
        // 不建的话 Process.Start 会以"目录名无效"失败，而那个错误完全看不出真正原因。
        if (!string.IsNullOrEmpty(options.WorkingDirectory))
        {
            try
            {
                Directory.CreateDirectory(options.WorkingDirectory);
            }
            catch (IOException)
            {
                error = ErrorCode.IoDataRootNotWritable;
                return false;
            }
            catch (UnauthorizedAccessException)
            {
                error = ErrorCode.IoDataRootNotWritable;
                return false;
            }
        }
        ProcessStartInfo startInfo = new ProcessStartInfo
        {
            FileName = options.ExecutablePath,
            Arguments = CommandLineBuilder.Build(options.Arguments),
            WorkingDirectory = string.IsNullOrEmpty(options.WorkingDirectory)
                ? Environment.CurrentDirectory
                : options.WorkingDirectory,
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
        };

        foreach (KeyValuePair<string, string> pair in options.Environment)
        {
            startInfo.EnvironmentVariables[pair.Key] = pair.Value;
        }

        StreamWriter? writer = null;

        try
        {
            if (!string.IsNullOrEmpty(options.LogFilePath))
            {
                string? directory = Path.GetDirectoryName(options.LogFilePath);
                if (!string.IsNullOrEmpty(directory))
                {
                    Directory.CreateDirectory(directory!);
                }

                writer = new StreamWriter(
                    new FileStream(options.LogFilePath, FileMode.Create, FileAccess.Write, FileShare.ReadWrite),
                    new UTF8Encoding(false));
            }
        }
        catch (IOException)
        {
            // 写不了日志不是拒绝启动的理由：进程照起，只是没有输出记录。
            writer = null;
        }
        catch (UnauthorizedAccessException)
        {
            writer = null;
        }

        Process? started = null;

        try
        {
            started = new Process { StartInfo = startInfo, EnableRaisingEvents = true };

            if (!started.Start())
            {
                error = ErrorCode.ProcStartFailed;
                started.Dispose();
                writer?.Dispose();
                return false;
            }

            // 两个流必须并发读：游戏日志能到几 MB，管道写满会把游戏卡死。
            object gate = new object();
            Task[] pumps =
            {
                Task.Run(() => Pump(started.StandardOutput, writer, gate)),
                Task.Run(() => Pump(started.StandardError, writer, gate)),
            };

            process = new GameProcess(started, writer, pumps, options.LogFilePath);
            log?.Info("process", "game process started", "pid=" + process.ProcessId);
            return true;
        }
        catch (Win32Exception ex)
        {
            error = ErrorCode.ProcStartFailed;
            log?.Failure("process", error, ex);
        }
        catch (InvalidOperationException ex)
        {
            error = ErrorCode.ProcStartFailed;
            log?.Failure("process", error, ex);
        }

        started?.Dispose();
        writer?.Dispose();
        return false;
    }

    /// <summary>阻塞等游戏结束，回收退出码。启动器退出前调用它。</summary>
    public GameProcessResult WaitForExit()
    {
        try
        {
            _process.WaitForExit();
        }
        catch (InvalidOperationException)
        {
            return new GameProcessResult(-1, DateTimeOffset.Now - _startedAt, LogFilePath, ErrorCode.ProcStartFailed);
        }

        DrainPumps();

        int exitCode = _process.ExitCode;

        return new GameProcessResult(
            exitCode,
            DateTimeOffset.Now - _startedAt,
            LogFilePath,
            exitCode == 0 ? (ErrorCode?)null : ErrorCode.ProcNonZeroExit);
    }

    /// <summary>等在给定时间内结束。超时不杀进程，只如实返回 false。</summary>
    public bool WaitForExit(TimeSpan timeout)
    {
        try
        {
            if (!_process.WaitForExit((int)Math.Max(0, timeout.TotalMilliseconds)))
            {
                return false;
            }
        }
        catch (InvalidOperationException)
        {
            return true;
        }

        DrainPumps();
        return true;
    }

    public void Kill()
    {
        try
        {
            if (!_process.HasExited)
            {
                _process.Kill();
            }
        }
        catch (InvalidOperationException)
        {
        }
        catch (Win32Exception)
        {
        }
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;

        try
        {
            DrainPumps();
        }
        catch (AggregateException)
        {
        }

        _process.Dispose();

        lock (_writerGate)
        {
            _logWriter?.Flush();
            _logWriter?.Dispose();
        }
    }

    private void DrainPumps()
    {
        try
        {
            // 进程已退出时，管道里可能还有尾巴；给一个上限，别把界面挂住。
            Task.WaitAll(_pumps, TimeSpan.FromSeconds(5));
        }
        catch (AggregateException)
        {
        }

        lock (_writerGate)
        {
            _logWriter?.Flush();
        }
    }

    private static void Pump(TextReader reader, StreamWriter? writer, object gate)
    {
        char[] buffer = new char[4096];

        try
        {
            int read;
            while ((read = reader.Read(buffer, 0, buffer.Length)) > 0)
            {
                if (writer == null)
                {
                    continue;
                }

                lock (gate)
                {
                    writer.Write(buffer, 0, read);
                    writer.Flush();
                }
            }
        }
        catch (IOException)
        {
            // 进程被杀时读端会抛 —— 这不是错误，输出到此为止。
        }
        catch (ObjectDisposedException)
        {
        }
        catch (InvalidOperationException)
        {
        }
    }
}
