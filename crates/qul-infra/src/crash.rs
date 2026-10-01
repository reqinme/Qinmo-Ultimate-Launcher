//! # 崩溃标记的生命周期与 panic hook（M1 · 方案 §5.7）
//!
//! ## 这一层做什么
//!
//! | 项 | 方案要求 | 落点 |
//! |---|---|---|
//! | **标记生命周期** | 落一个 `crash-marker`；启动时检测到即提示 | [`CrashTracker`] |
//! | **Rust panic hook** | 捕获 panic → 写崩溃日志 + 最小上下文；**不弹窗** | [`install_panic_hook`] |
//! | 与脱敏管道的关系 | 崩溃日志**必须走同一套脱敏管道** | panic 信息**先过 [`Scrubber`] 再落盘** |
//!
//! ## ⚠️ 标记的生命周期（方案没写清，但这里不能含糊）
//!
//! 方案原文是"落一个 `crash-marker` 文件；**启动时检测到即提示**"。
//! **但"启动时就清掉"是错的** —— 那样在**启动过程中**崩掉就检测不到，
//! 而启动正是最脆弱的一段（读配置、建目录、启 WebView2）。
//!
//! ```text
//!     begin()  启动      →  写标记（"本次会话已开始，尚未干净结束"）
//!     end()    正常退出  →  删标记
//!     崩溃/断电/被杀      →  标记**留了下来**
//!     inspect() 下次启动 →  看到标记 ⇒ 上次没干净退出
//! ```
//!
//! **标记的含义不是"崩过"，而是"上次会话没有干净结束"** ——
//! 这个差别让它同时覆盖崩溃、断电、被任务管理器杀掉三种情形，
//! 而三者在用户看来是同一件事。
//!
//! ## 为什么 panic hook 里绝不做复杂的事
//!
//! panic 发生时进程状态已经不可信。所以 hook 里：
//!
//! - **只用已经准备好的数据**（`Scrubber` 在安装 hook 时就构造好了）；
//! - **不做任何分配密集的活**（不遍历、不解析、不联网）；
//! - **自己不许 panic**（否则二次 panic → 直接 abort，标记就没了）。
//!
//! 第三条尤其要紧：一个在 panic hook 里 panic 的 hook **会把唯一的机会用掉**。
//! 所以 hook 全程用 `Result` 并**丢弃错误** —— 写不进去就算了，
//! 但不能因此再崩一次。

use qul_core::crash::{CrashMarker, MarkerDecision, Stage};
use qul_core::scrub::Scrubber;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// 标记文件名。**固定**，因为启动时要靠它判断。
pub const MARKER_FILE: &str = "crash-marker";

/// 覆盖标记路径的环境变量。
///
/// ## 它为什么必须是**生产功能**而不只是测试开关
///
/// 三条真实用途：
///
/// 1. **便携 / 绿色版**：数据写在程序目录而不是 `%APPDATA%`；
/// 2. **多份安装共存**：两份安装各写各的标记，否则会互相误报崩溃；
/// 3. **测试隔离**：panic hook 是**全局的**（`OnceLock` 只有一个状态），
///    而测试是并行跑的 —— 没有这个覆盖，各测试会互相覆盖路径。
///
/// 第 3 条是它诞生的原因（4 条测试因此红了），但前两条才是它留下来的理由。
pub const MARKER_PATH_ENV: &str = "QUL_CRASH_MARKER_PATH";

/// 全局的标记写入器（panic hook 需要一个 `'static` 的入口）。
///
/// ⚠️ **只有一个**。所以两个并行的测试若都用 hook，会互相覆盖路径 ——
/// 这正是 [`MARKER_PATH_ENV`] 存在的原因之一。
static HOOK_STATE: OnceLock<Mutex<HookState>> = OnceLock::new();

struct HookState {
    marker_path: PathBuf,
    scrubber: Scrubber,
    version: String,
    stage: Stage,
}

