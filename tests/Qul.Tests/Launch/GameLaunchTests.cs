using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Text;
using System.Threading;
using Microsoft.VisualStudio.TestTools.UnitTesting;
using Qul.Application.Launch;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Downloads;
using Qul.Domain.Identity;
using Qul.Domain.Launch;
using Qul.Domain.Metadata;
using Qul.Domain.Runtime;
using Qul.Infrastructure.Launch;
using Qul.Infrastructure.Metadata;
using Qul.Infrastructure.Processes;
using Qul.Infrastructure.Runtime;

namespace Qul.Tests.Launch;

/// <summary>命令行引号规则。类路径里含空格路径，引号错了就是"启动失败且看不出原因"。</summary>
[TestClass]
public sealed class CommandLineBuilderTests
{
    [TestMethod]
    public void PlainArguments_AreNotQuoted()
    {
        Assert.AreEqual("--username", CommandLineBuilder.Quote("--username"));
        Assert.AreEqual(@"C:\no\spaces\here.jar", CommandLineBuilder.Quote(@"C:\no\spaces\here.jar"));
        Assert.AreEqual("\"\"", CommandLineBuilder.Quote(string.Empty), "空参数在 Windows 上必须写成 \"\"");
    }

    [TestMethod]
    public void ArgumentsWithSpaces_AreQuoted()
    {
        Assert.AreEqual(
            @"""C:\Program Files\Java\bin\java.exe""",
            CommandLineBuilder.Quote(@"C:\Program Files\Java\bin\java.exe"));

        Assert.AreEqual(
            "--username \"a b\" --version 1.16.5",
            CommandLineBuilder.Build(new[] { "--username", "a b", "--version", "1.16.5" }));
    }

