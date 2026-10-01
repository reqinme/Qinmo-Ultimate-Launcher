//! # 崩溃标记与崩溃报告（**纯规则，零 IO**）
//!
//! ## 为什么"如何知道自己崩过"需要一个小文件
//!
//! 方案 §5.7 的级别表把**崩溃报告**定成"**重启后首次进入时呈现**（不在崩溃瞬间弹窗）"。
//! 而"重启后呈现"有一个前提：**重启后的那个进程必须知道上一次崩了**。
//!
//! 它没有别的办法知道 —— 进程死了就是死了，**死亡本身不留痕迹**。
//! 所以必须由**活着的那个**写下痕迹：**崩溃标记文件**。
//!
//! ## ⚠️ 标记的语义（方案没写清，但这里不能含糊）
//!
//! 方案原文是：*"落一个 `crash-marker` 文件；**启动时检测到即提示**'上次异常退出'"*。
//!
//! **但"启动时就清掉"是错的** —— 那样在**启动过程中**崩掉就检测不到，
//! 而启动正是最脆弱的一段（读配置、建目录、启 WebView2）。
//!
//! 正确的生命周期是：
//!
//! ```text
//!     启动          →  写标记（"本次会话已开始，尚未干净结束"）
//!     正常退出      →  删标记
//!     崩溃 / 断电   →  标记**留了下来**
//!     下次启动      →  看到标记 ⇒ 上次没干净退出
//! ```
//!
//! 也就是说：**标记的含义不是"崩过"，而是"上次会话没有干净结束"**。
//! 这个差别很重要 —— 它同时覆盖了**崩溃**、**断电**、**被任务管理器杀掉**三种情形，
//! 而三者在用户看来是同一件事：**"我上次没能正常关掉它"**。
//!
//! 本模块的 [`MarkerDecision`] 就是这条生命周期的**纯规则**部分。
//!
//! ## 崩溃报告的形状：**同一 Cause 合并证据**
//!
//! 方案 §5.7 从参照项目学到三条设计，其中第一条是：
//!
//! > **同一 Cause 合并证据，而不是重复结论** —— 这是"崩溃时弹十个框"的根治方式：
//! > 同一根因多次命中合成一条，证据列在其后
//!
//! 所以 [`CrashReport`] 的结构是 **`cause → 证据列表`**，而**不是一串独立的结论**。
//! 一条 `cause` 无论命中多少次，在报告里**只出现一次**。

use serde::{Deserialize, Serialize};

/// 会话阶段。**崩溃时最需要知道的就是它** ——
/// "启动到一半崩了"与"玩游戏时崩了"是完全不同的问题。
///
/// **粒度刻意很粗**：五个阶段。更细的粒度会变成"猜"，
/// 而猜错的阶段比"未知道阶段"更坏（它会把排查引向错方向）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Stage {
    /// 进程刚起，还没读配置
    Boot,
    /// 读配置 / 建目录 / 迁移数据
    Setup,
    /// 正向用户展示界面
    Ui,
    /// 下载 / 安装 / 部署
    Work,
    /// 游戏进程运行中（我们不直接管它，但要知道那是当前阶段）
    Running,
    /// 正在退出
    Shutdown,
}

impl Stage {
    pub const fn key(self) -> &'static str {
        match self {
            Stage::Boot => "boot",
            Stage::Setup => "setup",
            Stage::Ui => "ui",
            Stage::Work => "work",
            Stage::Running => "running",
            Stage::Shutdown => "shutdown",
        }
    }

    /// 从键解析。**未知的键返回 `None` 而不是猜一个** ——
    /// 猜错的阶段会把排查引向错方向，而"未知"至少是诚实的。
    pub fn from_key(s: &str) -> Option<Self> {
        match s {
            "boot" => Some(Stage::Boot),
            "setup" => Some(Stage::Setup),
            "ui" => Some(Stage::Ui),
            "work" => Some(Stage::Work),
            "running" => Some(Stage::Running),
            "shutdown" => Some(Stage::Shutdown),
            _ => None,
        }
    }

    pub const fn human(self) -> &'static str {
        match self {
            Stage::Boot => "启动",
            Stage::Setup => "初始化",
            Stage::Ui => "界面",
            Stage::Work => "下载/部署",
            Stage::Running => "游戏运行中",
            Stage::Shutdown => "退出中",
        }
    }
}

