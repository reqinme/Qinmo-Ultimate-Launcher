//! # 错误码体系（M1 · 方案 §5.7 与封存项目的编号空间）
//!
//! ## 为什么这一块必须在 M1 就有
//!
//! 方案 §5.7 的原话：**"M1 必须有，否则每模块自行造弹窗。"**
//! 不补这件事，每个模块会各造各的错误呈现，最终**界面风格不统一、
//! 用户也无法形成"一看就知道多严重"的肌肉记忆**。
//!
//! ## 两条纪律（都在这份代码里被强制，而不是靠自觉）
//!
//! 1. **同一级别内所有错误共享同一个视觉容器**——级别是**错误码的属性**，
//!    不是调用点的临场判断。**否则同一个错误码在不同地方会有不同呈现。**
//!    → 本模块里 `ErrorCode::severity()` 是 `match`，**没有第二个地方能改它**；
//!    并且有单测断言"每个码恰好一个级别 + 兜底码存在"。
//! 2. **级别由"用户能否继续"决定，不由"我们觉得多严重"决定。**
//!    → 所以 `Silent` 级别的例子是"重试成功"（技术上是错误，用户无感），
//!    而 `Blocked` 的例子是"磁盘空间不足"（技术上没什么，但用户走不下去）。
//!
//! ## 编号空间**沿用封存项目**
//!
//! 格式 `QUL-<领域>-<4 位>`（例：`QUL-AUTH-0002`）。
//! 这些编号在封存项目里已经写进过文档与测试，**沿用比另造一套正确**：
//! 错误码是**要稳定的标识**，而"重新起号"会让已经写下的引用全部失效。
//!
//! **绝不复用已登记编号**——复用会让历史日志与诊断包产生歧义。
//! 未登记的错误统一落到 [`ErrorCode::Unregistered`]（`QUL-GEN-0000`）。

use serde::{Deserialize, Serialize};

/// 错误严重级别（方案 §5.7 的五档，**用词与顺序都与方案一致**）。
///
/// 顺序即严重度：`Silent < Notice < Panel < Blocked`，
/// 而 `CrashReport` 不在这个序上——它**不在崩溃瞬间呈现**，
/// 而是**重启后首次进入时**呈现（见方案 §5.7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    /// **静默**：可自动恢复、用户无感。**只写日志，界面零提示。**
    Silent,
    /// **微提示**：用户可继续，但需知情。**状态条一行，3 秒自动消失。**
    Notice,
    /// **面板**：用户**须处理才能继续**。**灵动岛展开 + 可点击操作按钮。**
    Panel,
    /// **阻断**：无法继续。**模态框：错误码 + 原因 + 建议 + 操作按钮。**
    Blocked,
    /// **崩溃报告**：应用自身崩溃。**重启后首次进入时呈现**（不在崩溃瞬间弹窗）。
    CrashReport,
}

impl Severity {
    /// 呈现容器。**同一级别只有一个容器**，这是纪律 1 的落点。
    ///
    /// 界面按这个字符串选容器；**不许按错误码各自分支**。
    pub const fn presentation(self) -> &'static str {
        match self {
            Severity::Silent => "log-only",
            Severity::Notice => "status-bar",
            Severity::Panel => "island-expanded",
            Severity::Blocked => "modal",
            Severity::CrashReport => "crash-report-on-next-start",
        }
    }

    /// 给人看的中文名（诊断与日志用）。
    pub const fn human(self) -> &'static str {
        match self {
            Severity::Silent => "静默",
            Severity::Notice => "微提示",
            Severity::Panel => "面板",
            Severity::Blocked => "阻断",
            Severity::CrashReport => "崩溃报告",
        }
    }
}