    [TestMethod]
    public void TrailingBackslash_IsDoubledInsideQuotes()
    {
        // 不翻倍的话，末尾反斜杠会把闭合引号转义掉，参数就被截断成两半。
        Assert.AreEqual(
            @"""C:\my dir\\""",
            CommandLineBuilder.Quote(@"C:\my dir\"));
    }

    [TestMethod]
    public void EmbeddedQuotes_AreEscaped()
    {
        Assert.AreEqual(
            @"""say \""hi\""""",
            CommandLineBuilder.Quote("say \"hi\""));
    }
}

/// <summary>natives 选择规则：两代机制、模板值、悬空引用。</summary>
[TestClass]
public sealed class NativeSelectionTests
{
    private static readonly EnvironmentProfile WindowsX64 =
        new EnvironmentProfile(EnvironmentProfile.OsWindows, EnvironmentProfile.ArchX86_64);

    [TestMethod]
    public void ResolvesLegacyMappingTemplateAndModernClassifier()
    {
        // 第一代：普通映射
        LibraryRef legacy = new LibraryRef
        {
            RawName = "org.lwjgl.lwjgl:lwjgl-platform:2.9.1",
            Natives = new Dictionary<string, string>(StringComparer.Ordinal) { { "windows", "natives-windows" } },
            Classifiers = new Dictionary<string, DownloadRef>(StringComparer.Ordinal)
            {
                { "natives-windows", new DownloadRef("https://example.invalid/w.jar") },
            },
        };

        Assert.IsTrue(NativeExtractor.TryResolveArchive(legacy, WindowsX64, out DownloadRef? archive, out bool isLegacy));
        Assert.IsTrue(isLegacy);
        Assert.AreEqual("https://example.invalid/w.jar", archive!.Url);

        // 第一代：模板值 + 位宽选择
        LibraryRef templated = new LibraryRef
        {
            RawName = "tv.twitch:twitch-platform:5.16",
            Natives = new Dictionary<string, string>(StringComparer.Ordinal) { { "windows", "natives-windows-${arch}" } },
            Classifiers = new Dictionary<string, DownloadRef>(StringComparer.Ordinal)
            {
                { "natives-windows-32", new DownloadRef("https://example.invalid/32.jar") },
                { "natives-windows-64", new DownloadRef("https://example.invalid/64.jar") },
            },
        };

        Assert.IsTrue(NativeExtractor.TryResolveArchive(templated, WindowsX64, out DownloadRef? templatedArchive, out _));
        Assert.AreEqual("https://example.invalid/64.jar", templatedArchive!.Url);

        // 第一代：悬空引用 → 没有可解压的包，而不是抛异常
        LibraryRef dangling = new LibraryRef
        {
            RawName = "tv.twitch:twitch-platform:5.16",
            Natives = new Dictionary<string, string>(StringComparer.Ordinal) { { "linux", "natives-linux" } },
            Classifiers = new Dictionary<string, DownloadRef>(StringComparer.Ordinal),
        };

        Assert.IsFalse(NativeExtractor.TryResolveArchive(dangling, WindowsX64, out _, out _));

        // 第二代：坐标自带 classifier
        LibraryName.TryParse("com.mojang:jtracy:1.14.38:natives-windows", out LibraryName? modernName);
        LibraryRef modern = new LibraryRef
        {
            RawName = "com.mojang:jtracy:1.14.38:natives-windows",
            Name = modernName,
            Artifact = new DownloadRef("https://example.invalid/jtracy.jar"),
        };

        Assert.IsTrue(NativeExtractor.TryResolveArchive(modern, WindowsX64, out DownloadRef? modernArchive, out bool modernIsLegacy));
        Assert.IsFalse(modernIsLegacy);
        Assert.AreEqual("https://example.invalid/jtracy.jar", modernArchive!.Url);

        // 普通库不该被当成 natives
        LibraryRef ordinary = new LibraryRef { RawName = "com.mojang:netty:1.8.8" };
        Assert.IsFalse(NativeExtractor.TryResolveArchive(ordinary, WindowsX64, out _, out _));
    }

    [TestMethod]
    public void RealVersions_YieldTheExpectedNumberOfNativeArchives()
    {
        VersionDetail legacy = LoadVersion("1.7.10");

        int legacyCount = 0;
        foreach (LibraryRef library in legacy.Libraries)
        {
            if (NativeExtractor.TryResolveArchive(library, WindowsX64, out _, out _))
            {
                legacyCount++;
            }
        }

        Assert.AreEqual(4, legacyCount, "1.7.10 在 Windows 上应有 4 个可解压的 natives 包");

        VersionDetail modern = LoadVersion("26.3");

        int modernCount = 0;
        foreach (LibraryRef library in modern.Libraries)
        {
            if (NativeExtractor.TryResolveArchive(library, WindowsX64, out _, out bool isLegacy2))
            {
                Assert.IsFalse(isLegacy2, "26.x 不该走到 natives 映射那条路");
                modernCount++;
            }
        }

        Assert.IsTrue(modernCount > 0, "26.x 的 natives 靠坐标 classifier 表达，必须能被识别出来");
    }

    private static VersionDetail LoadVersion(string id)
    {
        string path = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "Golden", "version-" + id + ".json");
        Assert.IsTrue(File.Exists(path), "缺少黄金样本：" + path);
        return VersionMetadataParser.ParseVersion(File.ReadAllText(path, Encoding.UTF8), id);
    }
}

/// <summary>natives 解压：落盘正确、排除项生效、恶意包整包拒绝。</summary>
[TestClass]
public sealed class NativeExtractionTests
{
    private const string CacheRoot = @"C:\qul\data\cache";

    private string? _sandbox;

    [TestCleanup]
    public void Cleanup()
    {
        if (_sandbox != null && Directory.Exists(_sandbox))
        {
            try
            {
                Directory.Delete(_sandbox, recursive: true);
            }
            catch (IOException)
            {
            }
        }
    }

