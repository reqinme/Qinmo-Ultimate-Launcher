using System;
using System.Collections.Generic;
using System.Threading;
using Qul.Application.Ports;
using Qul.Domain.Runtime;

namespace Qul.Infrastructure.Runtime;

/// <summary>
/// 本机探测来源（MVP）。
/// 枚举所有能找到的 Java；找不到不是错误，只是空列表。
/// </summary>
public sealed class DetectedJavaRuntimeProvider : IJavaRuntimeProvider
{
    private static readonly JavaRuntimeCapabilities Declared = new JavaRuntimeCapabilities
    {
        CanEnumerate = true,
        CanFetch = false,
        RequiresLicenseNotice = false,
        CanUninstall = false,
        EnabledByDefault = true,
    };

    private readonly JavaInstallationScanner _scanner;

    public DetectedJavaRuntimeProvider(JavaInstallationScanner? scanner = null)
    {
        _scanner = scanner ?? new JavaInstallationScanner();
    }

    public string Name => "detected";

    public JavaRuntimeCapabilities Capabilities => Declared;

    public IReadOnlyList<JavaRuntimeCandidate> Discover(CancellationToken cancellationToken)
    {
        return _scanner.Scan(cancellationToken);
    }
}

/// <summary>
/// 用户手动指定来源（MVP）。
///
/// 路径通过委托读取而不是构造时快照，这样用户在设置里改了路径之后
/// 不用重建对象就能生效。
/// </summary>
public sealed class ManualJavaRuntimeProvider : IJavaRuntimeProvider
{
    private static readonly JavaRuntimeCapabilities Declared = new JavaRuntimeCapabilities
    {
        CanEnumerate = false,
        CanFetch = false,
        RequiresLicenseNotice = false,
        CanUninstall = false,
        EnabledByDefault = true,
    };

    private static readonly IReadOnlyList<JavaRuntimeCandidate> None = Array.Empty<JavaRuntimeCandidate>();

    private readonly JavaExecutableProbe _probe;
    private readonly Func<string?> _pathAccessor;

    public ManualJavaRuntimeProvider(JavaExecutableProbe probe, Func<string?> pathAccessor)
    {
        _probe = probe ?? throw new ArgumentNullException(nameof(probe));
        _pathAccessor = pathAccessor ?? throw new ArgumentNullException(nameof(pathAccessor));
    }

    public string Name => "manual";

    public JavaRuntimeCapabilities Capabilities => Declared;

    public IReadOnlyList<JavaRuntimeCandidate> Discover(CancellationToken cancellationToken)
    {
        string? path = _pathAccessor();
        if (string.IsNullOrWhiteSpace(path))
        {
            return None;
        }

        JavaRuntimeCandidate? candidate = _probe.ProbeCached(path!.Trim(), cancellationToken);
        if (candidate == null)
        {
            return None;
        }

        candidate.Source = JavaRuntimeSource.Manual;
        return new[] { candidate };
    }

    /// <summary>校验用户刚输入或刚选择的路径。界面用它即时反馈，而不是等启动时才发现不对。</summary>
    public JavaRuntimeCandidate? Validate(string path, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return null;
        }

        JavaRuntimeCandidate? candidate = _probe.ProbeCached(path.Trim(), cancellationToken);
        if (candidate != null)
        {
            candidate.Source = JavaRuntimeSource.Manual;
        }

        return candidate;
    }
}
