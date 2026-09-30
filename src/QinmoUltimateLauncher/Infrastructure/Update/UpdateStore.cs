using System;
using System.Globalization;
using System.IO;
using System.Text;
using Qul.Application.Update;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.Serialization;

namespace Qul.Infrastructure.Update;

/// <summary>
/// 待应用更新标记的读写。
///
/// 它承担两件事：告诉"下一次启动"有个更新要装，
/// 以及——更重要——**判断上一次更新到底活下来没有**。
///
/// 新版本启动成功才删标记，所以"标记还在"就等于"上次替换之后从未成功启动过"。
/// 这个判断不依赖新版本自报成功（它可能根本没机会报），因而是可靠的。
/// </summary>
public sealed class UpdateStore
{
    private const string PendingFileName = "pending.json";

    private readonly string _directory;
    private readonly SessionLog _log;

    public UpdateStore(string updatesDirectory, SessionLog? log = null)
    {
        if (string.IsNullOrWhiteSpace(updatesDirectory))
        {
            throw new ArgumentException("updates directory is required", nameof(updatesDirectory));
        }

        _directory = updatesDirectory!;
        _log = log ?? SessionLog.Null;
    }

    public string Directory => _directory;

    public string PendingFilePath => Path.Combine(_directory, PendingFileName);

    /// <summary>
    /// 判断是否应当回退。
    /// 条件：存在标记，且阶段仍是 swapping——说明替换动作发起过，但之后从未成功启动。
    /// </summary>
    public bool ShouldRollback(out PendingUpdate? pending)
    {
        pending = LoadPending();

        if (pending == null)
        {
            return false;
        }

        if (!pending.IsSwapping)
        {
            return false;
        }

        if (string.IsNullOrWhiteSpace(pending.BackupFileName))
        {
            _log.Warn("update", "pending marker has no backup name; discarding", ErrorCode.UpdFailed);
            ClearPending();
            return false;
        }

        return true;
    }

    public PendingUpdate? LoadPending()
    {
        try
        {
            if (!File.Exists(PendingFilePath))
            {
                return null;
            }

            JsonObject root = JsonValue.Parse(File.ReadAllText(PendingFilePath, Encoding.UTF8)).RequireObject();

            return new PendingUpdate
            {
                TargetVersion = root.GetString("targetVersion") ?? string.Empty,
                PendingFileName = root.GetString("pendingFile") ?? string.Empty,
                BackupFileName = root.GetString("backupFile") ?? string.Empty,
                Sha256 = root.GetString("sha256") ?? string.Empty,
                Stage = root.GetString("stage") ?? PendingUpdate.StageSwapping,
                StartedAt = ParseTime(root.GetString("startedAt")),
            };
        }
        catch (Exception ex) when (ex is IOException || ex is JsonFormatException)
        {
            // 标记读不出来不能让人开不了程序。当作没有标记处理。
            _log.Warn("update", "pending marker unreadable; ignoring", ErrorCode.UpdFailed);
            return null;
        }
    }

    /// <summary>写入标记。**必须在执行替换之前落盘**，否则断电时无从判断。</summary>
    public void SavePending(PendingUpdate pending)
    {
        if (pending == null)
        {
            throw new ArgumentNullException(nameof(pending));
        }

        System.IO.Directory.CreateDirectory(_directory);

        JsonObject root = new JsonObject()
            .Set("formatVersion", 1)
            .Set("targetVersion", pending.TargetVersion)
            .Set("pendingFile", pending.PendingFileName)
            .Set("backupFile", pending.BackupFileName)
            .Set("sha256", pending.Sha256)
            .Set("stage", pending.Stage)
            .Set("startedAt", pending.StartedAt.ToUniversalTime().ToString("o", CultureInfo.InvariantCulture));

        File.WriteAllText(PendingFilePath, root.ToJson(false), new UTF8Encoding(false));
    }

    /// <summary>新版本确认启动成功后调用。删掉标记即代表本次更新"活下来了"。</summary>
    public void ClearPending()
    {
        try
        {
            if (File.Exists(PendingFilePath))
            {
                File.Delete(PendingFilePath);
            }
        }
        catch (IOException)
        {
        }
        catch (UnauthorizedAccessException)
        {
        }
    }

    /// <summary>
    /// 备份文件的完整路径。
    ///
    /// **路径白名单**：只允许落在主程序所在目录或 updates 目录内，
    /// 其余一律拒绝。这样"回退不碰用户数据"是由代码保证的，而不是靠自觉。
    /// </summary>
    public string ResolveBackupPath(string programDirectory, string backupFileName)
    {
        if (string.IsNullOrWhiteSpace(backupFileName))
        {
            throw new LauncherException(ErrorCode.UpdFailed, "backup file name is empty");
        }

        // 只取文件名，丢掉任何目录成分——防止标记文件里塞进 "..\..\game" 这类路径。
        string safeName = Path.GetFileName(backupFileName);
        if (safeName.Length == 0)
        {
            throw new LauncherException(ErrorCode.UpdFailed, "backup file name is invalid");
        }

        return Path.Combine(programDirectory, safeName);
    }

    private static DateTimeOffset ParseTime(string? text)
    {
        return DateTimeOffset.TryParse(text, CultureInfo.InvariantCulture, DateTimeStyles.RoundtripKind, out DateTimeOffset value)
            ? value
            : DateTimeOffset.MinValue;
    }
}