    [TestMethod]
    public void ExtractsNativeArchiveAndHonoursExclusions()
    {
        string sandbox = NewSandbox();
        string cacheRoot = Path.Combine(sandbox, "cache");
        string natives = Path.Combine(sandbox, "natives");

        LibraryRef library = LegacyNativeLibrary("natives-windows");
        string relative = new CachePathConventions().LibraryFile(null, library.Name, library.RawName);
        string jarPath = Path.Combine(cacheRoot, relative.Replace('/', '\\'));
        Directory.CreateDirectory(Path.GetDirectoryName(jarPath)!);

        CreateZip(jarPath, ("lwjgl.dll", "binary"), ("META-INF/MANIFEST.MF", "manifest"));

        VersionDetail version = new VersionDetail
        {
            Id = "test",
            Libraries = new[] { library },
        };

        NativeExtractionResult result = NativeExtractor.Extract(
            version, WindowsProfile(), cacheRoot, natives);

        Assert.IsTrue(result.Succeeded, result.Error?.ToString());
        Assert.AreEqual(1, result.ArchivesExtracted);
        Assert.AreEqual(1, result.FilesExtracted);
        Assert.IsTrue(File.Exists(Path.Combine(natives, "lwjgl.dll")));
        Assert.IsFalse(Directory.Exists(Path.Combine(natives, "META-INF")), "排除项下的内容不得落盘");
    }

    [TestMethod]
    public void ReportMissingArchiveInsteadOfFailing()
    {
        string sandbox = NewSandbox();

        VersionDetail version = new VersionDetail
        {
            Id = "test",
            Libraries = new[] { LegacyNativeLibrary("natives-windows") },
        };

        NativeExtractionResult result = NativeExtractor.Extract(
            version, WindowsProfile(), Path.Combine(sandbox, "cache"), Path.Combine(sandbox, "natives"));

        Assert.IsNull(result.Error, "包还没下载不是解压的错误");
        Assert.AreEqual(1, result.MissingArchives.Count);
        Assert.AreEqual(0, result.ArchivesExtracted);
    }

    [TestMethod]
    public void RejectsMaliciousArchiveWholesale()
    {
        string sandbox = NewSandbox();
        string cacheRoot = Path.Combine(sandbox, "cache");
        string natives = Path.Combine(sandbox, "natives");

        LibraryRef library = LegacyNativeLibrary("natives-windows");
        string relative = new CachePathConventions().LibraryFile(null, library.Name, library.RawName);
        string jarPath = Path.Combine(cacheRoot, relative.Replace('/', '\\'));
        Directory.CreateDirectory(Path.GetDirectoryName(jarPath)!);

        CreateZip(jarPath, ("ok.dll", "fine"), ("../escaped.dll", "pwned"));

        VersionDetail version = new VersionDetail { Id = "test", Libraries = new[] { library } };

        NativeExtractionResult result = NativeExtractor.Extract(
            version, WindowsProfile(), cacheRoot, natives);

        Assert.AreEqual(1, result.RejectedArchives.Count, "含越界条目的包必须被整包拒绝");
        Assert.AreEqual(0, result.FilesExtracted, "整包拒绝时一个字节都不该写出");
        Assert.IsFalse(File.Exists(Path.Combine(sandbox, "escaped.dll")));
    }

    [TestMethod]
    public void CleanTargetRemovesStaleFiles()
    {
        string sandbox = NewSandbox();
        string cacheRoot = Path.Combine(sandbox, "cache");
        string natives = Path.Combine(sandbox, "natives");

        Directory.CreateDirectory(natives);
        File.WriteAllText(Path.Combine(natives, "stale-from-old-version.dll"), "x");

        LibraryRef library = LegacyNativeLibrary("natives-windows");
        string relative = new CachePathConventions().LibraryFile(null, library.Name, library.RawName);
        string jarPath = Path.Combine(cacheRoot, relative.Replace('/', '\\'));
        Directory.CreateDirectory(Path.GetDirectoryName(jarPath)!);
        CreateZip(jarPath, ("fresh.dll", "binary"));

        VersionDetail version = new VersionDetail { Id = "test", Libraries = new[] { library } };

        NativeExtractor.Extract(version, WindowsProfile(), cacheRoot, natives, cleanTarget: true);

        Assert.IsFalse(File.Exists(Path.Combine(natives, "stale-from-old-version.dll")), "上一次的残留必须被清掉");
        Assert.IsTrue(File.Exists(Path.Combine(natives, "fresh.dll")));
    }

    // ---------- 工具 ----------

    private static EnvironmentProfile WindowsProfile()
    {
        return new EnvironmentProfile(EnvironmentProfile.OsWindows, EnvironmentProfile.ArchX86_64);
    }

