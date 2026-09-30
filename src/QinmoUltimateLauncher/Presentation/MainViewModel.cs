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
using System.Windows.Media;
using Qul.Application.Diagnostics;
using Qul.Application.Identity;
using Qul.Application.Launch;
using Qul.Application.Update;
using Qul.Domain.Configuration;
using Qul.Domain.Diagnostics;
using Qul.Domain.Identity;
using Qul.Domain.Metadata;
using Qul.Infrastructure.Boot;
using Qul.Infrastructure.Diagnostics;
using Qul.Infrastructure.Launch;
using Qul.Infrastructure.Auth;
using Qul.Infrastructure.Net;
using Qul.Infrastructure.Platform;
using Qul.Infrastructure.Security;
using Qul.Infrastructure.Update;

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

public enum StageState
{
    Pending = 0,
    Active = 1,
    Done = 2,
    Failed = 3,
}

/// <summary>阶段条上的一格。</summary>
public sealed class StageVm : ObservableObject
{
    private StageState _state;

    public StageVm(LaunchStage stage)
    {
        Stage = stage;
    }

    public LaunchStage Stage { get; }

    public string Title => LaunchStages.Label(Stage);

    public StageState State
    {
        get => _state;
        set
        {
            if (Set(ref _state, value))
            {
                Raise(nameof(IsActive));
                Raise(nameof(IsDone));
            }
        }
    }

    public bool IsActive => _state == StageState.Active;

    public bool IsDone => _state == StageState.Done;
}

public sealed class LogLine
{
    public LogLine(string time, string text, LaunchStage stage)
    {
        Time = time;
        Text = text;
        Stage = stage;
    }

    public string Time { get; }

    public string Text { get; }

    public LaunchStage Stage { get; }

    public string Display => Time + "  " + Text;
}

/// <summary>
/// 主界面的状态机。它不自己实现启动链路，而是驱动 <see cref="LaunchPipeline"/>——
/// 与命令行走同一条路。
/// </summary>
public sealed class MainViewModel : ObservableObject
{
    /// <summary>日志上限。虚拟化列表撑得住更多，但没必要留无限历史。</summary>
    private const int MaxLogLines = 5000;

    private readonly BootContext _boot;
    private readonly LaunchPipeline _pipeline;
    private readonly ObservableCollection<LogLine> _log = new ObservableCollection<LogLine>();

    private CancellationTokenSource? _cancellation;
    private string _lastLoggedMessage = string.Empty;

    private bool _isBusy;
    private string _statusText = "先选择版本与身份来源。";
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
    private double _progressValue;
    private bool _isProgressIndeterminate;
    private string _currentStageText = "待命";
    private string _progressDetail = string.Empty;

    /// <summary>窗口取 30 秒：短了数字抖得厉害，长了看不出好转。</summary>
    private readonly ThroughputEstimator _throughput = new ThroughputEstimator(TimeSpan.FromSeconds(30));

    /// <summary>多久没有新数据就认为"卡住了"。要明显长于单次请求超时，否则会把正常重试误报成停滞。</summary>
    private static readonly TimeSpan StalledThreshold = TimeSpan.FromSeconds(20);

    private readonly AccountManager? _account;
    private AuthSession? _session;
    private IReadOnlyList<IdentityOption> _identityOptions = Array.Empty<IdentityOption>();

    private UpdateRelease? _pendingRelease;
    private bool _updateAvailable;
    private string _updateMessage = string.Empty;

    private LaunchPipelineRequest? _lastRequest;
    private LaunchPipelineResult? _lastResult;

    private readonly string? _initialVersionId;