/// **崩溃标记的内容。**
///
/// ## 它刻意很小
///
/// 因为它要在**进程即将死掉**的时刻被写下 —— 那时能做的事越少越可靠。
/// 一个"信息很全但写不完"的标记，等于没有标记。
///
/// ## ⚠️ 它写进磁盘，所以**必须已经脱敏**
///
/// `detail` 是唯一自由文本字段，而它**由调用方保证已脱敏**。
/// 为什么不在本模块脱敏：内核的脱敏管道在 `crate::scrub`，
/// 而本模块只描述形状。**但这条约束必须写在字段上** ——
/// 否则将来有人会往里塞一段原始日志。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashMarker {
    /// 启动器版本（**这是诊断里最该被看到的字段之一** ——
    /// 所以它绝不能被 IP 规则误抹，见 `scrub::looks_like_version`）
    pub version: String,
    /// 写下标记时的阶段
    pub stage: Stage,
    /// Unix 秒（**UTC，不含时区** —— 时区能定位用户所在区域）
    pub started_at_unix: u64,
    /// 可选的一句补充。**调用方必须传入已脱敏的文本。**
    #[serde(default)]
    pub detail: Option<String>,
}

impl CrashMarker {
    pub fn new(version: impl Into<String>, stage: Stage, started_at_unix: u64) -> Self {
        Self {
            version: version.into(),
            stage,
            started_at_unix,
            detail: None,
        }
    }

    pub fn with_detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }

    /// 序列化成**单行 JSON**。
    ///
    /// 单行是刻意的：标记文件可能只写了一半就断电，
    /// 而**单行文件"完整或没有"是可判定的**（解析成功即完整），
    /// 多行文件则会有"看起来有两行、其实是半截"的中间态。
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// 从单行 JSON 解析。
    ///
    /// **任何解析失败都返回 `None`，绝不猜。** 一个损坏的标记应当被当作
    /// "没有标记"处理（宁可漏报一次崩溃，也不要因为一个坏文件而无限提示）。
    pub fn from_line(s: &str) -> Option<Self> {
        serde_json::from_str::<CrashMarker>(s.trim()).ok()
    }
}

/// **启动时对标记的判定**（这条规则是纯的，所以可以被穷举测试）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerDecision {
    /// 干净退出过 → 没什么要说的
    Clean,
    /// 上一次会话没干净结束 → **要呈现崩溃报告**
    PreviousRunUnclean { marker: Box<CrashMarker> },
    /// 标记文件存在但读不出来（损坏）
    ///
    /// **它必须是一个单独的分支，而不是并进 `Clean`。**
    /// 并进去的话，"标记文件坏了"这件事永远不会被发现，
    /// 而它的成因可能是磁盘问题 —— 那值得知道。
    MarkerCorrupted { note: String },
}

impl MarkerDecision {
    /// 从"读标记的结果"推出判定。
    ///
    /// `found` 是读到的内容（`None` = 文件不存在）。
    pub fn from_read(found: Option<&str>) -> Self {
        match found {
            None => MarkerDecision::Clean,
            Some(text) => match CrashMarker::from_line(text) {
                Some(m) => MarkerDecision::PreviousRunUnclean {
                    marker: Box::new(m),
                },
                None => MarkerDecision::MarkerCorrupted {
                    note: "崩溃标记文件存在但无法解析".to_string(),
                },
            },
        }
    }

    /// 要不要在界面上出现东西。
    ///
    /// **两者都要出现**：损坏的标记也是一条要告诉用户的事实
    /// （"上次的状态记录坏了"），只是措辞不同。
    pub const fn is_noticeable(&self) -> bool {
        !matches!(self, MarkerDecision::Clean)
    }

    /// 要不要**主动呈现崩溃报告**（vs 只是记一笔）。
    pub const fn warrants_report(&self) -> bool {
        matches!(self, MarkerDecision::PreviousRunUnclean { .. })
    }
}

/// 崩溃报告的**建议动作**。
///
/// 方案 §5.7 学到的第二条设计的后半句：
///
/// > `CrashSuggestedAction { None, OpenInstanceSettings }` **是枚举，可直接驱动界面按钮**
/// > —— 比"给一句建议文本"强得多
///
/// **这就是那个枚举。**
///
/// ⚠️ **每个变体显式写出序列化名**，而不是靠 `rename_all` 的规则推导：
/// 这些字符串是**给界面派发按钮用的契约**，而"靠命名规则恰好对上"
/// 在重命名重构时会**静默失效** —— 界面上会少一个按钮，而不报错。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SuggestedAction {
    /// 没有具体动作可给
    #[serde(rename = "none")]
    None,
    /// 打开日志目录
    #[serde(rename = "open.logs")]
    OpenLogs,
    /// 导出脱敏诊断包
    #[serde(rename = "export.diagnostics")]
    ExportDiagnostics,
    /// 打开设置页
    #[serde(rename = "open.settings")]
    OpenSettings,
    /// 打开实例设置
    #[serde(rename = "open.instance_settings")]
    OpenInstanceSettings,
    /// 重新登录
    #[serde(rename = "auth.relogin")]
    ReLogin,
    /// 重试上一步
    #[serde(rename = "retry")]
    Retry,
}