/// 错误码。**变体名与编号都沿用封存项目**（见模块文档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ErrorCode {
    // ── 配置 ──
    CfgReadFailed,
    CfgParseFailed,
    CfgVersionTooNew,
    // ── 文件系统 ──
    IoDataRootNotWritable,
    IoDiskFull,
    IoPathTooLong,
    IoPathEscapesRoot,
    // ── 网络 ──
    NetUnreachable,
    NetProxyInvalid,
    NetCertificateInvalid,
    NetTimeout,
    NetHttpStatus,
    NetResourceMissing,
    // ── 版本元数据 ──
    MetaIndexFailed,
    MetaVersionInvalid,
    MetaInheritBroken,
    MetaUnknownField,
    // ── 下载 ──
    DlFailed,
    DlChecksumMismatch,
    DlChecksumMissing,
    // ── 解压 ──
    ZipEntryEscape,
    ZipInvalid,
    // ── Java ──
    JavaNotFound,
    JavaVersionMismatch,
    JavaInvalid,
    // ── 身份与授权 ──
    AuthUserCancelled,
    AuthTokenRefreshFailed,
    AuthNoOwnership,
    AuthThirdPartyUnavailable,
    AuthPrerequisiteMissing,
    AuthOfflineNameInvalid,
    // ── 启动计划 ──
    PlanUnresolvedPlaceholder,
    PlanSkeletonMismatch,
    // ── 进程 ──
    ProcStartFailed,
    ProcNonZeroExit,
    ProcStillRunning,
    // ── 更新 ──
    UpdFailed,
    /// **未登记的错误统一落到此编号，绝不复用已登记编号。**
    Unregistered,
}

