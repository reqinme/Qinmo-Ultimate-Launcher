using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using Qul.Application.Ports;
using Qul.Application.Update;
using Qul.Domain.Diagnostics;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.Serialization;

namespace Qul.Infrastructure.Update;

/// <summary>
/// 更新服务：检查 → 下载 → 校验 → 应用。
///
/// 三条不可让步的纪律：
///   1. **没有摘要就不下载**（发布清单里 sha256 为空的一律跳过）
///   2. **摘要不符就丢弃**，绝不进入替换流程
///   3. 更新只在用户显式要求时应用——不静默替换自己
/// </summary>
public sealed class UpdateService
{
    private const string PendingFileName = "pending.exe";

    private readonly IHttpTransport _transport;
    private readonly UpdateStore _store;
    private readonly SessionLog _log;

    public UpdateService(IHttpTransport transport, UpdateStore store, SessionLog? log = null)
    {
        _transport = transport ?? throw new ArgumentNullException(nameof(transport));
        _store = store ?? throw new ArgumentNullException(nameof(store));
        _log = log ?? SessionLog.Null;
    }

    /// <summary>把发布清单解析成模型。格式不认识就当作"没有可用更新"，不抛给用户。</summary>
    public static ReleaseManifest ParseManifest(string json)
    {
        JsonObject root = JsonValue.Parse(json).RequireObject();

        int formatVersion = root.GetInt("formatVersion") ?? 0;
        if (formatVersion != ReleaseManifest.SupportedFormatVersion)
        {
            throw new LauncherException(ErrorCode.UpdFailed, "unsupported release manifest format: " + formatVersion.ToString(CultureInfo.InvariantCulture));
        }

        List<UpdateRelease> releases = new List<UpdateRelease>();
        JsonArray? array = root.GetArray("releases");

        if (array != null)
        {
            for (int i = 0; i < array.Count; i++)
            {
                if (!(array[i] is JsonObject item))
                {
                    continue;
                }

                releases.Add(new UpdateRelease
                {
                    Version = item.GetString("version") ?? string.Empty,
                    DownloadUrl = item.GetString("downloadUrl") ?? string.Empty,
                    Sha256 = (item.GetString("sha256") ?? string.Empty).Trim().ToLowerInvariant(),
                    SizeBytes = item.GetLong("sizeBytes") ?? 0,
                    MinimumVersion = item.GetString("minimumVersion"),
                    Notes = item.GetString("notes"),
                });
            }
        }

        return new ReleaseManifest { FormatVersion = formatVersion, Releases = releases };
    }

    /// <summary>取清单并挑出该升到的版本。没有可用更新时返回 null。</summary>
    public UpdateRelease? Check(string manifestUrl, string currentVersion, CancellationToken cancellationToken)
    {
        string text;

        using (HttpFetchResponse response = _transport.Fetch(
            new HttpFetchRequest { Url = manifestUrl, Accept = "application/json", Timeout = TimeSpan.FromSeconds(20) },
            cancellationToken))
        {
            if (response.Status != HttpFetchStatus.Success)
            {
                throw new LauncherException(ErrorCode.NetHttpStatus, "release manifest returned " + response.StatusCode.ToString(CultureInfo.InvariantCulture));
            }

            using (StreamReader reader = new StreamReader(response.Content, Encoding.UTF8))
            {
                text = reader.ReadToEnd();
            }
        }

        ReleaseManifest manifest = ParseManifest(text);
        UpdateRelease? release = manifest.Pick(currentVersion);

        _log.Info("update", release == null
            ? "no applicable release; current=" + currentVersion
            : "release available: " + release.Version);

        return release;
    }

    /// <summary>
    /// 下载并校验。返回待应用文件的完整路径。**任何一步不通过都不写标记、不碰主程序。**
    /// </summary>
    public string Download(UpdateRelease release, CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(release.Sha256))
        {
            throw new LauncherException(ErrorCode.UpdFailed, "release has no sha256; refusing to download");
        }