    private static LibraryRef LegacyNativeLibrary(string classifierKey)
    {
        LibraryName.TryParse("org.lwjgl.lwjgl:lwjgl-platform:2.9.1", out LibraryName? name);

        return new LibraryRef
        {
            RawName = "org.lwjgl.lwjgl:lwjgl-platform:2.9.1",
            Name = name,
            Natives = new Dictionary<string, string>(StringComparer.Ordinal) { { "windows", classifierKey } },
            Classifiers = new Dictionary<string, DownloadRef>(StringComparer.Ordinal)
            {
                { classifierKey, new DownloadRef("https://example.invalid/n.jar") },
            },
            ExtractExclude = new[] { "META-INF/" },
        };
    }

    private string NewSandbox()
    {
        _sandbox = Path.Combine(Path.GetTempPath(), "qul-p4", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_sandbox);
        return _sandbox;
    }

    private static void CreateZip(string path, params (string Name, string Content)[] entries)
    {
        using (FileStream stream = new FileStream(path, FileMode.Create, FileAccess.Write))
        using (ZipArchive archive = new ZipArchive(stream, ZipArchiveMode.Create))
        {
            foreach ((string name, string content) in entries)
            {
                ZipArchiveEntry entry = archive.CreateEntry(name);
                using (Stream body = entry.Open())
                using (StreamWriter writer = new StreamWriter(body, new UTF8Encoding(false)))
                {
                    writer.Write(content);
                }
            }
        }
    }
}

/// <summary>进程管理：输出捕获、退出码映射、启动失败分类。</summary>
[TestClass]
public sealed class GameProcessTests
{
    private string? _sandbox;

    [TestCleanup]
    public void Cleanup()
    {
        if (_sandbox != null && Directory.Exists(_sandbox))
        {
            try
            {
                Directory.Delete(_sandbox, recursive: true);
            }
            catch (IOException)
            {
            }
        }
    }

    [TestMethod]
    public void CapturesOutputAndMapsZeroExitCode()
    {
        string java = RequireJava();
        string sandbox = NewSandbox();
        string logPath = Path.Combine(sandbox, "game.log");

        GameProcessOptions options = new GameProcessOptions
        {
            ExecutablePath = java,
            Arguments = new[] { "-version" },
            WorkingDirectory = sandbox,
            LogFilePath = logPath,
        };

        Assert.IsTrue(GameProcess.TryStart(options, null, out GameProcess? process, out ErrorCode error), error.ToString());

        using (process!)
        {
            GameProcessResult result = process!.WaitForExit();

            Assert.AreEqual(0, result.ExitCode);
            Assert.IsNull(result.Error, "正常退出不该被标成错误");
            Assert.IsTrue(File.Exists(logPath), "输出必须落盘");
            StringAssert.Contains(LogFileReader.Read(logPath), "version");
        }
    }

    [TestMethod]
    public void MapsNonZeroExitCodeToProcNonZeroExit()
    {
        string java = RequireJava();
        string sandbox = NewSandbox();

        GameProcessOptions options = new GameProcessOptions
        {
            ExecutablePath = java,
            Arguments = new[] { "-XX:+NoSuchOptionAtAll" },
            WorkingDirectory = sandbox,
            LogFilePath = Path.Combine(sandbox, "game.log"),
        };

        Assert.IsTrue(GameProcess.TryStart(options, null, out GameProcess? process, out _));

        using (process!)
        {
            GameProcessResult result = process!.WaitForExit();

            Assert.AreNotEqual(0, result.ExitCode);
            Assert.AreEqual(ErrorCode.ProcNonZeroExit, result.Error);
        }
    }

    [TestMethod]
    public void ReportsMissingExecutableWithoutStarting()
    {
        string sandbox = NewSandbox();

        GameProcessOptions options = new GameProcessOptions
        {
            ExecutablePath = Path.Combine(sandbox, "no-such-java.exe"),
            Arguments = new[] { "-version" },
        };

        Assert.IsFalse(GameProcess.TryStart(options, null, out GameProcess? process, out ErrorCode error));
        Assert.IsNull(process);
        Assert.AreEqual(ErrorCode.JavaInvalid, error);
    }

