using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Reflection;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using Qul.Application.Diagnostics;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;
using Qul.Domain.Metadata;
using Qul.Infrastructure.Boot;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.Launch;

namespace Qul.Presentation;

public sealed class IdentityOption
{
    public IdentityOption(IdentitySource source, string title, string detail, bool enabled)
    {
        Source = source;
        Title = title;
        Detail = detail;
        Enabled = enabled;
    }

    public IdentitySource Source { get; }

    public string Title { get; }

    public string Detail { get; }

    public bool Enabled { get; }
}

/// <summary>
/// 主界面的状态机。
///
/// 它不自己实现启动链路，而是驱动 <see cref="LaunchPipeline"/>——
/// 与命令行走同一条路。
/// </summary>
public sealed class MainViewModel : ObservableObject
{
    private const int MaxLogLines = 1000;

    private readonly BootContext _boot;
    private readonly LaunchPipeline _pipeline;
    private readonly List<string> _logLines = new List<string>(MaxLogLines + 1);

    private CancellationTokenSource? _cancellation;

    private bool _isBusy;
    private string _statusText = "先选择版本与身份来源。";
    private string _logText = string.Empty;
    private string _offlineName = "Player";
    private string _serverTarget = string.Empty;
    private VersionSummary? _selectedVersion;
    private IdentityOption? _selectedIdentity;
    private bool _acknowledgementGiven;
    private bool _needsAcknowledgement;
    private bool _hasBlocking;
    private string _errorBanner = string.Empty;
    private string _resultSummary = string.Empty;
    private bool _diagnosticsReady;
    private string _diagnosticReport = string.Empty;

    private LaunchPipelineRequest? _lastRequest;
    private LaunchPipelineResult? _lastResult;

    public MainViewModel(BootContext boot)
    {
        _boot = boot ?? throw new ArgumentNullException(nameof(boot));
        _pipeline = new LaunchPipeline(boot);

        Versions = new ObservableCollection<VersionSummary>();
        Preflight = new ObservableCollection<PreflightItem>();

        RefreshCommand = new RelayCommand(() => _ = RefreshAsync(), () => !IsBusy);
        LaunchCommand = new RelayCommand(() => _ = LaunchAsync(), () => CanLaunch);
        CancelCommand = new RelayCommand(Cancel, () => IsBusy);
        CopyDiagnosticsCommand = new RelayCommand(CopyDiagnostics, () => DiagnosticsReady);
        OpenDataFolderCommand = new RelayCommand(OpenDataFolder);
        RecheckCommand = new RelayCommand(() => _ = RebuildPreflightAsync(), () => !IsBusy);

        IdentityOptions = BuildIdentityOptions();
        SelectedIdentity = IdentityOptions[0];
    }

    // ---------- 集合 ----------

    public ObservableCollection<VersionSummary> Versions { get; }

    public ObservableCollection<PreflightItem> Preflight { get; }

    public IReadOnlyList<IdentityOption> IdentityOptions { get; }

    // ---------- 命令 ----------

    public RelayCommand RefreshCommand { get; }

    public RelayCommand LaunchCommand { get; }

    public RelayCommand CancelCommand { get; }

    public RelayCommand CopyDiagnosticsCommand { get; }

    public RelayCommand OpenDataFolderCommand { get; }

    public RelayCommand RecheckCommand { get; }

    // ---------- 绑定属性 ----------

    public bool IsBusy
    {
        get => _isBusy;
        private set
        {
            if (Set(ref _isBusy, value))
            {
                UpdateCommands();
            }
        }
    }

    public string StatusText
    {
        get => _statusText;
        private set => Set(ref _statusText, value);
    }

    public string LogText
    {
        get => _logText;
        private set => Set(ref _logText, value);
    }

    public string OfflineName
    {
        get => _offlineName;
        set => Set(ref _offlineName, value);
    }

    public string ServerTarget
    {
        get => _serverTarget;
        set
        {
            if (Set(ref _serverTarget, value))
            {
                _ = RebuildPreflightAsync();
            }
        }
    }