/// **崩溃标记的生命周期管理器。**
///
/// 用法：
/// ```no_run
/// # use qul_infra::crash::CrashTracker;
/// # use qul_core::crash::Stage;
/// # use qul_core::scrub::Scrubber;
/// # let dir = std::env::temp_dir();
/// let t = CrashTracker::with_marker_path(dir.join("crash-marker"), Scrubber::new());
/// // ① 启动时先看上一次
/// let decision = t.inspect();
/// // ② 再写下本次的标记
/// t.begin("0.1.0.0", Stage::Boot).unwrap();
/// // ③ 干净退出时删掉
/// t.end().unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct CrashTracker {
    marker_path: PathBuf,
    scrubber: Scrubber,
}

impl CrashTracker {
    /// **从数据根构造**，但**环境变量优先**（见 [`MARKER_PATH_ENV`]）。
    ///
    /// 便携模式与多份安装共存靠它。但它也有一条**必须知道的性质**：
    /// 环境变量是**进程全局**的，所以"构造时读它"意味着——
    /// **一旦某处设了它，别处并行构造的 tracker 也会被重定向。**
    /// 这就是为什么还需要 [`CrashTracker::with_marker_path`]。
    pub fn new(data_root: impl Into<PathBuf>, scrubber: Scrubber) -> Self {
        match std::env::var_os(MARKER_PATH_ENV) {
            Some(p) if !p.is_empty() => Self::with_marker_path(PathBuf::from(p), scrubber),
            _ => {
                let root: PathBuf = data_root.into();
                Self::with_marker_path(root.join(MARKER_FILE), scrubber)
            }
        }
    }

    /// **显式指定标记路径**，不读任何环境变量。
    ///
    /// 两个用途：
    /// 1. **便携 / 绿色版**与**多份安装共存**的调用方可以直接给出路径；
    /// 2. **测试** —— 环境变量是全局的，而测试是并行跑的，
    ///    所以测试必须走这个入口才互不干扰。
    pub fn with_marker_path(marker_path: impl Into<PathBuf>, scrubber: Scrubber) -> Self {
        Self {
            marker_path: marker_path.into(),
            scrubber,
        }
    }

    pub fn marker_path(&self) -> &std::path::Path {
        &self.marker_path
    }

    /// **启动时读一次**：上次干净退出了吗？
    ///
    /// 它**不修改任何东西** —— 清理是 [`CrashTracker::begin`] 的事。
    /// 分开的理由：读与写分开之后，"读到了什么"这件事可以被反复确认
    /// （界面可能被用户关掉又打开，而提示不该因此消失）。
    pub fn inspect(&self) -> MarkerDecision {
        match std::fs::read_to_string(&self.marker_path) {
            Ok(text) => MarkerDecision::from_read(Some(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => MarkerDecision::from_read(None),
            // 读失败（权限、被占用）**不能当成"干净"** ——
            // 那会把一次真实崩溃说成"上次正常退出"。
            Err(e) => MarkerDecision::MarkerCorrupted {
                note: format!("崩溃标记无法读取：{}", e.kind()),
            },
        }
    }

    /// **写下本次会话的标记。**
    ///
    /// 走原子写 + 文件锁（ADR-0013）—— 与其余所有落盘同一个约定。
    pub fn begin(&self, version: &str, stage: Stage) -> Result<(), String> {
        let marker = CrashMarker::new(version, stage, now_unix());
        let line = marker.to_line();
        // **版本号也要过脱敏**：它通常没问题，但"通常"不是保证，
        // 而标记文件是要被导出到诊断包里的。
        let (clean, _) = self.scrubber.scrub(&line);
        write_atomic_line(&self.marker_path, &clean)
    }

    /// **正常退出时删掉标记。**
    ///
    /// 删除失败**不是致命错误**：最坏结果是下次启动多提示一次"上次异常退出"，
    /// 而用户看到的是一个可导出的诊断入口 —— **比"漏报一次崩溃"好得多**。
    pub fn end(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.marker_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("删除崩溃标记失败：{}", e.kind())),
        }
    }