    [TestMethod]
    public void QuotingSurvivesARealJvmRoundTrip()
    {
        // 这是引号规则的**真实往返验证**：把带空格与末尾反斜杠的值交给真 java，
        // 再从它自己的属性输出里读回来。引号规则错一个字符，这里就会断。
        string java = RequireJava();
        string sandbox = NewSandbox();

        const string SpacedValue = "hello world";
        const string TrailingBackslash = @"C:\my dir\";

        GameProcessOptions options = new GameProcessOptions
        {
            ExecutablePath = java,
            // -D 必须排在 -version 之前。
            // 实测：排在 -version 之后的 JVM 选项根本不会被应用——属性不会出现，而且没有任何报错。
            // （真实启动里没有 -version，所以不受影响；这里踩到的是诊断姿势的坑。）
            Arguments = new[]
            {
                "-Dqul.probe=" + SpacedValue,
                "-Dqul.tail=" + TrailingBackslash,
                "-XshowSettings:properties",
                "-version",
            },
            WorkingDirectory = sandbox,
            LogFilePath = Path.Combine(sandbox, "probe.log"),
        };

        Assert.IsTrue(GameProcess.TryStart(options, null, out GameProcess? process, out ErrorCode error), error.ToString());

        using (process!)
        {
            process!.WaitForExit();
            string output = LogFileReader.Read(Path.Combine(sandbox, "probe.log"));

            StringAssert.Contains(output, "qul.probe = " + SpacedValue, "含空格的参数必须原样送达 JVM");
            StringAssert.Contains(output, @"qul.tail = " + TrailingBackslash, "末尾反斜杠必须原样送达 JVM");
        }
    }

    private static string RequireJava()
    {
        string? java = JavaInstallationScanner.CollectExecutablePaths().FirstOrDefault(File.Exists);
        Assert.IsNotNull(java, "本机没有可执行的 java.exe，无法验证进程链路");
        return java!;
    }

    private string NewSandbox()
    {
        _sandbox = Path.Combine(Path.GetTempPath(), "qul-proc", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_sandbox);
        return _sandbox;
    }
}

/// <summary>启动用例：准备阶段的门槛，以及用真 java 跑通"计划 → 引号 → 进程 → 退出码"整条链路。</summary>
[TestClass]
public sealed class GameLauncherTests
{
    private string? _sandbox;

    [TestCleanup]
    public void Cleanup()
    {
        if (_sandbox != null && Directory.Exists(_sandbox))
        {
            try
            {
                Directory.Delete(_sandbox, recursive: true);
            }
            catch (IOException)
            {
            }
        }
    }

    [TestMethod]
    public void Prepare_ReportsMissingClientJarInPlainLanguage()
    {
        string sandbox = NewSandbox();

        GameLaunchOutcome outcome = new GameLauncher().Prepare(new GameLaunchRequest
        {
            Plan = MinimalPlan(sandbox, createClientJar: false),
        });

        Assert.IsFalse(outcome.Prepared);
        Assert.AreEqual(ErrorCode.DlFailed, outcome.Error, "客户端 jar 没下载时该给人话，而不是让 JVM 抛 ClassNotFoundException");
    }

    [TestMethod]
    public void Prepare_SucceedsAndReportsWhatItDid()
    {
        string sandbox = NewSandbox();

        GameLaunchOutcome outcome = new GameLauncher().Prepare(new GameLaunchRequest
        {
            Plan = MinimalPlan(sandbox, createClientJar: true),
        });

        Assert.IsTrue(outcome.Prepared, outcome.Error?.ToString());
        Assert.IsNotNull(outcome.Plan);
        Assert.AreEqual(8, outcome.Plan!.Skeleton.JavaMajorRequirement);
        Assert.IsTrue(outcome.Notes.Any(n => n.Contains("骨架指纹")));
        Assert.IsTrue(outcome.Notes.Any(n => n.Contains("类路径")));
        Assert.IsTrue(outcome.Notes.Any(n => n.Contains("natives")));
    }