    public MainViewModel(BootContext boot, string? initialVersionId = null)
    {
        _initialVersionId = initialVersionId;
        _boot = boot ?? throw new ArgumentNullException(nameof(boot));
        _pipeline = new LaunchPipeline(boot);

        Versions = new ObservableCollection<VersionSummary>();
        Preflight = new ObservableCollection<PreflightItem>();
        LogLines = new ReadOnlyObservableCollection<LogLine>(_log);

        Stages = new ObservableCollection<StageVm>();
        for (int i = 0; i < LaunchStages.Visible.Count; i++)
        {
            Stages.Add(new StageVm(LaunchStages.Visible[i]));
        }

        RefreshCommand = new RelayCommand(() => _ = RefreshAsync(), () => !IsBusy);
        LaunchCommand = new RelayCommand(() => _ = LaunchAsync(), () => CanLaunch);
        CancelCommand = new RelayCommand(Cancel, () => IsBusy);
        CopyDiagnosticsCommand = new RelayCommand(CopyDiagnostics, () => DiagnosticsReady);
        OpenDataFolderCommand = new RelayCommand(OpenDataFolder);
        RecheckCommand = new RelayCommand(() => _ = RebuildPreflightAsync(), () => !IsBusy);
        ThemeCommand = new RelayCommand(CycleTheme);
        CheckUpdateCommand = new RelayCommand(() => _ = CheckUpdateAsync(), () => !IsBusy && UpdateEndpoints.IsConfigured);
        ApplyUpdateCommand = new RelayCommand(() => _ = ApplyUpdateAsync(), () => !IsBusy && _pendingRelease != null);

        _account = CreateAccountManager(boot);

        RefreshIdentityOptions();
    }

    // ---------- 集合 ----------

    public ObservableCollection<VersionSummary> Versions { get; }

    public ObservableCollection<PreflightItem> Preflight { get; }

    public ObservableCollection<StageVm> Stages { get; }

    /// <summary>虚拟化日志数据源。用集合而不是一个大字符串——大字符串没法虚拟化。</summary>
    public ReadOnlyObservableCollection<LogLine> LogLines { get; }

    public IReadOnlyList<IdentityOption> IdentityOptions
    {
        get => _identityOptions;
        private set => Set(ref _identityOptions, value);
    }

    // ---------- 命令 ----------

    public RelayCommand RefreshCommand { get; }

    public RelayCommand LaunchCommand { get; }

    public RelayCommand CancelCommand { get; }

    public RelayCommand CopyDiagnosticsCommand { get; }

    public RelayCommand OpenDataFolderCommand { get; }

    public RelayCommand RecheckCommand { get; }

    public RelayCommand ThemeCommand { get; }

    public RelayCommand CheckUpdateCommand { get; }

    public RelayCommand ApplyUpdateCommand { get; }

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

    public double ProgressValue
    {
        get => _progressValue;
        private set => Set(ref _progressValue, value);
    }

    public bool IsProgressIndeterminate
    {
        get => _isProgressIndeterminate;
        private set => Set(ref _isProgressIndeterminate, value);
    }

    public string CurrentStageText
    {
        get => _currentStageText;
        private set => Set(ref _currentStageText, value);
    }

    /// <summary>
    /// 进度条下面那一行："12 KB/s · 剩余约 8 分 20 秒"。
    ///
    /// 冷装一个 1.7.10 在这台机器上实测要 15 分钟以上，而界面上原本只有
    /// 一行"开始获取所需文件…"——**用户分不清"慢但在推进"和"卡住了"**。
    /// 这不是带宽问题，是产品问题。
    /// </summary>
    public string ProgressDetail
    {
        get => _progressDetail;
        private set => Set(ref _progressDetail, value);
    }

    public string ThemeLabel => ThemeManager.Describe(ThemeManager.Mode);

    /// <summary>当前构建有没有配置发布源。没配置时「检查更新」是禁用的。</summary>
    public bool UpdateSourceConfigured => UpdateEndpoints.IsConfigured;

    public string UpdateSourceHint => UpdateEndpoints.NotConfiguredHint;

    public bool UpdateAvailable
    {
        get => _updateAvailable;
        private set
        {
            if (Set(ref _updateAvailable, value))
            {
                UpdateCommands();
            }
        }
    }