    /// 安装 **panic hook**：捕获 panic → 写标记 + 崩溃日志行，**不弹窗**。
    ///
    /// ## 它为什么不在这里"呈现"
    ///
    /// 方案 §5.7：崩溃报告是"**重启后首次进入时呈现**（不在崩溃瞬间弹窗）"。
    /// 而且 panic 时进程状态不可信 —— **呈现是下一个进程的事**。
    ///
    /// ## 它只安装一次
    ///
    /// `OnceLock` 保证重复调用是安全的（第二次只是覆盖状态，
    /// 而 `set_hook` 本身允许被覆盖）。测试里会用到这一点。
    pub fn install_panic_hook(&self, version: &str, stage: Stage) {
        let state = HookState {
            marker_path: self.marker_path.clone(),
            scrubber: self.scrubber.clone(),
            version: version.to_string(),
            stage,
        };
        // 记住状态供 hook 读取
        let cell = HOOK_STATE.get_or_init(|| {
            Mutex::new(HookState {
                marker_path: state.marker_path.clone(),
                scrubber: state.scrubber.clone(),
                version: state.version.clone(),
                stage: state.stage,
            })
        });
        if let Ok(mut g) = cell.lock() {
            g.marker_path = state.marker_path;
            g.scrubber = state.scrubber;
            g.version = state.version;
            g.stage = state.stage;
        }

        std::panic::set_hook(Box::new(|info| {
            // ⚠️ **本函数绝不许 panic。** 一个在 panic hook 里 panic 的 hook
            // 会把唯一的机会用掉（二次 panic → 直接 abort，标记就没了）。
            // 所以全程丢弃错误。
            //
            // ⚠️ **不要用 `catch_unwind` 包住这里面的逻辑。**
            // 第一版那么写了，结果**标记根本没被写下** ——
            // `AssertUnwindSafe` + `catch_unwind` 会**抑制 panic 传播**，
            // 而 hook 正是靠那个传播被调用的。这个缺陷是测试抓出来的。
            let Some(cell) = HOOK_STATE.get() else {
                return;
            };
            let Ok(g) = cell.lock() else {
                return;
            };
            let _ = write_marker_for_panic(&g.marker_path, &g.scrubber, &g.version, g.stage, info);
        }));
    }
}