impl SuggestedAction {
    /// 稳定标识（界面据此派发，**不用中文名**）。
    pub const fn key(self) -> &'static str {
        match self {
            SuggestedAction::None => "none",
            SuggestedAction::OpenLogs => "open.logs",
            SuggestedAction::ExportDiagnostics => "export.diagnostics",
            SuggestedAction::OpenSettings => "open.settings",
            SuggestedAction::OpenInstanceSettings => "open.instance_settings",
            SuggestedAction::ReLogin => "auth.relogin",
            SuggestedAction::Retry => "retry",
        }
    }

    pub const fn human(self) -> &'static str {
        match self {
            SuggestedAction::None => "无",
            SuggestedAction::OpenLogs => "打开日志目录",
            SuggestedAction::ExportDiagnostics => "导出诊断包",
            SuggestedAction::OpenSettings => "打开设置",
            SuggestedAction::OpenInstanceSettings => "打开实例设置",
            SuggestedAction::ReLogin => "重新登录",
            SuggestedAction::Retry => "重试",
        }
    }
}

/// 一条崩溃归因：**原因 + 建议 + 动作（三元组）**，证据列在其后。
///
/// 方案 §5.7 的三条设计的**前两条**都落在这个结构上。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashCause {
    /// 稳定标识（同一 `key` 的多次命中会被合并）
    pub key: String,
    /// 发生了什么（人话）
    pub reason: String,
    /// 你该做什么（人话）
    pub suggestion: String,
    /// **可直接驱动界面按钮**的动作
    pub action: SuggestedAction,
    /// 证据：**支持这条归因的原始行**（已脱敏）
    pub evidence: Vec<String>,
}

impl CrashCause {
    pub fn new(
        key: impl Into<String>,
        reason: impl Into<String>,
        suggestion: impl Into<String>,
        action: SuggestedAction,
    ) -> Self {
        Self {
            key: key.into(),
            reason: reason.into(),
            suggestion: suggestion.into(),
            action,
            evidence: Vec::new(),
        }
    }
}

/// **崩溃报告：按 Cause 合并，而不是列一串结论。**
///
/// 方案 §5.7 第一条设计的落点：
///
/// > 同一根因多次命中合成一条，证据列在其后
///
/// **这是"崩溃时弹十个框"的根治方式。** 一个 NullPointer 会导致后续
/// 几十行报错，而它们**全是同一个根因的证据** ——
/// 不做合并的话，用户看到的是"几十个错误"，而实际只有**一个**。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    pub causes: Vec<CrashCause>,
    /// 采集到的原始总行数（**让用户知道"我们看了多少"**）
    ///
    /// **显式写 `rename`**：字段名是给界面与诊断包用的契约，
    /// 而"靠默认命名规则恰好对上"在重命名重构时会静默失效。
    #[serde(rename = "scanned_lines")]
    pub scanned_lines: usize,
}