    public VersionSummary? SelectedVersion
    {
        get => _selectedVersion;
        set
        {
            if (Set(ref _selectedVersion, value))
            {
                _ = RebuildPreflightAsync();
            }
        }
    }

    public IdentityOption? SelectedIdentity
    {
        get => _selectedIdentity;
        set
        {
            if (Set(ref _selectedIdentity, value))
            {
                // 换了身份来源就要重新确认能力告知——告知的内容跟着来源变。
                AcknowledgementGiven = false;
                _ = RebuildPreflightAsync();
            }
        }
    }

    /// <summary>
    /// 用户是否已确认能力告知。
    /// 这个勾不是装饰：<see cref="NeedsAcknowledgement"/> 为真时，没有它就不能启动。
    /// </summary>
    public bool AcknowledgementGiven
    {
        get => _acknowledgementGiven;
        set
        {
            if (Set(ref _acknowledgementGiven, value))
            {
                UpdateCommands();
            }
        }
    }

    public bool NeedsAcknowledgement
    {
        get => _needsAcknowledgement;
        private set => Set(ref _needsAcknowledgement, value);
    }

    public bool HasBlocking
    {
        get => _hasBlocking;
        private set => Set(ref _hasBlocking, value);
    }

    public string ErrorBanner
    {
        get => _errorBanner;
        private set
        {
            if (Set(ref _errorBanner, value))
            {
                Raise(nameof(HasError));
            }
        }
    }

    public bool HasError => _errorBanner.Length > 0;

    public string ResultSummary
    {
        get => _resultSummary;
        private set => Set(ref _resultSummary, value);
    }

    public bool DiagnosticsReady
    {
        get => _diagnosticsReady;
        private set
        {
            if (Set(ref _diagnosticsReady, value))
            {
                UpdateCommands();
            }
        }
    }

    /// <summary>
    /// 能否启动：不忙、预检无阻断项、且该确认的已确认。
    /// **三者缺一不可**，尤其是最后一条。
    /// </summary>
    public bool CanLaunch =>
        !IsBusy && !HasBlocking && (!NeedsAcknowledgement || AcknowledgementGiven);

    // ---------- 流程 ----------

    private async Task RefreshAsync()
    {
        IsBusy = true;
        ErrorBanner = string.Empty;
        StatusText = "正在读取版本清单…";

        try
        {
            Progress<string> progress = new Progress<string>(AppendLog);

            VersionManifest manifest = await Task.Run(
                () => _pipeline.FetchManifest(progress, CancellationToken.None));

            Versions.Clear();
            for (int i = 0; i < manifest.Versions.Count; i++)
            {
                Versions.Add(manifest.Versions[i]);
            }

            StatusText = "共 " + Versions.Count + " 个版本；最新正式版 " + manifest.LatestRelease;

            if (SelectedVersion == null && Versions.Count > 0)
            {
                SelectedVersion = FindNewestRelease(manifest);
            }
        }
        catch (LauncherException ex)
        {
            ErrorBanner = ErrorCodes.Id(ex.Code) + " " + ErrorCodes.Hint(ex.Code);
            StatusText = "版本清单读取失败。";
            AppendLog("版本清单读取失败：" + ex.Code);
        }
        finally
        {
            IsBusy = false;
        }

        await RebuildPreflightAsync();
    }

    private async Task RebuildPreflightAsync()
    {
        LaunchPipelineRequest request = BuildRequest(LaunchPipelineMode.Prepare);

        if (request.VersionId.Length == 0)
        {
            Preflight.Clear();
            NeedsAcknowledgement = false;
            HasBlocking = false;
            DiagnosticsReady = false;
            UpdateCommands();
            return;
        }

        IReadOnlyList<PreflightItem> items;
        try
        {
            items = await Task.Run(() => _pipeline.PreparePreflight(request));
        }
        catch (LauncherException)
        {
            return;
        }

        Preflight.Clear();
        for (int i = 0; i < items.Count; i++)
        {
            Preflight.Add(items[i]);
        }

        NeedsAcknowledgement = PreflightCheck.NeedsAcknowledgement(items);
        HasBlocking = PreflightCheck.HasBlocking(items);

        _lastRequest = request;
        _lastResult = new LaunchPipelineResult
        {
            Preflight = items,
            Version = null,
        };

        UpdateDiagnostics();
        UpdateCommands();
    }

