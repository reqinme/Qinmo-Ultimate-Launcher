using System;
using System.IO;
using Qul.Domain.Diagnostics;

namespace Qul.Infrastructure.IO;

/// <summary>数据根的实际落地方式。</summary>
public enum DataRootPlacement
{
    /// <summary>便携模式：数据与 exe 同级，随目录一起搬走。</summary>
    Portable = 0,

    /// <summary>回退模式：exe 目录不可写，数据落到用户目录。</summary>
    UserProfile = 1,
}

/// <summary>
/// 运行时目录布局的唯一权威。
/// 全工程禁止在别处拼接数据/缓存/游戏目录路径——路径一律经过这里，
/// 这样超长路径（QUL-IO-0003）、中文与空格路径才有统一的处置点。
/// </summary>
public sealed class DataLayout
{
    public const string AppFolderName = "QinmoUltimateLauncher";

    private DataLayout(string executableDirectory, string dataRoot, string gameRoot, DataRootPlacement placement)
    {
        ExecutableDirectory = executableDirectory;
        DataRoot = dataRoot;
        GameRoot = gameRoot;
        Placement = placement;
    }

    public string ExecutableDirectory { get; }

    public string DataRoot { get; }

    public string GameRoot { get; }

    public DataRootPlacement Placement { get; }

    public string ConfigFile => Path.Combine(DataRoot, "config.json");

    public string StateDirectory => Path.Combine(DataRoot, "state");

    public string LogDirectory => Path.Combine(DataRoot, "logs");

    public string CacheDirectory => Path.Combine(DataRoot, "cache");

    public string CacheMetaDirectory => Path.Combine(CacheDirectory, "meta");

    public string CacheLibrariesDirectory => Path.Combine(CacheDirectory, "libraries");

    public string CacheObjectsDirectory => Path.Combine(CacheDirectory, "objects");

    public string NativesDirectory => Path.Combine(CacheDirectory, "natives");

    /// <summary>仅存放 DPAPI 密文。明文令牌永不落盘。</summary>
    public string SecretsDirectory => Path.Combine(DataRoot, "secrets");

    /// <summary>
    /// 解析数据根：便携优先，exe 目录不可写时回退到用户目录。
    /// 该决定必须尽早做出，并且必须记入启动日志——否则用户排障时找不到自己的数据。
    /// </summary>
    public static DataLayout Resolve(string executableDirectory, string userProfileDirectory)
    {
        if (string.IsNullOrWhiteSpace(executableDirectory))
        {
            throw new ArgumentException("executable directory is required", nameof(executableDirectory));
        }

        if (string.IsNullOrWhiteSpace(userProfileDirectory))
        {
            throw new ArgumentException("user profile directory is required", nameof(userProfileDirectory));
        }

        string portableData = Path.Combine(executableDirectory, "data");
        DataRootPlacement placement = DataRootPlacement.Portable;
        string dataRoot = portableData;

        if (!IsDirectoryWritable(executableDirectory))
        {
            placement = DataRootPlacement.UserProfile;
            dataRoot = Path.Combine(userProfileDirectory, AppFolderName, "data");
        }

        // 游戏目录默认与数据根同级放置，便于整目录搬迁。
        string gameRoot = Path.Combine(Path.GetDirectoryName(dataRoot) ?? dataRoot, "game");

        return new DataLayout(executableDirectory, dataRoot, gameRoot, placement);
    }

    /// <summary>
    /// 首次运行建目录。返回创建过程中遇到的第一个非致命问题（用于日志），全部成功则为 null。
    /// 目录已存在不算问题。
    /// </summary>
    public LauncherException? EnsureCreated()
    {
        string[] required =
        {
            DataRoot,
            StateDirectory,
            LogDirectory,
            CacheMetaDirectory,
            CacheLibrariesDirectory,
            CacheObjectsDirectory,
            NativesDirectory,
            SecretsDirectory,
            GameRoot,
        };

        foreach (string path in required)
        {
            try
            {
                if (IsPathTooLong(path))
                {
                    return new LauncherException(ErrorCode.IoPathTooLong, ErrorCodes.Id(ErrorCode.IoPathTooLong));
                }

                Directory.CreateDirectory(path);
            }
            catch (PathTooLongException ex)
            {
                return LauncherException.Wrap(ErrorCode.IoPathTooLong, ex);
            }
            catch (UnauthorizedAccessException ex)
            {
                return LauncherException.Wrap(ErrorCode.IoDataRootNotWritable, ex);
            }
            catch (IOException ex)
            {
                return LauncherException.Wrap(ErrorCode.IoDataRootNotWritable, ex);
            }
        }

        return null;
    }

    /// <summary>
    /// 主动做长度检查，而不是等系统抛异常。
    /// S7 已确认本机启用了长路径支持——也就是说，本机测不出问题恰恰是因为它开着。
    /// 因此不能把"能否创建"当作判据，必须自己算。
    /// </summary>
    public static bool IsPathTooLong(string path)
    {
        if (string.IsNullOrEmpty(path))
        {
            return false;
        }

        // 保守阈值：留出目录名扩展与临时文件名后缀的余量。
        const int ConservativeLimit = 240;
        return path.Length > ConservativeLimit;
    }

    private static bool IsDirectoryWritable(string directory)
    {
        try
        {
            if (!Directory.Exists(directory))
            {
                return false;
            }

            string probe = Path.Combine(directory, ".qinmo-write-probe");
            using (FileStream stream = new FileStream(probe, FileMode.Create, FileAccess.Write, FileShare.None))
            {
                stream.WriteByte(0);
            }

            File.Delete(probe);
            return true;
        }
        catch (Exception)
        {
            return false;
        }
    }
}