impl ErrorCode {
    /// 稳定编号。**这是对外契约的一部分**（诊断包、日志、用户报错都引用它）。
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::CfgReadFailed => "QUL-CFG-0001",
            ErrorCode::CfgParseFailed => "QUL-CFG-0002",
            ErrorCode::CfgVersionTooNew => "QUL-CFG-0003",
            ErrorCode::IoDataRootNotWritable => "QUL-IO-0001",
            ErrorCode::IoDiskFull => "QUL-IO-0002",
            ErrorCode::IoPathTooLong => "QUL-IO-0003",
            ErrorCode::IoPathEscapesRoot => "QUL-IO-0004",
            ErrorCode::NetUnreachable => "QUL-NET-0001",
            ErrorCode::NetProxyInvalid => "QUL-NET-0002",
            ErrorCode::NetCertificateInvalid => "QUL-NET-0003",
            ErrorCode::NetTimeout => "QUL-NET-0004",
            ErrorCode::NetHttpStatus => "QUL-NET-0005",
            ErrorCode::NetResourceMissing => "QUL-NET-0006",
            ErrorCode::MetaIndexFailed => "QUL-META-0001",
            ErrorCode::MetaVersionInvalid => "QUL-META-0002",
            ErrorCode::MetaInheritBroken => "QUL-META-0003",
            ErrorCode::MetaUnknownField => "QUL-META-0004",
            ErrorCode::DlFailed => "QUL-DL-0001",
            ErrorCode::DlChecksumMismatch => "QUL-DL-0002",
            ErrorCode::DlChecksumMissing => "QUL-DL-0003",
            ErrorCode::ZipEntryEscape => "QUL-ZIP-0001",
            ErrorCode::ZipInvalid => "QUL-ZIP-0002",
            ErrorCode::JavaNotFound => "QUL-JAVA-0001",
            ErrorCode::JavaVersionMismatch => "QUL-JAVA-0002",
            ErrorCode::JavaInvalid => "QUL-JAVA-0003",
            ErrorCode::AuthUserCancelled => "QUL-AUTH-0001",
            ErrorCode::AuthTokenRefreshFailed => "QUL-AUTH-0002",
            ErrorCode::AuthNoOwnership => "QUL-AUTH-0003",
            ErrorCode::AuthThirdPartyUnavailable => "QUL-AUTH-0004",
            ErrorCode::AuthPrerequisiteMissing => "QUL-AUTH-0005",
            ErrorCode::AuthOfflineNameInvalid => "QUL-AUTH-0006",
            ErrorCode::PlanUnresolvedPlaceholder => "QUL-PLAN-0001",
            ErrorCode::PlanSkeletonMismatch => "QUL-PLAN-0002",
            ErrorCode::ProcStartFailed => "QUL-PROC-0001",
            ErrorCode::ProcNonZeroExit => "QUL-PROC-0002",
            ErrorCode::ProcStillRunning => "QUL-PROC-0003",
            ErrorCode::UpdFailed => "QUL-UPD-0001",
            ErrorCode::Unregistered => "QUL-GEN-0000",
        }
    }

    /// **级别是这个码的属性**（纪律 1 的落点）。
    ///
    /// 判据是**"用户能否继续"**，不是"我们觉得多严重"（纪律 2）：
    /// - `AuthUserCancelled` 是**静默**：用户自己取消的，本来就知道，不该再弹一次。
    /// - `NetTimeout` 是**微提示**：单次超时会被重试吸收，用户只需知情。
    /// - `DlChecksumMismatch` 是**面板**：要重下，但用户可继续用别的东西。
    /// - `IoDiskFull` 是**阻断**：走不下去了。
    /// - `CfgVersionTooNew` 是**阻断**：配置来自更新版本，我们解析不了，不能假装能。
    pub const fn severity(self) -> Severity {
        match self {
            ErrorCode::CfgReadFailed => Severity::Blocked,
            ErrorCode::CfgParseFailed => Severity::Blocked,
            ErrorCode::CfgVersionTooNew => Severity::Blocked,

            ErrorCode::IoDataRootNotWritable => Severity::Blocked,
            ErrorCode::IoDiskFull => Severity::Blocked,
            ErrorCode::IoPathTooLong => Severity::Panel,
            ErrorCode::IoPathEscapesRoot => Severity::Panel,

            ErrorCode::NetUnreachable => Severity::Panel,
            ErrorCode::NetProxyInvalid => Severity::Panel,
            ErrorCode::NetCertificateInvalid => Severity::Panel,
            // 单次超时会被重试吸收 → 用户只需知情
            ErrorCode::NetTimeout => Severity::Notice,
            ErrorCode::NetHttpStatus => Severity::Notice,
            ErrorCode::NetResourceMissing => Severity::Panel,

            ErrorCode::MetaIndexFailed => Severity::Panel,
            ErrorCode::MetaVersionInvalid => Severity::Panel,
            ErrorCode::MetaInheritBroken => Severity::Panel,
            // 未知字段是前进兼容：能继续就别打扰用户
            ErrorCode::MetaUnknownField => Severity::Silent,

            ErrorCode::DlFailed => Severity::Notice,
            ErrorCode::DlChecksumMismatch => Severity::Panel,
            ErrorCode::DlChecksumMissing => Severity::Panel,

            // 目录穿越是安全问题：不能静默，但不是"用户能修"的，故用面板告知
            ErrorCode::ZipEntryEscape => Severity::Panel,
            ErrorCode::ZipInvalid => Severity::Panel,

            ErrorCode::JavaNotFound => Severity::Panel,
            ErrorCode::JavaVersionMismatch => Severity::Panel,
            ErrorCode::JavaInvalid => Severity::Panel,

            // 用户主动取消 → 不该再弹
            ErrorCode::AuthUserCancelled => Severity::Silent,
            ErrorCode::AuthTokenRefreshFailed => Severity::Panel,
            ErrorCode::AuthNoOwnership => Severity::Panel,
            ErrorCode::AuthThirdPartyUnavailable => Severity::Panel,
            ErrorCode::AuthPrerequisiteMissing => Severity::Panel,
            ErrorCode::AuthOfflineNameInvalid => Severity::Panel,

            ErrorCode::PlanUnresolvedPlaceholder => Severity::Blocked,
            ErrorCode::PlanSkeletonMismatch => Severity::Panel,

            ErrorCode::ProcStartFailed => Severity::Panel,
            // 游戏自己非零退出：用户要知情，但我们不阻断他用别的实例
            ErrorCode::ProcNonZeroExit => Severity::Panel,
            ErrorCode::ProcStillRunning => Severity::Notice,

            ErrorCode::UpdFailed => Severity::Notice,

            // 未登记：不能假装它不重要，但也不能阻断用户
            ErrorCode::Unregistered => Severity::Panel,
        }
    }

    /// 是否可自动恢复。**这一项决定了"该不该打扰用户"**，
    /// 而它同样属于错误码（调用点无权改）。
    pub const fn recoverable(self) -> bool {
        matches!(
            self,
            ErrorCode::NetTimeout
                | ErrorCode::DlFailed
                | ErrorCode::MetaUnknownField
                | ErrorCode::ProcStillRunning
                | ErrorCode::UpdFailed
                | ErrorCode::AuthUserCancelled
        )
    }

    /// 全部已登记的错误码。**新增码必须加进这里**，否则有单测会红。
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::CfgReadFailed,
        ErrorCode::CfgParseFailed,
        ErrorCode::CfgVersionTooNew,
        ErrorCode::IoDataRootNotWritable,
        ErrorCode::IoDiskFull,
        ErrorCode::IoPathTooLong,
        ErrorCode::IoPathEscapesRoot,
        ErrorCode::NetUnreachable,
        ErrorCode::NetProxyInvalid,
        ErrorCode::NetCertificateInvalid,
        ErrorCode::NetTimeout,
        ErrorCode::NetHttpStatus,
        ErrorCode::NetResourceMissing,
        ErrorCode::MetaIndexFailed,
        ErrorCode::MetaVersionInvalid,
        ErrorCode::MetaInheritBroken,
        ErrorCode::MetaUnknownField,
        ErrorCode::DlFailed,
        ErrorCode::DlChecksumMismatch,
        ErrorCode::DlChecksumMissing,
        ErrorCode::ZipEntryEscape,
        ErrorCode::ZipInvalid,
        ErrorCode::JavaNotFound,
        ErrorCode::JavaVersionMismatch,
        ErrorCode::JavaInvalid,
        ErrorCode::AuthUserCancelled,
        ErrorCode::AuthTokenRefreshFailed,
        ErrorCode::AuthNoOwnership,
        ErrorCode::AuthThirdPartyUnavailable,
        ErrorCode::AuthPrerequisiteMissing,
        ErrorCode::AuthOfflineNameInvalid,
        ErrorCode::PlanUnresolvedPlaceholder,
        ErrorCode::PlanSkeletonMismatch,
        ErrorCode::ProcStartFailed,
        ErrorCode::ProcNonZeroExit,
        ErrorCode::ProcStillRunning,
        ErrorCode::UpdFailed,
        ErrorCode::Unregistered,
    ];
}