        System.IO.Directory.CreateDirectory(_store.Directory);

        string target = Path.Combine(_store.Directory, PendingFileName);
        string part = target + ".part";

        using (HttpFetchResponse response = _transport.Fetch(
            new HttpFetchRequest { Url = release.DownloadUrl, Timeout = TimeSpan.FromMinutes(10) },
            cancellationToken))
        {
            if (response.Status != HttpFetchStatus.Success)
            {
                throw new LauncherException(ErrorCode.NetHttpStatus, "download returned " + response.StatusCode.ToString(CultureInfo.InvariantCulture));
            }

            using (FileStream file = new FileStream(part, FileMode.Create, FileAccess.Write, FileShare.None))
            {
                response.Content.CopyTo(file);
            }
        }

        string actual = ComputeSha256(part);

        if (!string.Equals(actual, release.Sha256, StringComparison.OrdinalIgnoreCase))
        {
            SafeDelete(part);
            _log.Failure("update", ErrorCode.DlChecksumMismatch, new LauncherException(
                ErrorCode.DlChecksumMismatch, "expected " + release.Sha256 + " but got " + actual));

            throw new LauncherException(ErrorCode.DlChecksumMismatch, "下载的文件校验未通过，已丢弃");
        }

        if (File.Exists(target))
        {
            File.Delete(target);
        }

        File.Move(part, target);
        _log.Info("update", "downloaded and verified " + release.Version);

        return target;
    }

    /// <summary>
    /// 应用更新：先落标记，再换文件。
    ///
    /// **标记必须先落盘**，否则断电时无从判断"上次替换到底发生没发生"。
    /// 换文件由 <see cref="SelfReplacer"/> 完成，失败时它会在进程内把原文件改回来。
    /// </summary>
    public SelfReplaceOutcome Apply(
        string programPath,
        string pendingFilePath,
        UpdateRelease release,
        string backupFileName)
    {
        string programDirectory = Path.GetDirectoryName(programPath) ?? ".";
        string backupPath = _store.ResolveBackupPath(programDirectory, backupFileName);

        string currentVersion = typeof(UpdateService).Assembly.GetName().Version?.ToString() ?? "0.0.0";

        _store.SavePending(new PendingUpdate
        {
            TargetVersion = release.Version,
            PendingFileName = Path.GetFileName(pendingFilePath),
            BackupFileName = Path.GetFileName(backupPath),
            Sha256 = release.Sha256,
            Stage = PendingUpdate.StageSwapping,
            StartedAt = DateTimeOffset.UtcNow,
        });

        _log.Info("update", "applying " + release.Version + " over " + currentVersion);

        SelfReplaceOutcome outcome = SelfReplacer.Apply(programPath, pendingFilePath, backupPath);

        if (!outcome.Succeeded)
        {
            // 替换没发生或已被改回，标记就没有意义了；留着只会让下次启动白跑一趟。
            _store.ClearPending();
            _log.Failure("update", ErrorCode.UpdFailed, new LauncherException(
                ErrorCode.UpdFailed, outcome.FailureReason ?? "apply failed"));
        }

        return outcome;
    }

    /// <summary>应用成功、且应用真正起来之后调用。删掉标记才算这次更新活下来了。</summary>
    private static string ComputeSha256(string path)
    {
        using (SHA256 sha = SHA256.Create())
        using (FileStream file = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            byte[] hash = sha.ComputeHash(file);
            StringBuilder builder = new StringBuilder(hash.Length * 2);

            for (int i = 0; i < hash.Length; i++)
            {
                builder.Append(hash[i].ToString("x2", CultureInfo.InvariantCulture));
            }

            return builder.ToString();
        }
    }

    private static void SafeDelete(string path)
    {
        try
        {
            if (File.Exists(path))
            {
                File.Delete(path);
            }
        }
        catch (IOException)
        {
        }
    }
}