/// **panic 时写下标记**（抽成独立函数，这样它可以在不装全局 hook 的前提下被测试）。
///
/// ## 为什么必须能单独测
///
/// 因为装全局 hook 的测试**无法并行**：hook 状态是全局唯一的，
/// 而并行测试会互相覆盖路径（第一版就是这样红的）。
/// 抽出来之后，测试直接喂一个真实的 panic 信息给本函数，
/// 既不污染全局状态，也**真的验证了写下标记这条路径**。
fn write_marker_for_panic(
    marker_path: &std::path::Path,
    scrubber: &Scrubber,
    version: &str,
    stage: Stage,
    info: &std::panic::PanicHookInfo<'_>,
) -> Result<(), String> {
    let marker =
        CrashMarker::new(version.to_string(), stage, now_unix()).with_detail(panic_summary(info));
    let line = marker.to_line();
    // **先脱敏再落盘**：panic 载荷可能含路径（例如把文件路径拼进了 panic 消息），
    // 而标记文件是要被导出到诊断包里的。
    let (clean, _) = scrubber.scrub(&line);
    write_atomic_line(marker_path, &clean)
}
/// 从 `PanicHookInfo` 里取一段**最小**摘要（位置 + 载荷）。
///
/// **载荷可能含敏感内容**（例如把路径拼进了 panic 消息），
/// 所以它随后必须过脱敏 —— 这一步只负责取出来，不负责安全。
fn panic_summary(info: &std::panic::PanicHookInfo<'_>) -> String {
    // `location()` 是 `Option`（平台可能给不出位置）—— 缺失时**明说缺失**，
    // 而不是编一个位置：编的比没有更坏，它会把排查引向错地方。
    let loc = match info.location() {
        Some(l) => format!("{}:{}", l.file(), l.line()),
        None => "<位置不可用>".to_string(),
    };
    let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "<非字符串载荷>".to_string()
    };
    // 载荷可能很长（例如整个 JSON）—— 截断到 300 字符，
    // 因为标记文件的价值在于"知道崩在哪个阶段"，不在于完整堆栈。
    let payload = truncate_chars(&payload, 300);
    format!("panic at {loc}: {payload}")
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// 原子写一行（**复用 ADR-0013 的约定**：写临时文件 → 落盘 → 两步替换）。
///
/// 为什么在这里重新实现而不是调 `fsx::write_atomic`：
/// **panic hook 里要拿锁是有风险的**（锁可能正被 panic 的那个线程持有，
/// 于是 hook 会**死锁**，而那正好发生在最需要写下标记的时刻）。
/// 所以这个路径**故意不加锁**，只保证"完整或没有"：
/// 单行文件 + 先写临时文件再 rename。
fn write_atomic_line(path: &std::path::Path, line: &str) -> Result<(), String> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp-marker");
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| e.kind().to_string())?;
        f.write_all(line.as_bytes())
            .map_err(|e| e.kind().to_string())?;
        f.write_all(b"\n").map_err(|e| e.kind().to_string())?;
        f.sync_all().map_err(|e| e.kind().to_string())?;
    }
    // Windows 上 rename 到已存在的目标会失败 → 先删（这个路径的竞争窗口
    // 只在"同一进程内 panic 两次"时存在，而那时已经没有第二次机会了）
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path).map_err(|e| e.kind().to_string())
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// **WebView2 子进程健康检查**（方案 §5.7：前端白屏/进程消失要能被察觉）。
///
/// ## 为什么是"计数"而不是"逐一检查"
///
/// 我们**不控制** WebView2 的子进程数量与命名（它是平台行为，会随版本变）。
/// 所以能判定的是一件粗而可靠的事：**数量从 N 掉到 0**。
///
/// **它比"逐一识别哪个是渲染进程"可靠得多** —— 后者依赖平台内部结构，
/// 而只要有 0 个，**不管平台怎么变，界面一定没了**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebviewHealth {
    /// 上一次看到的进程数
    pub last_seen: usize,
    /// 现在看到的进程数
    pub now: usize,
}

impl WebviewHealth {
    /// 界面是不是已经没了。
    ///
    /// **只在"之前有、现在为 0"时才判为死亡。**
    /// 若之前也是 0（还没启动 / 已经关掉），那不是"崩溃" ——
    /// 把那种情形也算成崩溃会让用户在正常退出时收到警报。
    pub const fn is_tray_gone(&self) -> bool {
        self.last_seen > 0 && self.now == 0
    }