/// 一个**面向用户**的错误：错误码 + 原因 + 建议 + 可点击操作。
///
/// 方案 §5.7 的两段式（对齐 PCL）：**原因**说"发生了什么"，
/// **建议**说"你该做什么"。**只有原因没有建议，等于把问题丢回给用户。**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QulError {
    pub code: ErrorCode,
    /// 稳定编号（序列化出去给界面与诊断包用，免得界面自己映射）
    pub id: &'static str,
    pub severity: Severity,
    /// 呈现容器（界面按它选容器；**不许按码各自分支**）
    pub presentation: &'static str,
    /// 发生了什么（面向用户，不含内部路径与令牌）
    pub reason: String,
    /// 你该做什么。**允许为空只在一处**：`Silent` 与 `Notice` 级别
    /// （用户无需动作），且有空测断言其它级别必须有建议。
    pub suggestion: String,
    /// 可点击操作（`(标签, 操作 id)`）。界面据此渲染按钮。
    pub actions: Vec<(String, String)>,
}

impl QulError {
    /// 构造：**建议为空时会被自动补上兜底文案**。
    ///
    /// 为什么不是"断言失败"或"返回 `Err`"：
    /// - **断言失败**在 release 构建里不存在，于是线上会静默出现"只有原因没有建议"的弹窗
    ///   —— 那正是方案 §5.7 禁止的形态（把问题丢回给用户）。
    /// - **返回 `Result`** 会把"写一句建议"的成本推给每个调用点，
    ///   而多数调用点只想报个错；**摩擦会让人改用更省事的写法绕过这个类型**。
    ///
    /// 所以取第三条路：**画面不会出现空建议**，而"兜底文案被用到了"这件事
    /// 由 `tests/error_codes.rs` 的穷举断言守住——**正确性不靠自觉，靠测试**。
    pub fn new(code: ErrorCode, reason: impl Into<String>, suggestion: impl Into<String>) -> Self {
        let severity = code.severity();
        let mut suggestion = suggestion.into();
        if suggestion.trim().is_empty() && !matches!(severity, Severity::Silent | Severity::Notice)
        {
            suggestion = "请重试；若反复出现，请在诊断里导出日志以便定位。".to_string();
        }
        Self {
            code,
            id: code.as_str(),
            severity,
            presentation: severity.presentation(),
            reason: reason.into(),
            suggestion,
            actions: Vec::new(),
        }
    }

    /// 加一个可点击操作。
    pub fn with_action(mut self, label: impl Into<String>, action: impl Into<String>) -> Self {
        self.actions.push((label.into(), action.into()));
        self
    }

    /// 这个错误是否应当**打扰**用户（即界面上要不要出现东西）。
    ///
    /// `Silent` 不打扰——**这正是纪律 2 的落点**：
    /// 一个已经自动恢复的错误不该弹窗，哪怕它技术上很严重。
    pub fn is_user_visible(&self) -> bool {
        self.severity != Severity::Silent
    }
}

impl std::fmt::Display for QulError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 编号必须在文本里：用户报错时只报一句话，编号是唯一能定位的东西。
        write!(f, "[{}] {}", self.id, self.reason)?;
        if !self.suggestion.is_empty() {
            write!(f, " —— 建议：{}", self.suggestion)?;
        }
        Ok(())
    }
}

impl std::error::Error for QulError {}