    public string UpdateMessage
    {
        get => _updateMessage;
        private set => Set(ref _updateMessage, value);
    }

    /// <summary>主题按钮的图标。直接从资源取几何，避免把资源键绑进 XAML。</summary>
    public Geometry? ThemeIcon =>
        System.Windows.Application.Current?.TryFindResource(ThemeManager.IconKey(ThemeManager.Mode)) as Geometry;

    /// <summary>
    /// 能否启动：不忙、预检无阻断项、且该确认的已确认。**三者缺一不可**。
    /// </summary>
    public bool CanLaunch =>
        !IsBusy && !HasBlocking && (!NeedsAcknowledgement || AcknowledgementGiven);

    // ---------- 流程 ----------

    private async Task RefreshAsync()
    {
        await RestoreAccountAsync();

        IsBusy = true;
        ErrorBanner = string.Empty;
        StatusText = "正在读取版本清单…";

        try
        {
            LaunchProgressReporter reporter = new LaunchProgressReporter(OnProgress);

            VersionManifest manifest = await Task.Run(
                () => _pipeline.FetchManifest(reporter, CancellationToken.None));

            Versions.Clear();
            for (int i = 0; i < manifest.Versions.Count; i++)
            {
                Versions.Add(manifest.Versions[i]);
            }

            StatusText = "共 " + Versions.Count + " 个版本；最新正式版 " + manifest.LatestRelease;

            if (SelectedVersion == null && Versions.Count > 0)
            {
                SelectedVersion = FindInitial(manifest);
            }
        }
        catch (LauncherException ex)
        {
            ErrorBanner = ErrorCodes.Id(ex.Code) + " " + ErrorCodes.Hint(ex.Code);
            StatusText = "版本清单读取失败。";
            AppendLog(LaunchStage.Failed, "版本清单读取失败：" + ex.Code);
        }
        finally
        {
            IsBusy = false;
        }

        await RebuildPreflightAsync();

        // 启动期的临时对象（版本清单与资源索引的 DOM）到这里已经没用了，
        // 而 .NET 不会主动把内存还给系统——实测 227 MB 的工作集里
        // 只有约 24 MB 是之后还会被触碰的页。
        //
        // **只在启动完成后做这一次**，不放在热路径上，也不在游戏运行期间反复做。
        await Task.Run(() => WorkingSetTrimmer.Trim());
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
        _lastResult = new LaunchPipelineResult { Preflight = items };

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
        _lastLoggedMessage = string.Empty;
        IsBusy = true;
        ErrorBanner = string.Empty;
        ResultSummary = string.Empty;
        DiagnosticsReady = false;
        StatusText = "正在准备启动…";
        ProgressValue = 0;
        IsProgressIndeterminate = true;
        UpdateCommands();

        LaunchPipelineResult result;

        try
        {
            // 进度回调在后台线程上同步触发；转发器负责切回界面线程并保持顺序。
            LaunchProgressReporter reporter = new LaunchProgressReporter(OnProgress);

            result = await Task.Run(() => _pipeline.Run(request, reporter, _cancellation!.Token));
        }
        catch (Exception ex)
        {
            result = new LaunchPipelineResult { Error = ErrorCode.DlFailed, ErrorDetail = ex.Message };
        }
        finally
        {
            IsBusy = false;
            IsProgressIndeterminate = false;
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
            OnProgress(new LaunchProgress(LaunchStage.Cancelled, "已取消。"));
            CurrentStageText = LaunchStages.Label(LaunchStage.Cancelled);
            StatusText = "已取消。";
            ResultSummary = "已取消，没有产生任何改动。";
        }
        else if (!result.Succeeded)
        {
            ErrorCode code = result.Error ?? ErrorCode.DlFailed;
            MarkStagesFailed();
            CurrentStageText = LaunchStages.Label(LaunchStage.Failed);
            ErrorBanner = ErrorCodes.Id(code) + " " + ErrorCodes.Hint(code);
            StatusText = "启动失败。";

            ResultSummary = string.IsNullOrWhiteSpace(result.ErrorDetail)
                ? "失败原因见上方的错误码。点「复制诊断信息」可以把定位所需的信息一次取走。"
                : result.ErrorDetail!;
        }
        else
        {
            MarkStagesDone();
            CurrentStageText = LaunchStages.Label(LaunchStage.Completed);
            ProgressValue = 100;
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
            // 立刻给反馈：在途请求要等底层醒过来才真正断掉，
            // 这段时间界面不能看起来像没反应。
            StatusText = "正在取消…";

            _cancellation?.Cancel();
            StatusText = "正在取消…";
        }
        catch (ObjectDisposedException)
        {
        }
    }

