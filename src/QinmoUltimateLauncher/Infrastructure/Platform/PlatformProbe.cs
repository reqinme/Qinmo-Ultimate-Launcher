using System;
using System.Collections.Generic;
using Qul.Domain.Metadata;

namespace Qul.Infrastructure.Platform;

/// <summary>
/// 运行环境探测。放在基础设施层：领域层被禁止引用进程与系统 API，
/// 因此真实探测在这里发生，规则求值本身仍是纯逻辑。
/// </summary>
public static class PlatformProbe
{
    public static EnvironmentProfile Current(IReadOnlyDictionary<string, bool>? features = null)
    {
        return new EnvironmentProfile(DetectOsName(), DetectArchitecture(), DetectOsVersion(), features);
    }

    public static string DetectOsName()
    {
        // 本项目只面向 Windows；其余取值保留是为了让 profile 在别的平台上仍可构造，
        // 而不是声称已经支持那些平台。
        switch (Environment.OSVersion.Platform)
        {
            case PlatformID.Win32NT:
                return EnvironmentProfile.OsWindows;
            case PlatformID.Unix:
                return EnvironmentProfile.OsLinux;
            default:
                return EnvironmentProfile.OsWindows;
        }
    }

    /// <summary>
    /// 探测**机器**架构。
    ///
    /// 注意：natives 需要匹配的是 **JVM** 的架构，不是机器的架构。
    /// 32 位 JVM 跑在 64 位 Windows 上时需要的是 natives-windows-32。
    /// 因此这里给出的只是默认值，P3 选定 Java 之后必须以 JVM 的架构覆盖它。
    /// </summary>
    public static string DetectArchitecture()
    {
        // 32 位进程在 64 位 Windows 上时，PROCESSOR_ARCHITEW6432 给出真实的机器架构。
        string? processor = Environment.GetEnvironmentVariable("PROCESSOR_ARCHITEW6432");
        if (string.IsNullOrEmpty(processor))
        {
            processor = Environment.GetEnvironmentVariable("PROCESSOR_ARCHITECTURE");
        }

        if (!string.IsNullOrEmpty(processor))
        {
            switch (processor!.Trim().ToUpperInvariant())
            {
                case "AMD64":
                case "IA64":
                    return EnvironmentProfile.ArchX86_64;
                case "ARM64":
                    return EnvironmentProfile.ArchArm64;
                case "X86":
                    return EnvironmentProfile.ArchX86;
            }
        }

        return Environment.Is64BitOperatingSystem ? EnvironmentProfile.ArchX86_64 : EnvironmentProfile.ArchX86;
    }

    /// <summary>os.version 规则用的是正则匹配，这里给出点分三段的版本号供其匹配。</summary>
    public static string DetectOsVersion()
    {
        Version version = Environment.OSVersion.Version;
        return version.Major + "." + version.Minor + "." + version.Build;
    }

    /// <summary>
    /// 按**选定的 Java 运行时**构造环境画像。
    ///
    /// natives 与 os.arch 规则要匹配的是 JVM 的架构，不是机器的架构：
    /// 32 位 JVM 跑在 64 位 Windows 上时需要的是 natives-windows-32。
    /// JVM 自己报告的 os.arch 就是它自己的架构，所以这里以它为准；
    /// 只有拿不到时才回落到机器架构。
    /// </summary>
    public static EnvironmentProfile ForJavaRuntime(
        Qul.Domain.Runtime.JavaRuntimeCandidate? runtime,
        IReadOnlyDictionary<string, bool>? features = null)
    {
        string? arch = runtime?.OsArch;
        if (string.IsNullOrEmpty(arch))
        {
            arch = DetectArchitecture();
        }

        return new EnvironmentProfile(DetectOsName(), arch!, DetectOsVersion(), features);
    }
}