    [TestMethod]
    public void Launch_RunsTheWholeChainAndMapsTheExitCode()
    {
        string sandbox = NewSandbox();

        // 主类不存在，所以 JVM 必然以非零码退出——这正好用来验证整条链路是通的：
        // 计划 → 参数 → 引号 → 进程启动 → 输出落盘 → 退出码映射。
        LaunchPlanRequest plan = MinimalPlan(sandbox, createClientJar: true);
        plan.Version.MainClass = "qul.nonexistent.Main";

        GameLaunchOutcome outcome = new GameLauncher().Launch(new GameLaunchRequest
        {
            Plan = plan,
            GameLogPath = Path.Combine(sandbox, "logs", "game.log"),
        });

        Assert.IsTrue(outcome.Started, outcome.Error?.ToString());
        Assert.IsNotNull(outcome.Process);

        using (outcome.Process!)
        {
            GameProcessResult result = outcome.Process!.WaitForExit();

            Assert.AreNotEqual(0, result.ExitCode);
            Assert.AreEqual(ErrorCode.ProcNonZeroExit, result.Error);

            Assert.IsTrue(File.Exists(result.LogFilePath), "游戏输出必须落盘");
            StringAssert.Contains(
                LogFileReader.Read(result.LogFilePath),
                "nonexistent",
                "日志里应当能看到是哪个主类没找到");
        }
    }

    // ---------- 工具 ----------

    private string NewSandbox()
    {
        _sandbox = Path.Combine(Path.GetTempPath(), "qul-launch", Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_sandbox);
        return _sandbox;
    }

    private static LaunchPlanRequest MinimalPlan(string sandbox, bool createClientJar)
    {
        string cacheRoot = Path.Combine(sandbox, "data", "cache");

        if (createClientJar)
        {
            string jarPath = Path.Combine(cacheRoot, "meta", "client-test-1.0.jar");
            Directory.CreateDirectory(Path.GetDirectoryName(jarPath)!);

            // 一个合法的空 zip：让它当 jar 用不会引发"压缩包损坏"，好让失败点落在主类上。
            using (FileStream stream = new FileStream(jarPath, FileMode.Create, FileAccess.Write))
            using (new ZipArchive(stream, ZipArchiveMode.Create))
            {
            }
        }

        string? java = JavaInstallationScanner.CollectExecutablePaths().FirstOrDefault(File.Exists);
        Assert.IsNotNull(java, "本机没有 java.exe，无法验证启动链路");

        JavaVersion.TryParse("17.0.20", out JavaVersion version);

        return new LaunchPlanRequest
        {
            Version = new VersionDetail
            {
                Id = "test-1.0",
                Type = VersionType.Release,
                MainClass = "net.minecraft.client.main.Main",
                Assets = "test",
                ClientDownload = new DownloadRef("https://example.invalid/client.jar"),
                JavaVersion = new JavaVersionRequirement { MajorVersion = 8 },
                Libraries = Array.Empty<LibraryRef>(),
            },
            Environment = new EnvironmentProfile(EnvironmentProfile.OsWindows, EnvironmentProfile.ArchX86_64),
            Java = new JavaRuntimeCandidate
            {
                ExecutablePath = java!,
                Version = version,
                OsArch = "amd64",
                Bitness = 64,
            },
            Identity = OfflineIdentityFactory.Create("Player"),
            CacheRoot = cacheRoot,
            GameDirectory = Path.Combine(sandbox, "game"),
            NativesDirectory = Path.Combine(sandbox, "data", "cache", "natives"),
            MaxMemoryMb = 64,
        };
    }
}

/// <summary>
/// 读日志文件时允许共享。
/// GameProcess 在游戏运行期间一直持有写句柄；用默认的 FileShare.Read 去读会被系统拒绝，
/// 而"游戏还在跑所以读不了自己的日志"是个很蠢的限制。
/// </summary>
internal static class LogFileReader
{
    public static string Read(string path)
    {
        using (FileStream stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite))
        using (StreamReader reader = new StreamReader(stream))
        {
            return reader.ReadToEnd();
        }
    }
}