    // ---------- 进度与阶段 ----------

    private void OnProgress(LaunchProgress progress)
    {
        UpdateStages(progress.Stage);
        CurrentStageText = LaunchStages.Label(progress.Stage);
        UpdateThroughput(progress);

        if (progress.IsDeterminate)
        {
            // 带总量的进度只喂进度条，不进日志——否则日志会被几百行"进度 x/y"淹掉。
            ProgressValue = progress.Fraction * 100.0;
            IsProgressIndeterminate = false;
            return;
        }

        IsProgressIndeterminate = true;

        if (progress.Message.Length > 0 && progress.Message != _lastLoggedMessage)
        {
            _lastLoggedMessage = progress.Message;
            AppendLog(progress.Stage, progress.Message);
        }
    }

    /// <summary>
    /// 把累计字节变成"速度 + 剩余时间"，并区分"慢但在推进"和"卡住了"。
    ///
    /// 停滞时**刻意不显示剩余时间**：那一刻的速率已经不可信，
    /// 继续报一个数只会让人以为马上就好。
    /// </summary>
    private void UpdateThroughput(LaunchProgress progress)
    {
        if (progress.Stage != LaunchStage.Downloading || progress.BytesCompleted <= 0)
        {
            _throughput.Reset();
            ProgressDetail = string.Empty;
            return;
        }

        _throughput.Sample(progress.BytesCompleted, progress.BytesTotal, DateTimeOffset.Now);

        if (_throughput.SinceProgress >= StalledThreshold)
        {
            string stalled = "已 " + ThroughputEstimator.FormatDuration(_throughput.SinceProgress) + "没有新数据，仍在重试";

            ProgressDetail = _throughput.HasRate
                ? ThroughputEstimator.FormatRate(_throughput.BytesPerSecond) + " · " + stalled
                : stalled;

            return;
        }

        ProgressDetail = _throughput.Describe();
    }

    private void UpdateStages(LaunchStage current)
    {
        int order = LaunchStages.OrderOf(current);

        for (int i = 0; i < Stages.Count; i++)
        {
            StageVm chip = Stages[i];
            int chipOrder = LaunchStages.OrderOf(chip.Stage);

            if (current == LaunchStage.Failed || current == LaunchStage.Cancelled)
            {
                if (chip.State == StageState.Active)
                {
                    chip.State = StageState.Failed;
                }

                continue;
            }

            if (order == 0)
            {
                chip.State = StageState.Pending;
            }
            else if (chipOrder < order)
            {
                chip.State = StageState.Done;
            }
            else if (chipOrder == order)
            {
                chip.State = StageState.Active;
            }
            else
            {
                chip.State = StageState.Pending;
            }
        }
    }

    private void MarkStagesDone()
    {
        for (int i = 0; i < Stages.Count; i++)
        {
            Stages[i].State = StageState.Done;
        }
    }

    private void MarkStagesFailed()
    {
        for (int i = 0; i < Stages.Count; i++)
        {
            if (Stages[i].State == StageState.Active)
            {
                Stages[i].State = StageState.Failed;
            }
        }
    }



    private void CycleTheme()
    {
        ThemeManager.Cycle();
        Raise(nameof(ThemeLabel));
        Raise(nameof(ThemeIcon));
    }

    // ---------- 更新 ----------

