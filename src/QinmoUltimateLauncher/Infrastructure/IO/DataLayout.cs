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

    /// <summary>资源根。启动参数里的 ${assets_root} 指向它，游戏在其下找 indexes/ 与 objects/。</summary>
    public string CacheAssetsDirectory => Path.Combine(CacheDirectory, "assets");

    /// <summary>资源索引目录。游戏只认 indexes 这个名字。</summary>
    public string CacheAssetIndexesDirectory => Path.Combine(CacheAssetsDirectory, "indexes");

    /// <summary>资源对象目录。游戏只认 objects 这个名字，且必须在 assets 之下。</summary>
    public string CacheObjectsDirectory => Path.Combine(CacheAssetsDirectory, "objects");

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

        // 探测的必须是「便携数据根本身能不能建出来」，而不是「exe 目录可不可写」。
        // 两者不等价：exe 目录可写、但 data 这个名字已被一个文件占住时，
        // 按 exe 目录判断会选便携模式，随后建目录失败——程序照跑，却一个日志都不写，
        // 排障时人会发现"哪都没有数据"。
        if (!CanUsePortableRoot(portableData))
        {
            placement = DataRootPlacement.UserProfile;
            dataRoot = Path.Combine(userProfileDirectory, AppFolderName, "data");
        }

        // 游戏目录默认与数据根同级放置，便于整目录搬迁。
        string gameRoot = Path.Combine(Path.GetDirectoryName(dataRoot) ?? dataRoot, "game");

        return new DataLayout(executableDirectory, dataRoot, gameRoot, placement);
    }

    /// <summary>
    /// 便携数据根是否真的可用：先把它建出来，再往里写一个探针文件。
    /// 只判断"父目录可写"是不够的——同名文件占位、权限被改、路径过长都会让它建不出来。
    /// </summary>
    private static bool CanUsePortableRoot(string portableData)
    {
        // Resolve 只做决定，不产生副作用：探测过程中建出来的目录要收掉。
        // 真正的创建由 EnsureCreated 负责。否则便携与回退两条路径会不对称——
        // 一条顺手把目录建了、另一条没有。
        bool existed = System.IO.Directory.Exists(portableData);
        bool usable = false;

        try
        {
            System.IO.Directory.CreateDirectory(portableData);

            string probe = Path.Combine(portableData, ".qul-writable-probe");
            File.WriteAllText(probe, string.Empty);
            File.Delete(probe);
            usable = true;
        }
        catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException || ex is NotSupportedException)
        {
            usable = false;
        }

        if (!existed && System.IO.Directory.Exists(portableData))
        {
            try
            {
                System.IO.Directory.Delete(portableData, false);
            }
            catch (IOException)
            {
            }
        }

        return usable;
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
            CacheAssetsDirectory,
            CacheAssetIndexesDirectory,
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