    private async Task LaunchAsync()
    {
        if (!CanLaunch)
        {
            return;
        }

        LaunchPipelineRequest request = BuildRequest(LaunchPipelineMode.Launch);

        _cancellation = new CancellationTokenSource();
        IsBusy = true;
        ErrorBanner = string.Empty;
        ResultSummary = string.Empty;
        DiagnosticsReady = false;
        StatusText = "正在准备启动…";
        UpdateCommands();

        LaunchPipelineResult result;

        try
        {
            // Progress<T> 会捕获当前的同步上下文，所以这里从后台线程发出的进度
            // 会自动回到界面线程——顺序也保持不变。
            Progress<string> progress = new Progress<string>(AppendLog);

            result = await Task.Run(
                () => _pipeline.Run(request, progress, _cancellation!.Token));
        }
        catch (Exception ex)
        {
            result = new LaunchPipelineResult
            {
                Error = ErrorCode.DlFailed,
                ErrorDetail = ex.Message,
            };
        }
        finally
        {
            IsBusy = false;
            _cancellation?.Dispose();
            _cancellation = null;
        }

        ApplyResult(request, result);
    }

    private void ApplyResult(LaunchPipelineRequest request, LaunchPipelineResult result)
    {
        _lastRequest = request;
        _lastResult = result;

        if (result.Cancelled)
        {
            StatusText = "已取消。";
            ResultSummary = "已取消，没有产生任何改动。";
        }
        else if (!result.Succeeded)
        {
            ErrorCode code = result.Error ?? ErrorCode.DlFailed;
            ErrorBanner = ErrorCodes.Id(code) + " " + ErrorCodes.Hint(code);
            StatusText = "启动失败。";

            ResultSummary = string.IsNullOrWhiteSpace(result.ErrorDetail)
                ? "失败原因见上方的错误码。点「复制诊断信息」可以把定位所需的信息一次取走。"
                : result.ErrorDetail!;
        }
        else
        {
            StatusText = "已拉起游戏进程。";
            ResultSummary = "游戏窗口出现前请稍候；若长时间无窗口，请点「复制诊断信息」。";
        }

        UpdateDiagnostics();
        UpdateCommands();
    }

    private void Cancel()
    {
        try
        {
            _cancellation?.Cancel();
            StatusText = "正在取消…";
        }
        catch (ObjectDisposedException)
        {
        }
    }

    // ---------- 诊断 ----------

    private void UpdateDiagnostics()
    {
        LaunchPipelineResult? result = _lastResult;

        DiagnosticReportInput input = new DiagnosticReportInput
        {
            LauncherVersion = Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "0.0.0",
            DataPlacement = _boot.Layout.Placement.ToString(),
            IdentitySource = _lastRequest?.IdentitySource,
            IsOnlineVerified = _lastRequest?.IdentitySource != IdentitySource.Offline,
            VersionId = _lastRequest?.VersionId,
            SkeletonHash = result?.SkeletonHash,
            SelectedJava = result?.SelectedJava,
            JavaCandidates = result?.JavaCandidates ?? Array.Empty<Domain.Runtime.JavaRuntimeCandidate>(),
            Preflight = Preflight,
            Error = result?.Error,
            ErrorDetail = result?.ErrorDetail,
            Notes = result?.Notes ?? Array.Empty<string>(),
            LogTail = _logLines.ToArray(),
        };

        try
        {
            _diagnosticReport = DiagnosticReportBuilder.Build(input);
            DiagnosticsReady = true;
        }
        catch (Exception ex)
        {
            _diagnosticReport = "诊断报告生成失败：" + ex.Message;
            DiagnosticsReady = true;
        }
    }