    private static string CurrentVersion()
    {
        return Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "0.0.0";
    }

    private UpdateService CreateUpdateService()
    {
        return new UpdateService(
            new HttpTransport(),
            new UpdateStore(_boot.Layout.UpdatesDirectory, _boot.Log),
            _boot.Log);
    }

    private async Task CheckUpdateAsync()
    {
        if (!UpdateEndpoints.IsConfigured)
        {
            UpdateMessage = UpdateEndpoints.NotConfiguredHint;
            return;
        }

        IsBusy = true;
        UpdateMessage = "正在检查更新…";
        StatusText = "正在检查更新…";
        AppendLog(LaunchStage.Idle, "检查更新：" + UpdateEndpoints.ReleaseManifestUrl);

        try
        {
            UpdateService service = CreateUpdateService();
            string current = CurrentVersion();

            UpdateRelease? release = await Task.Run(
                () => service.Check(UpdateEndpoints.ReleaseManifestUrl, current, CancellationToken.None));

            if (release == null)
            {
                _pendingRelease = null;
                UpdateAvailable = false;
                UpdateMessage = "已是最新版本（" + current + "）。";
                StatusText = UpdateMessage;
                AppendLog(LaunchStage.Idle, "已是最新版本");
            }
            else
            {
                _pendingRelease = release;
                UpdateAvailable = true;
                UpdateMessage = "发现新版本 " + release.Version + "（" + Megabytes(release.SizeBytes) + "）";
                StatusText = UpdateMessage;
                AppendLog(LaunchStage.Idle, "发现新版本 " + release.Version);
            }
        }
        catch (LauncherException ex)
        {
            UpdateMessage = "检查更新失败：" + ErrorCodes.Hint(ex.Code);
            StatusText = UpdateMessage;
            AppendLog(LaunchStage.Failed, "检查更新失败：" + ErrorCodes.Id(ex.Code));
        }
        finally
        {
            IsBusy = false;
        }
    }

    private async Task ApplyUpdateAsync()
    {
        UpdateRelease? release = _pendingRelease;
        if (release == null || IsBusy)
        {
            return;
        }

        _cancellation = new CancellationTokenSource();
        IsBusy = true;
        ErrorBanner = string.Empty;
        UpdateMessage = "正在下载新版本…";
        UpdateCommands();

        SelfReplaceOutcome outcome;

        try
        {
            UpdateService service = CreateUpdateService();
            CancellationToken token = _cancellation.Token;

            string pendingPath = await Task.Run(() => service.Download(release, token));

            UpdateMessage = "正在替换…";
            string selfPath = CurrentExecutablePath();

            // 替换这一步刻意不可取消：标记已经落盘，
            // 中途放弃只会留下一个需要下次启动去收拾的状态。
            outcome = service.Apply(selfPath, pendingPath, release, Path.GetFileName(selfPath) + ".old");
        }
        catch (LauncherException ex)
        {
            UpdateMessage = "更新失败：" + ErrorCodes.Hint(ex.Code);
            ErrorBanner = ErrorCodes.Id(ex.Code) + " " + ErrorCodes.Hint(ex.Code);
            outcome = SelfReplaceOutcome.Failed(ex.Message);
        }
        finally
        {
            IsBusy = false;
            _cancellation?.Dispose();
            _cancellation = null;
            UpdateCommands();
        }

        if (!outcome.Succeeded)
        {
            ErrorBanner = "更新未完成，已恢复原版本：" + (outcome.FailureReason ?? string.Empty);
            UpdateMessage = string.Empty;
            return;
        }

        // 替换成功。此刻磁盘上已经是新版本，但内存里跑的还是旧代码——
        // 必须重启才算真正更新完成，否则用户会以为更新了、其实没有。
        UpdateMessage = "更新已就绪，正在重启…";
        RestartApplication();
    }

