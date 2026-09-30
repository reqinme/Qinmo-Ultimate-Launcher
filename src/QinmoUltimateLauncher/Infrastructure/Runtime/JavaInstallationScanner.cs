using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using Qul.Domain.Runtime;

namespace Qul.Infrastructure.Runtime;

/// <summary>
/// 本机 Java 安装扫描。
///
/// 目录名只用于**快速筛选候选**，从不用于判定版本或架构——最终一律以
/// <see cref="JavaExecutableProbe"/> 的实际执行结果为准（尖刺 S5 的结论）。
///
/// 覆盖的位置来自实测：PATH、JAVA_HOME、以及若干厂商根目录。
/// 本机就同时存在 Oracle 默认目录、Azul Zulu，以及一个不含 java.exe 的空壳目录。
/// </summary>
public sealed class JavaInstallationScanner
{
    private readonly JavaExecutableProbe _probe;

    public JavaInstallationScanner(JavaExecutableProbe? probe = null)
    {
        _probe = probe ?? new JavaExecutableProbe();
    }

    /// <summary>厂商安装根目录。它们的一级与二级子目录会被检查。</summary>
    public static IReadOnlyList<string> VendorRoots()
    {
        List<string> roots = new List<string>();

        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Java");
        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Eclipse Adoptium");
        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Zulu");
        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Microsoft");
        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Amazon Corretto");
        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "BellSoft");
        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Semeru");
        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "AdoptOpenJDK");
        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "RedHat");
        Add(roots, Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "SapMachine");

        string programFilesX86 = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86);
        Add(roots, programFilesX86, "Java");
        Add(roots, programFilesX86, "Zulu");
        Add(roots, programFilesX86, "Eclipse Adoptium");

        string localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        Add(roots, localAppData, "Programs", "Eclipse Adoptium");

        return roots.Where(Directory.Exists).Distinct(StringComparer.OrdinalIgnoreCase).ToList();
    }

    public IReadOnlyList<JavaRuntimeCandidate> Scan(CancellationToken cancellationToken = default)
    {
        List<string> executables = CollectExecutablePaths(cancellationToken);

        List<JavaRuntimeCandidate> found = new List<JavaRuntimeCandidate>();

        for (int i = 0; i < executables.Count; i++)
        {
            cancellationToken.ThrowIfCancellationRequested();

            JavaRuntimeCandidate? candidate = _probe.ProbeCached(executables[i], cancellationToken);
            if (candidate == null)
            {
                // 目录在、但不能跑或无法识别：跳过它，其余候选照常。
                continue;
            }

            candidate.Source = JavaRuntimeSource.Detected;
            found.Add(candidate);
        }

        return found;
    }

    /// <summary>
    /// 收集候选可执行文件路径。排序后再探测，保证候选顺序可复现。
    /// </summary>
    public static List<string> CollectExecutablePaths(CancellationToken cancellationToken = default)
    {
        List<string> executables = new List<string>();
        HashSet<string> seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

        void Consider(string? path)
        {
            if (string.IsNullOrWhiteSpace(path))
            {
                return;
            }

            string full;
            try
            {
                full = Path.GetFullPath(path!);
            }
            catch (ArgumentException)
            {
                return;
            }
            catch (NotSupportedException)
            {
                return;
            }
            catch (PathTooLongException)
            {
                return;
            }

            if (seen.Add(full))
            {
                executables.Add(full);
            }
        }

        cancellationToken.ThrowIfCancellationRequested();

        string? javaHome = Environment.GetEnvironmentVariable("JAVA_HOME");
        if (!string.IsNullOrEmpty(javaHome))
        {
            Consider(Path.Combine(javaHome!, "bin", "java.exe"));
        }

        string? pathVariable = Environment.GetEnvironmentVariable("PATH");
        if (!string.IsNullOrEmpty(pathVariable))
        {
            foreach (string entry in pathVariable!.Split(';'))
            {
                string trimmed = entry.Trim().Trim('"');
                if (trimmed.Length > 0)
                {
                    Consider(Path.Combine(trimmed, "java.exe"));
                }
            }
        }

        foreach (string root in VendorRoots())
        {
            foreach (string level1 in SafeDirectories(root))
            {
                Consider(Path.Combine(level1, "bin", "java.exe"));

                foreach (string level2 in SafeDirectories(level1))
                {
                    Consider(Path.Combine(level2, "bin", "java.exe"));
                }
            }
        }

        // 官方启动器自带的运行时：runtime/<组件>/<平台>/<组件>/bin/java.exe
        string appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        if (!string.IsNullOrEmpty(appData))
        {
            string runtimeRoot = Path.Combine(appData, ".minecraft", "runtime");
            foreach (string component in SafeDirectories(runtimeRoot))
            {
                foreach (string platform in SafeDirectories(component))
                {
                    foreach (string inner in SafeDirectories(platform))
                    {
                        Consider(Path.Combine(inner, "bin", "java.exe"));
                    }
                }
            }
        }

        // 排序让候选顺序稳定；探测结果的排序另有规则（见 JavaRuntimeSelector）。
        executables.Sort(StringComparer.OrdinalIgnoreCase);
        return executables;
    }

    private static IReadOnlyList<string> SafeDirectories(string path)
    {
        try
        {
            return Directory.Exists(path) ? Directory.GetDirectories(path) : Array.Empty<string>();
        }
        catch (IOException)
        {
            return Array.Empty<string>();
        }
        catch (UnauthorizedAccessException)
        {
            return Array.Empty<string>();
        }
    }

    private static void Add(List<string> roots, string? basePath, params string[] segments)
    {
        if (string.IsNullOrEmpty(basePath))
        {
            return;
        }

        string combined = basePath!;
        foreach (string segment in segments)
        {
            combined = Path.Combine(combined, segment);
        }

        roots.Add(combined);
    }
}