    private void CopyDiagnostics()
    {
        if (TrySetClipboard(_diagnosticReport))
        {
            ResultSummary = "诊断信息已复制到剪贴板（已脱敏）。";
        }
        else
        {
            ErrorBanner = "剪贴板被其他程序占用，请稍后再试。";
        }
    }

    private static bool TrySetClipboard(string text)
    {
        for (int attempt = 0; attempt < 5; attempt++)
        {
            try
            {
                Clipboard.SetText(text);
                return true;
            }
            catch (Exception ex) when (
                ex is System.Runtime.InteropServices.COMException
                || ex is ThreadStateException)
            {
                Thread.Sleep(60);
            }
        }

        return false;
    }

    private void OpenDataFolder()
    {
        try
        {
            Process.Start("explorer.exe", _boot.Layout.DataRoot);
        }
        catch (Exception ex) when (ex is IOException || ex is System.ComponentModel.Win32Exception)
        {
            ErrorBanner = "打不开数据目录：" + ex.Message;
        }
    }

    // ---------- 辅助 ----------

    private LaunchPipelineRequest BuildRequest(LaunchPipelineMode mode)
    {
        return new LaunchPipelineRequest
        {
            VersionId = SelectedVersion?.Id ?? string.Empty,
            Mode = mode,
            IdentitySource = SelectedIdentity?.Source ?? IdentitySource.Offline,
            OfflineUserName = string.IsNullOrWhiteSpace(OfflineName) ? "Player" : OfflineName.Trim(),
            ServerTarget = string.IsNullOrWhiteSpace(ServerTarget) ? null : ServerTarget.Trim(),
            MaxMemoryMb = 2048,
        };
    }

    private IReadOnlyList<IdentityOption> BuildIdentityOptions()
    {
        // C1–C4 未满足时，微软登录必须显示为"可见但不可用"，并说明缺什么——
        // 而不是让用户点进去撞一个笼统的失败。
        MicrosoftAuthPrerequisites prerequisites = new MicrosoftAuthPrerequisites();

        List<IdentityOption> options = new List<IdentityOption>
        {
            new IdentityOption(
                IdentitySource.Offline,
                "离线账户",
                "仅在本机有效，无法进入正版验证（online-mode）服务器。",
                true),
        };

        options.Add(new IdentityOption(
            IdentitySource.Microsoft,
            "微软正版账户",
            prerequisites.IsSatisfied ? "使用你的微软账户登录。" : prerequisites.Describe(),
            prerequisites.IsSatisfied));

        options.Add(new IdentityOption(
            IdentitySource.ThirdParty,
            "第三方验证（未启用）",
            "该来源默认关闭，当前版本不提供。",
            false));

        return options;
    }

    private static VersionSummary? FindNewestRelease(VersionManifest manifest)
    {
        for (int i = 0; i < manifest.Versions.Count; i++)
        {
            if (string.Equals(manifest.Versions[i].Id, manifest.LatestRelease, StringComparison.OrdinalIgnoreCase))
            {
                return manifest.Versions[i];
            }
        }

        return manifest.Versions.Count > 0 ? manifest.Versions[0] : null;
    }

    private void UpdateCommands()
    {
        RefreshCommand.RaiseCanExecuteChanged();
        LaunchCommand.RaiseCanExecuteChanged();
        CancelCommand.RaiseCanExecuteChanged();
        CopyDiagnosticsCommand.RaiseCanExecuteChanged();
        RecheckCommand.RaiseCanExecuteChanged();

        Raise(nameof(CanLaunch));
    }

    private void AppendLog(string line)
    {
        _logLines.Add(DateTime.Now.ToString("HH:mm:ss", CultureInfo.InvariantCulture) + "  " + line);

        if (_logLines.Count > MaxLogLines)
        {
            _logLines.RemoveRange(0, _logLines.Count - MaxLogLines);
        }

        LogText = string.Join(Environment.NewLine, _logLines);
    }
}