    private static string CurrentExecutablePath()
    {
        try
        {
            return Process.GetCurrentProcess().MainModule?.FileName
                   ?? Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "QinmoUltimateLauncher.exe");
        }
        catch (Exception ex) when (ex is InvalidOperationException || ex is NotSupportedException)
        {
            return Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "QinmoUltimateLauncher.exe");
        }
    }

    private void RestartApplication()
    {
        try
        {
            Process.Start(CurrentExecutablePath());
        }
        catch (Exception ex) when (ex is IOException || ex is System.ComponentModel.Win32Exception)
        {
            // 起不来也不该把用户困住：文件已经换好了，手动再打开一次即可。
            ErrorBanner = "更新已完成，但自动重启失败，请手动重新打开：" + ex.Message;
            return;
        }

        System.Windows.Application.Current?.Shutdown();
    }

    private static string Megabytes(long bytes)
    {
        return (bytes / 1024.0 / 1024.0).ToString("F1", CultureInfo.InvariantCulture) + " MB";
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
            LogTail = SnapshotLog(),
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

    private string[] SnapshotLog()
    {
        string[] lines = new string[_log.Count];
        for (int i = 0; i < _log.Count; i++)
        {
            lines[i] = _log[i].Display;
        }

        return lines;
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
                ex is System.Runtime.InteropServices.COMException || ex is ThreadStateException)
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
            Session = _session,
            OfflineUserName = string.IsNullOrWhiteSpace(OfflineName) ? "Player" : OfflineName.Trim(),
            ServerTarget = string.IsNullOrWhiteSpace(ServerTarget) ? null : ServerTarget.Trim(),
            MaxMemoryMb = 2048,
        };
    }

    private IReadOnlyList<IdentityOption> BuildIdentityOptions()
    {
        // C1–C4 未满足时，微软登录必须显示为"可见但不可用"并说明缺什么，
        // 而不是让用户点进去撞一个笼统的失败。
        MicrosoftAuthPrerequisites prerequisites = _boot.Config.Identity.Microsoft.ToPrerequisites();

        string microsoftTitle = "微软正版账户";
        string microsoftDetail;

        if (_session != null)
        {
            // 已经有可用会话：这一条与门禁无关——门禁管的是"发起新的登录"。
            microsoftTitle = "微软正版账户（" + _session.UserName + "）";
            microsoftDetail = "已登录。会话以 DPAPI 密文保存在本机。";
        }
        else
        {
            microsoftDetail = prerequisites.IsSatisfied
                ? "使用你的微软账户登录。"
                : prerequisites.Describe();
        }

        List<IdentityOption> options = new List<IdentityOption>
        {
            new IdentityOption(
                IdentitySource.Offline,
                "离线账户",
                "仅在本机有效，无法进入正版验证（online-mode）服务器。",
                true),
            new IdentityOption(
                IdentitySource.Microsoft,
                microsoftTitle,
                microsoftDetail,
                prerequisites.IsSatisfied || _session != null),
            new IdentityOption(
                IdentitySource.ThirdParty,
                "第三方验证（未启用）",
                "该来源默认关闭，当前版本不提供。",
                false),
        };

        return options;
    }

    /// <summary>
    /// 建立账户编排器。
    ///
    /// 账户键从存储里现有的键里取——键含 uuid，而启动时我们还不知道 uuid，
    /// 只能反过来问存储。没有已保存账户时用一个占位键；
    /// 那种情况下登录本来就会被门禁挡住，走不到写盘那一步。
    /// </summary>
    private static AccountManager? CreateAccountManager(BootContext boot)
    {
        try
        {
            DpapiTokenStore store = new DpapiTokenStore(boot.Layout.SecretsDirectory, boot.Log);
            IReadOnlyList<string> keys = store.ListAccounts();
            string key = keys.Count > 0 ? keys[0] : "unbound";

            MicrosoftAuthPrerequisites prerequisites = boot.Config.Identity.Microsoft.ToPrerequisites();

            return new AccountManager(
                new MicrosoftAuthProvider(new HttpTransport(), prerequisites, boot.Log),
                store,
                key,
                boot.Log);
        }
        catch (LauncherException)
        {
            return null;
        }
    }

    /// <summary>重新生成身份来源列表，并尽量保住当前选择。</summary>
    private void RefreshIdentityOptions()
    {
        IdentitySource keep = SelectedIdentity?.Source ?? IdentitySource.Offline;
        IReadOnlyList<IdentityOption> options = BuildIdentityOptions();
        IdentityOptions = options;

        for (int i = 0; i < options.Count; i++)
        {
            if (options[i].Source == keep)
            {
                SelectedIdentity = options[i];
                return;
            }
        }

        SelectedIdentity = options.Count > 0 ? options[0] : null;
    }

    /// <summary>
    /// 启动时恢复本机保存的账户。**刷新可能联网，所以放后台。**
    /// 没有会话、或恢复失败都不算错误——那只是"这台机器上还没登录过"。
    /// </summary>
    private async Task RestoreAccountAsync()
    {
        if (_account == null)
        {
            return;
        }

        AuthSession? session;

        try
        {
            session = await Task.Run(() => _account.RestoreAny(CancellationToken.None));
        }
        catch (LauncherException)
        {
            return;
        }

        if (session == null)
        {
            return;
        }

        _session = session;
        RefreshIdentityOptions();

        // 上次就是用微软账户登录的，恢复之后没有理由不默认选它。
        for (int i = 0; i < IdentityOptions.Count; i++)
        {
            if (IdentityOptions[i].Source == IdentitySource.Microsoft && IdentityOptions[i].Enabled)
            {
                SelectedIdentity = IdentityOptions[i];
                break;
            }
        }

        AppendLog(LaunchStage.Idle, "已恢复本机保存的账户：" + session.UserName);
        StatusText = "已恢复账户 " + session.UserName + "。";
    }

    /// <summary>优先用命令行指定的版本（从快捷方式直接启动某个版本），否则用最新正式版。</summary>
    private VersionSummary? FindInitial(VersionManifest manifest)
    {
        if (!string.IsNullOrWhiteSpace(_initialVersionId))
        {
            for (int i = 0; i < manifest.Versions.Count; i++)
            {
                if (string.Equals(manifest.Versions[i].Id, _initialVersionId, StringComparison.OrdinalIgnoreCase))
                {
                    return manifest.Versions[i];
                }
            }
        }

        return FindNewestRelease(manifest);
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
        CheckUpdateCommand.RaiseCanExecuteChanged();
        ApplyUpdateCommand.RaiseCanExecuteChanged();

        Raise(nameof(CanLaunch));
    }

    private void AppendLog(LaunchStage stage, string line)
    {
        _log.Add(new LogLine(DateTime.Now.ToString("HH:mm:ss", CultureInfo.InvariantCulture), line, stage));

        while (_log.Count > MaxLogLines)
        {
            _log.RemoveAt(0);
        }
    }

    /// <summary>
    /// 把管线在后台线程上同步发出的进度转回界面线程。
    ///
    /// 不用 <see cref="Progress{T}"/>：它内部是异步投递的，
    /// 下载进度会在流程结束之后才到达（界面上表现为进度条不动然后突然跳完）。
    /// 这里用 BeginInvoke 保住顺序，且不阻塞下载线程。
    /// </summary>
    private sealed class LaunchProgressReporter : IProgress<LaunchProgress>
    {
        private readonly Action<LaunchProgress> _handler;
        private readonly System.Windows.Threading.Dispatcher? _dispatcher;

        public LaunchProgressReporter(Action<LaunchProgress> handler)
        {
            _handler = handler;
            _dispatcher = System.Windows.Threading.Dispatcher.CurrentDispatcher;
        }

        public void Report(LaunchProgress value)
        {
            if (_dispatcher == null || _dispatcher.CheckAccess())
            {
                _handler(value);
                return;
            }

            _dispatcher.BeginInvoke(new Action(() => _handler(value)));
        }
    }
}