impl CrashReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// **加入一次命中。同一个 `key` 会合并进已有条目，只追加证据。**
    ///
    /// 这是本类型唯一应该被用来添加归因的方法 ——
    /// 直接 `causes.push` 会绕过合并，而**那正是"弹十个框"的成因**。
    pub fn hit(&mut self, cause: CrashCause, evidence_line: impl Into<String>) {
        let line = evidence_line.into();
        if let Some(existing) = self.causes.iter_mut().find(|c| c.key == cause.key) {
            // **证据去重**：同一行重复出现不该在报告里重复两次
            // （日志被循环读取时会出现；而重复证据会让用户以为问题更严重）
            if !existing.evidence.contains(&line) {
                existing.evidence.push(line);
            }
            return;
        }
        let mut c = cause;
        if !line.is_empty() {
            c.evidence.push(line);
        }
        self.causes.push(c);
    }

    /// 报告里有多少条**独立**归因（不是命中次数）。
    pub fn cause_count(&self) -> usize {
        self.causes.len()
    }

    /// 是否什么都没归因出来。
    pub fn is_empty(&self) -> bool {
        self.causes.is_empty()
    }

    /// 收集全部动作（去重、有序）—— 界面据此渲染按钮。
    ///
    /// **去重是必要的**：三条归因都给"打开日志"时，界面上该有**一个**按钮，不是三个。
    pub fn actions(&self) -> Vec<SuggestedAction> {
        let mut out: Vec<SuggestedAction> = Vec::new();
        for c in &self.causes {
            if c.action != SuggestedAction::None && !out.contains(&c.action) {
                out.push(c.action);
            }
        }
        out.sort();
        out
    }

    /// 一行摘要（给日志与界面标题用）。
    pub fn summary(&self) -> String {
        if self.causes.is_empty() {
            return format!("扫描 {} 行，未归因出已知原因", self.scanned_lines);
        }
        format!(
            "扫描 {} 行，归因出 {} 条独立原因",
            self.scanned_lines,
            self.causes.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────── 标记的生命周期 ─────────────────

    #[test]
    fn 没有标记说明上次干净退出() {
        assert_eq!(MarkerDecision::from_read(None), MarkerDecision::Clean);
        assert!(!MarkerDecision::Clean.is_noticeable());
        assert!(!MarkerDecision::Clean.warrants_report());
    }

    #[test]
    fn 标记还在说明上次没干净结束() {
        let m = CrashMarker::new("0.1.0.0", Stage::Work, 1_700_000_000);
        let d = MarkerDecision::from_read(Some(&m.to_line()));
        match &d {
            MarkerDecision::PreviousRunUnclean { marker } => {
                assert_eq!(marker.version, "0.1.0.0");
                assert_eq!(marker.stage, Stage::Work);
            }
            other => panic!("应当是 PreviousRunUnclean，实际 {other:?}"),
        }
        assert!(d.is_noticeable());
        assert!(d.warrants_report());
    }

    #[test]
    fn 启动阶段崩溃也会被捕获() {
        // **这一条是"标记在启动时写、干净退出时删"的直接意义。**
        // 若按"启动时就清掉标记"的做法，启动中崩溃就检测不到 ——
        // 而启动正是最脆弱的一段（读配置、建目录、启 WebView2）。
        let m = CrashMarker::new("0.1.0.0", Stage::Boot, 1);
        let d = MarkerDecision::from_read(Some(&m.to_line()));
        assert!(d.warrants_report(), "启动阶段崩溃必须被发现");

        let m2 = CrashMarker::new("0.1.0.0", Stage::Setup, 1);
        assert!(MarkerDecision::from_read(Some(&m2.to_line())).warrants_report());
    }

    #[test]
    fn 损坏的标记是单独的分支而不是并进干净() {
        // 并进去的话，"标记文件坏了"永远不会被发现，
        // 而它的成因可能是磁盘问题 —— 那值得知道。
        for bad in ["", "  ", "not json", "{", "{\"version\":", "null"] {
            let d = MarkerDecision::from_read(Some(bad));
            match d {
                MarkerDecision::MarkerCorrupted { .. } => {}
                // `null` 是合法 JSON 但**不是**一个合法标记 → 也该算损坏
                MarkerDecision::Clean => panic!("{bad:?} 不该被判成干净"),
                other => panic!("{bad:?} 应当是 MarkerCorrupted，实际 {other:?}"),
            }
        }
    }

    #[test]
    fn 标记序列化成单行且能往返() {
        // 单行是刻意的：标记可能只写了一半就断电，
        // 而**单行文件"完整或没有"是可判定的**。
        let m = CrashMarker::new("1.2.3", Stage::Ui, 42).with_detail("某一步失败");
        let line = m.to_line();
        assert!(!line.contains('\n'), "必须单行：{line}");
        assert_eq!(CrashMarker::from_line(&line), Some(m));
    }

    #[test]
    fn 未知阶段的键返回_none_而不是猜() {
        // 猜错的阶段会把排查引向错方向，而"未知"至少是诚实的。
        assert_eq!(Stage::from_key("boot"), Some(Stage::Boot));
        assert_eq!(Stage::from_key("no_such_stage"), None);
        for s in [
            Stage::Boot,
            Stage::Setup,
            Stage::Ui,
            Stage::Work,
            Stage::Running,
            Stage::Shutdown,
        ] {
            assert_eq!(Stage::from_key(s.key()), Some(s), "键必须能往返：{s:?}");
            assert!(s.key().is_ascii());
        }
    }

    #[test]
    fn 版本号必须能在标记里完整保存() {
        // 版本号是"诊断里最该被看到的字段之一"（封存项目在这里丢过它）。
        // 这条测试管的是"它别在序列化里被弄坏"。
        for v in ["0.1.0.0", "1.2.3.4", "26.3.0.0", "v1.0.0"] {
            let m = CrashMarker::new(v, Stage::Boot, 0);
            let back = CrashMarker::from_line(&m.to_line()).unwrap();
            assert_eq!(back.version, v, "版本号往返被改动");
        }
    }

    // ───────────────── 报告的合并 ─────────────────

    #[test]
    fn 同一原因多次命中只出现一次() {
        // 方案 §5.7：**这是"崩溃时弹十个框"的根治方式。**
        // 一个根因会导致后续几十行报错，而它们全是同一根因的证据。
        let mut r = CrashReport::new();
        let mk = || {
            CrashCause::new(
                "java.not_found",
                "没找到 Java",
                "去设置里指定 Java 路径",
                SuggestedAction::OpenSettings,
            )
        };
        r.hit(mk(), "第 1 行：no java");
        r.hit(mk(), "第 2 行：cannot find runtime");
        r.hit(mk(), "第 3 行：exit 1");

        assert_eq!(r.cause_count(), 1, "三条命中必须合并成一条归因");
        assert_eq!(r.causes[0].evidence.len(), 3, "证据要全留下来");
        assert_eq!(r.actions(), vec![SuggestedAction::OpenSettings]);
    }

    #[test]
    fn 重复的证据行不会重复出现() {
        // 日志被循环读取时会出现同一行；而重复证据会让用户以为问题更严重。
        let mut r = CrashReport::new();
        let c = || CrashCause::new("x", "r", "s", SuggestedAction::None);
        r.hit(c(), "同一行");
        r.hit(c(), "同一行");
        r.hit(c(), "同一行");
        assert_eq!(r.causes[0].evidence.len(), 1);
    }

    #[test]
    fn 不同原因各自成条() {
        let mut r = CrashReport::new();
        r.hit(
            CrashCause::new("a", "原因A", "建议A", SuggestedAction::OpenLogs),
            "ev-a",
        );
        r.hit(
            CrashCause::new("b", "原因B", "建议B", SuggestedAction::ReLogin),
            "ev-b",
        );
        assert_eq!(r.cause_count(), 2);
        assert_eq!(r.actions().len(), 2);
    }

    #[test]
    fn 动作去重且有序() {
        // 三条归因都给"打开日志"时，界面上该有**一个**按钮，不是三个。
        let mut r = CrashReport::new();
        for k in ["a", "b", "c"] {
            r.hit(
                CrashCause::new(k, "r", "s", SuggestedAction::OpenLogs),
                format!("ev-{k}"),
            );
        }
        assert_eq!(r.actions(), vec![SuggestedAction::OpenLogs]);
    }

    #[test]
    fn 无动作的归因不会产出按钮() {
        let mut r = CrashReport::new();
        r.hit(CrashCause::new("a", "r", "s", SuggestedAction::None), "ev");
        assert!(r.actions().is_empty());
    }

    #[test]
    fn 空报告也要有可读摘要() {
        let r = CrashReport::new();
        assert!(r.is_empty());
        let s = r.summary();
        assert!(s.contains("未归因"), "{s}");

        let mut r2 = CrashReport::new();
        r2.scanned_lines = 120;
        r2.hit(CrashCause::new("a", "r", "s", SuggestedAction::Retry), "ev");
        assert!(r2.summary().contains("120"), "{}", r2.summary());
        assert!(r2.summary().contains("1"), "{}", r2.summary());
    }

    #[test]
    fn 报告能序列化给界面() {
        let mut r = CrashReport::new();
        r.scanned_lines = 7;
        r.hit(
            CrashCause::new("a", "原因", "建议", SuggestedAction::ExportDiagnostics),
            "证据行",
        );
        let v = serde_json::to_value(&r).unwrap();
        assert!(v.get("causes").is_some());
        assert!(v.get("scanned_lines").is_some());
        assert_eq!(v["causes"][0]["action"], "export.diagnostics");
        // 三元组都在
        for k in ["key", "reason", "suggestion", "action", "evidence"] {
            assert!(v["causes"][0].get(k).is_some(), "缺字段 {k}");
        }
    }

    #[test]
    fn 建议动作的键是稳定_ascii_标识() {
        // 界面据此派发按钮，所以它必须是给机器看的。
        let all = [
            SuggestedAction::None,
            SuggestedAction::OpenLogs,
            SuggestedAction::ExportDiagnostics,
            SuggestedAction::OpenSettings,
            SuggestedAction::OpenInstanceSettings,
            SuggestedAction::ReLogin,
            SuggestedAction::Retry,
        ];
        let mut keys: Vec<&str> = all.iter().map(|a| a.key()).collect();
        for k in &keys {
            assert!(k.is_ascii(), "{k}");
        }
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), n, "动作键有重复");
    }
}