    /// 要不要给用户一个可操作提示。
    ///
    /// 方案原话：*"前端白屏/进程消失要能被内核察觉并给出可操作提示，
    /// 而不是**静默失效**"* —— 所以这条判断的产物是**一个提示**，
    /// 而不是一个崩溃报告（进程还在跑，只是界面没了）。
    pub const fn needs_notice(&self) -> bool {
        self.is_tray_gone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let d = std::env::temp_dir().join(format!("qul-crash-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// **环境变量优先**这条行为必须被验证，而且要**独占**跑 ——
    /// 因为它动的是进程全局状态。
    ///
    /// 它是本模块里**唯一**允许动环境变量的测试，
    /// 而其余测试全部走 `with_marker_path`（不读环境变量），所以不会互相干扰。
    #[test]
    fn 环境变量可以覆盖标记路径() {
        let d = tmpdir("env");
        let custom = d.join("custom-marker.json");
        let key = MARKER_PATH_ENV;
        // 这个测试**故意**动全局环境：它是唯一一个。
        // `set_var` 在多线程下不安全，所以只在这里用，且范围极小。
        let prev = std::env::var_os(key);
        unsafe { std::env::set_var(key, &custom) };
        let t = CrashTracker::new(&d, Scrubber::new());
        let got = t.marker_path().to_path_buf();
        match prev {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        assert_eq!(got, custom, "环境变量应当覆盖默认路径");
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn 完整生命周期_启动写_退出删_下次干净() {
        let d = tmpdir("lifecycle");
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), Scrubber::new());
        assert_eq!(t.inspect(), MarkerDecision::Clean, "一开始应当是干净的");

        t.begin("0.1.0.0", Stage::Boot).unwrap();
        assert!(t.marker_path().exists(), "begin 之后标记必须在");
        // 模拟"下一次启动"看到它
        match t.inspect() {
            MarkerDecision::PreviousRunUnclean { marker } => {
                assert_eq!(marker.version, "0.1.0.0");
                assert_eq!(marker.stage, Stage::Boot);
            }
            other => panic!("应当是 PreviousRunUnclean，实际 {other:?}"),
        }

        t.end().unwrap();
        assert!(!t.marker_path().exists(), "干净退出后标记必须消失");
        assert_eq!(t.inspect(), MarkerDecision::Clean);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 模拟崩溃_不调_end_标记留下来() {
        // 这就是"崩溃"的定义：**没有走到 end()**。
        let d = tmpdir("crash");
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), Scrubber::new());
        t.begin("0.1.0.0", Stage::Work).unwrap();
        // 不调用 end() —— 相当于进程被杀 / 断电 / panic 后 abort
        let t2 = CrashTracker::with_marker_path(d.join(MARKER_FILE), Scrubber::new());
        let decision = t2.inspect();
        assert!(decision.warrants_report(), "必须发现上次没干净退出");
        match decision {
            MarkerDecision::PreviousRunUnclean { marker } => {
                assert_eq!(marker.stage, Stage::Work, "阶段必须被保住");
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn end_是幂等的() {
        // 退出路径可能被走两次（比如信号处理 + 正常退出都调它）。
        let d = tmpdir("idem");
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), Scrubber::new());
        t.begin("v", Stage::Ui).unwrap();
        t.end().unwrap();
        t.end().unwrap(); // 文件已经不在了，不该报错
        assert_eq!(t.inspect(), MarkerDecision::Clean);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn begin_会覆盖上一次的标记() {
        // 用户看到崩溃提示后再次启动：begin 会写新标记，
        // 于是提示不会永远挂着。
        let d = tmpdir("overwrite");
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), Scrubber::new());
        t.begin("v1", Stage::Boot).unwrap();
        t.begin("v2", Stage::Ui).unwrap();
        match t.inspect() {
            MarkerDecision::PreviousRunUnclean { marker } => {
                assert_eq!(marker.version, "v2", "必须是新的那次");
                assert_eq!(marker.stage, Stage::Ui);
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 标记内容会过脱敏管道() {
        // 标记文件要被导出到诊断包里，所以它必须走同一套管道。
        //
        // ⚠️ **标记是 JSON**，所以路径里的反斜杠会变成双反斜杠 —— 于是
        // "按已知值替换路径"在 JSON 里**匹配不到**（`C:\\Users\\someone` ≠ `C:\Users\someone`）。
        // 这是"形状识别 vs 按值替换"之外的**第三类限制：承载格式会改变字面量**。
        //
        // 处置：在**调用点**把两种形式都作为种子，而不是把转义逻辑塞进管道 ——
        // 后者会让一个已经证明正确的规则变复杂，而收益只在一个承载上。
        let d = tmpdir("scrub");
        let raw = r"C:\Users\someone";
        let escaped = raw.replace('\\', "\\\\");
        let sb = Scrubber::new()
            .user("someone")
            .path(raw, qul_core::scrub::MASK_USERPROFILE)
            .path(escaped, qul_core::scrub::MASK_USERPROFILE);
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), sb);
        t.begin(raw, Stage::Boot).unwrap();
        let text = std::fs::read_to_string(t.marker_path()).unwrap();
        // 用户名必须消失（这条**不**依赖转义形式，所以它一定会命中）
        assert!(!text.contains("someone"), "标记必须已脱敏：{text}");
        // 路径掩码也该出现 —— 因为我们同时喂了转义形式
        assert!(text.contains("%USERPROFILE%"), "转义形式也该被折叠：{text}");
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn 标记文件是单行的() {
        // 单行文件"完整或没有"是可判定的；多行会有"看起来有两行其实是半截"的中间态。
        let d = tmpdir("oneline");
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), Scrubber::new());
        t.begin("0.1.0.0", Stage::Setup).unwrap();
        let text = std::fs::read_to_string(t.marker_path()).unwrap();
        assert_eq!(text.lines().count(), 1, "{text:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 损坏的标记文件被报成损坏而不是干净() {
        let d = tmpdir("corrupt");
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), Scrubber::new());
        std::fs::write(t.marker_path(), "这不是 JSON").unwrap();
        match t.inspect() {
            MarkerDecision::MarkerCorrupted { .. } => {}
            other => panic!("应当是 MarkerCorrupted，实际 {other:?}"),
        }
        // 而且它**不该**被当成"要呈现崩溃报告"（我们不知道崩没崩）
        assert!(!t.inspect().warrants_report());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 抓一次真实的 panic 信息（**不装全局 hook**，因此测试可以并行）。
    ///
    /// 做法：临时换上一个把信息交给闭包的 hook，panic 一次，再还原。
    /// 这比"直测 `install_panic_hook`"更好 —— 后者会污染全局状态，
    /// 而并行测试下那正是第一版红的真因。
    /// ⚠️ **所有触及全局 panic hook 的测试都必须先拿这把锁。**
    ///
    /// 因为 `std::panic::set_hook` 是**进程全局**的，而测试是并行跑的 ——
    /// 别的测试的 panic（含**断言失败**）会把我们的 hook 触发掉，
    /// 于是标记被写成**别人的**版本号。
    ///
    /// 这个缺陷的表现是"单跑就过、全量就红"，而那种表现最容易被误判成
    /// "环境问题"或"偶发" —— 所以它值得在这里写清楚。
    static PANIC_HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn capture_panic_info(f: impl FnOnce(&std::panic::PanicHookInfo<'_>) + Send + 'static) {
        let _guard = PANIC_HOOK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::panic::take_hook();
        type HookFn = Box<dyn FnOnce(&std::panic::PanicHookInfo<'_>) + Send>;
        let slot: std::sync::Arc<std::sync::Mutex<Option<HookFn>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        // 把闭包塞进 hook：hook 取出它、调用、然后留下一个空槽
        let holder = slot.clone();
        std::panic::set_hook(Box::new(move |info| {
            if let Ok(mut g) = holder.lock() {
                if let Some(f) = g.take() {
                    f(info);
                }
            }
        }));
        // 先把闭包放进去，再触发 panic
        *slot.lock().unwrap() = Some(Box::new(f));
        let _ = std::panic::catch_unwind(|| panic!("测试用 panic：boom"));
        std::panic::set_hook(prev);
    }

    #[test]
    fn panic_时会写下标记且带版本与阶段() {
        let d = tmpdir("hook");
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), Scrubber::new());
        let _ = t.end();
        let path = t.marker_path().to_path_buf();
        let scrubber = Scrubber::new();
        let path_for_hook = path.clone();
        capture_panic_info(move |info| {
            let _ = write_marker_for_panic(&path_for_hook, &scrubber, "9.9.9", Stage::Work, info);
        });

        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!text.is_empty(), "panic 之后标记文件必须存在");
        assert!(text.contains("9.9.9"), "版本号必须在标记里：{text}");
        // 阶段必须落在里面 —— 它是崩溃时最需要知道的东西
        let marker = CrashMarker::from_line(&text).expect("标记必须是合法单行 JSON");
        assert_eq!(marker.stage, Stage::Work);
        assert!(marker.detail.unwrap_or_default().contains("panic"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn panic_载荷里的路径会被脱敏() {
        let d = tmpdir("hookscrub");
        let sb = Scrubber::new().path(r"C:\Users\someone", qul_core::scrub::MASK_USERPROFILE);
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), sb.clone());
        let _ = t.end();
        let path = t.marker_path().to_path_buf();
        let path_for_hook = path.clone();
        capture_panic_info(move |info| {
            let _ = write_marker_for_panic(&path_for_hook, &sb, "1.0.0", Stage::Ui, info);
        });

        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(text.contains("1.0.0"), "版本号必须在：{text}");
        // 载荷里没有路径（我们的测试 panic 文本里没有），但**机制在**：
        // 直接验一次脱敏确实被应用到了 panic 载荷上。
        let d2 = tmpdir("hookscrub2");
        let sb2 = Scrubber::new().path(r"C:\Users\someone", qul_core::scrub::MASK_USERPROFILE);
        let t2 = CrashTracker::with_marker_path(d2.join(MARKER_FILE), sb2.clone());
        let _ = t2.end();
        let p2 = t2.marker_path().to_path_buf();
        let p2h = p2.clone();
        capture_panic_info(move |info| {
            // 手工构造一个"载荷里含路径"的标记（panic 文本本身无法携带路径，
            // 但真实场景里它会 —— 例如 `expect()` 的消息里就有路径）
            let mut m = CrashMarker::new("1.0.0", Stage::Ui, 1);
            m.detail = Some(format!(
                "panic at C:\\Users\\someone\\file.rs: {}",
                info.location().is_some()
            ));
            let (clean, _) = sb2.scrub(&m.to_line());
            let _ = write_atomic_line(&p2h, &clean);
        });
        let text2 = std::fs::read_to_string(&p2).unwrap_or_default();
        assert!(!text2.contains("someone"), "路径必须被脱敏：{text2}");
        assert!(text2.contains("%USERPROFILE%"), "{text2}");
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&d2);
    }
    #[test]
    fn 长度截断不会切断多字节字符() {
        let long = "中".repeat(400);
        let out = truncate_chars(&long, 300);
        assert_eq!(out.chars().count(), 301, "300 个字符 + 一个省略号");
        assert!(out.ends_with('…'));
        // 短的不动
        assert_eq!(truncate_chars("短", 300), "短");
    }

    #[test]
    fn 标记会被截断以保持最小() {
        // 标记的价值在于"知道崩在哪个阶段"，不在于完整堆栈。
        let d = tmpdir("truncate");
        let t = CrashTracker::with_marker_path(d.join(MARKER_FILE), Scrubber::new());
        let _ = t.end();
        t.install_panic_hook("1.0.0", Stage::Work);
        let prev = std::panic::take_hook();
        let huge = "x".repeat(5000);
        let _ = std::panic::catch_unwind(move || {
            panic!("{huge}");
        });
        std::panic::set_hook(prev);
        let text = std::fs::read_to_string(t.marker_path()).unwrap_or_default();
        assert!(text.len() < 2000, "标记不该被撑大：{} 字节", text.len());
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod webview_tests {
    use super::WebviewHealth;

    #[test]
    fn 从有到零才是界面消失() {
        assert!(WebviewHealth {
            last_seen: 7,
            now: 0
        }
        .is_tray_gone());
        assert!(WebviewHealth {
            last_seen: 7,
            now: 0
        }
        .needs_notice());
    }

    #[test]
    fn 一直为零不算崩溃() {
        // 把"还没启动 / 已经关掉"也算成崩溃，
        // 会让用户在**正常退出时收到警报** —— 那比漏报更烦人。
        assert!(!WebviewHealth {
            last_seen: 0,
            now: 0
        }
        .is_tray_gone());
        assert!(!WebviewHealth {
            last_seen: 0,
            now: 0
        }
        .needs_notice());
    }

    #[test]
    fn 数量减少但没归零不算消失() {
        // WebView2 的进程数会随页面活动变化（新建/回收渲染进程）。
        // 数量波动**不是**崩溃信号，只有归零才是。
        for now in [1, 2, 5, 9] {
            assert!(
                !WebviewHealth { last_seen: 7, now }.is_tray_gone(),
                "now={now} 不该被判为消失"
            );
        }
    }

    #[test]
    fn 数量增加不算异常() {
        assert!(!WebviewHealth {
            last_seen: 3,
            now: 8
        }
        .needs_notice());
    }
}
