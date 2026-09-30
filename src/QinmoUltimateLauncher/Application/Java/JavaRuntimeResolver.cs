using System;
using System.Collections.Generic;
using System.Globalization;
using System.Threading;
using Qul.Application.Ports;
using Qul.Domain.Diagnostics;
using Qul.Domain.Runtime;

namespace Qul.Application.Java;

public sealed class JavaResolutionOutcome
{
    public JavaResolutionOutcome(
        JavaSelectionResult selection,
        IReadOnlyList<JavaRuntimeCandidate> candidates,
        IReadOnlyList<string> notes)
    {
        Selection = selection;
        Candidates = candidates;
        Notes = notes;
    }

    public JavaSelectionResult Selection { get; }

    /// <summary>合并去重后的全部候选。</summary>
    public IReadOnlyList<JavaRuntimeCandidate> Candidates { get; }

    /// <summary>每个来源产出了多少、手动路径是否被采用——"说得清"那一条的原料。</summary>
    public IReadOnlyList<string> Notes { get; }

    public bool Succeeded => Selection.Succeeded;
}

/// <summary>
/// 把多个运行时来源合并成一次选择。
///
/// 覆盖语义：**手动指定优先**。只要手动路径可跑且满足要求就用它；
/// 手动路径无效或版本不够时才回落到本机探测，并在 Notes 里说明回落原因——
/// 静默回落会让用户以为自己的设置生效了。
/// </summary>
public sealed class JavaRuntimeResolver
{
    private readonly IReadOnlyList<IJavaRuntimeProvider> _providers;

    public JavaRuntimeResolver(IReadOnlyList<IJavaRuntimeProvider> providers)
    {
        _providers = providers ?? throw new ArgumentNullException(nameof(providers));
    }

    public JavaResolutionOutcome Resolve(JavaSelectionRequest request, CancellationToken cancellationToken = default)
    {
        if (request == null)
        {
            throw new ArgumentNullException(nameof(request));
        }

        List<string> notes = new List<string>();
        List<JavaRuntimeCandidate> all = new List<JavaRuntimeCandidate>();
        HashSet<string> seenPaths = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        List<JavaRuntimeCandidate> manual = new List<JavaRuntimeCandidate>();

        for (int i = 0; i < _providers.Count; i++)
        {
            IJavaRuntimeProvider provider = _providers[i];
            if (provider == null)
            {
                continue;
            }

            // 来源不可枚举时不算失败，它本来就只提供自己那一个。
            if (!provider.Capabilities.CanEnumerate && provider.Name != "manual")
            {
                continue;
            }

            IReadOnlyList<JavaRuntimeCandidate> found;
            try
            {
                found = provider.Discover(cancellationToken) ?? Array.Empty<JavaRuntimeCandidate>();
            }
            catch (OperationCanceledException)
            {
                throw;
            }
            catch (Exception ex)
            {
                notes.Add("来源 " + provider.Name + " 枚举失败：" + ex.GetType().Name);
                continue;
            }

            int accepted = 0;
            for (int j = 0; j < found.Count; j++)
            {
                JavaRuntimeCandidate candidate = found[j];
                if (!seenPaths.Add(candidate.ExecutablePath))
                {
                    continue;
                }

                all.Add(candidate);
                accepted++;

                if (candidate.Source == JavaRuntimeSource.Manual)
                {
                    manual.Add(candidate);
                }
            }

            notes.Add("来源 " + provider.Name + "：" + accepted.ToString(CultureInfo.InvariantCulture) + " 个可用运行时");
        }

        if (manual.Count > 0)
        {
            JavaSelectionResult manualSelection = JavaRuntimeSelector.Select(manual, request);
            if (manualSelection.Succeeded)
            {
                notes.Add("已采用手动指定的 Java");
                return new JavaResolutionOutcome(manualSelection, all, notes);
            }

            notes.Add("手动指定的 Java 不满足要求，已回落到本机探测：" + (manualSelection.Explanation ?? string.Empty));
        }
        else if (HasManualProvider())
        {
            notes.Add("手动指定的路径不可用（文件不存在或无法执行），已回落到本机探测");
        }

        JavaSelectionResult selection = JavaRuntimeSelector.Select(all, request);
        return new JavaResolutionOutcome(selection, all, notes);
    }

    private bool HasManualProvider()
    {
        for (int i = 0; i < _providers.Count; i++)
        {
            if (_providers[i] != null && _providers[i].Name == "manual")
            {
                return true;
            }
        }

        return false;
    }
}
