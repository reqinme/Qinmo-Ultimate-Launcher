//! # 下载引擎（M1 · 方案 §7.2 与《下载引擎设计规格》）
//!
//! ## 三条不变量在这里落地
//!
//! | 不变量 | 原文 | 落点 |
//! |---|---|---|
//! | **I1** | **未通过校验的字节，永远不以正式文件名存在于磁盘上** | 写 `<dest>.download`，**校验通过后才 rename** |
//! | **I2** | **提交（rename）与取消/暂停必须串行化** | 提交前**再检查一次取消**；提交瞬间不可中断 |
//! | **I4** | **服务端忽略 Range 时必须删掉临时文件重头下** | 请求了 Range 却拿到 200 → **删临时文件 + 从 0 重来** |
//!
//! 另加 **I3**：**校验函数按内容类型注入**，不在下载器里 `switch` 文件类型。
//! 所以 [`Verifier`] 是一个 trait，而下载器**不知道**它在验什么。
//!
//! ## ⚠️ 为什么"临时文件"这个细节值得单独成一条不变量
//!
//! 因为**没有它，一次校验失败会留下一个"看起来完整"的文件**。
//! 游戏启动器读到它 → 解析失败 → 用户看到"游戏起不来"，
//! 而真正的状态是"那份文件本该被删掉"。
//!
//! `.download` 后缀的作用不是"标记",而是**让"未验证"这个状态在文件系统上可见** ——
//! 于是任何一份"看起来是正式文件"的东西**必然已经过校验**。
//! 这个性质很强，而且它不依赖任何代码正确性，只依赖命名约定。

use qul_core::download::{
    plan_segments, segments_cover_exactly, Progress, ResumeVerdict, Segment, TaskState, Throttle,
    THROTTLE_ENGINE_MS,
};
use qul_core::http::{FetchRequest, FetchResponse, Transport};
use qul_core::retry::{CancelToken, Outcome, RetryPolicy};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// 临时文件后缀。**未通过校验的字节只能以这个名字存在。**
pub const TEMP_SUFFIX: &str = ".download";

/// 校验器（不变量 I3：**按内容类型注入**）。
///
/// 下载器**不知道**它在验什么 —— 它只知道"要么通过，要么不通过"。
/// 这样加一种新的校验方式（sha256 / md5 / 内容指纹）
/// **不需要改下载器的任何一行**。
pub trait Verifier: Send + Sync {
    /// 校验一个**已经落盘**的文件。
    fn verify(&self, path: &Path) -> Result<(), String>;
    /// 给人看的名字（错误信息里要能说清"是什么校验没过"）。
    fn name(&self) -> &'static str;
}

/// 一个**没有期望值**的校验器：总是通过。
///
/// 它有用，而且**不是为了图省事**：有些下载本来就没有哈希
/// （例如用户手动指定的 URL）。一个"必须给哈希"的接口
/// 会逼调用方去编一个假的 —— 而假的哈希会**在某一天真的拦住一份好文件**。
pub struct NoVerify;

impl Verifier for NoVerify {
    fn verify(&self, _path: &Path) -> Result<(), String> {
        Ok(())
    }
    fn name(&self) -> &'static str {
        "无校验"
    }
}

/// 下载结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadOutcome {
    /// 成功。`attempts` **包含**那些校验失败后的重试。
    Done {
        bytes: u64,
        attempts: u32,
        /// 是否走了多段并行
        segmented: bool,
    },
    /// 被取消（**不是失败**）
    Cancelled,
    /// 失败。
    ///
    /// ⚠️ **两个计数刻意分开**（这个字段形状是被测试逼出来的）：
    ///
    /// | 字段 | 什么时候加 |
    /// |---|---|
    /// | `attempts` | **网络尝试**（请求失败、HTTP 非 2xx） |
    /// | `verify_attempts` | **校验尝试**（哈希不匹配） |
    ///
    /// 第一版只有一个 `attempts`，而在校验失败分支里写了 `attempts -= 1`
    /// （想表达"校验失败不消耗网络重试"）。**那让计数非单调** ——
    /// 于是 `max_verify_retries` 事实上失效：循环永远到不了那个阈值。
    /// 这个缺陷是 `校验不通过时不留下正式文件` 那条测试抓出来的。
    ///
    /// 教训：**想用一个计数器表达两件事，就会两件都表达不清。**
    Failed {
        reason: String,
        attempts: u32,
        verify_attempts: u32,
    },
}

impl DownloadOutcome {
    pub fn into_result(self) -> Result<(u64, u32, bool), String> {
        match self {
            DownloadOutcome::Done {
                bytes,
                attempts,
                segmented,
            } => Ok((bytes, attempts, segmented)),
            DownloadOutcome::Cancelled => Err("已取消".to_string()),
            DownloadOutcome::Failed {
                reason,
                attempts,
                verify_attempts,
            } => Err(format!(
                "{reason}（网络尝试 {attempts} 次、校验尝试 {verify_attempts} 次）"
            )),
        }
    }
}

/// 下载配置。
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// 期望的分段数（**来自自适应爬坡的输出**，见规格 §5.1）
    pub want_segments: u32,
    /// 重试策略（校验失败也会用它）
    pub retry: RetryPolicy,
    /// 校验失败后最多重试几次（规格 §2：**3 次**）
    pub max_verify_retries: u32,
    /// 进度节流间隔
    pub throttle_ms: u64,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            // 起点与 `Ramp::new()` 一致（4）——**两处必须是同一个数**，
            // 否则爬坡的输出与下载器的期望会不一致。
            want_segments: qul_core::download::RAMP_INITIAL,
            retry: RetryPolicy::default_for_network(),
            max_verify_retries: 3,
            throttle_ms: THROTTLE_ENGINE_MS,
        }
    }
}

/// 进度回调。**只报 [`Progress`]，不报内部细节** ——
/// 界面要的就是那两个字节口径（I7）。
pub type ProgressFn<'a> = dyn Fn(&Progress) + Send + Sync + 'a;

/// 临时文件路径（`<dest>.download`）。
pub fn temp_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(TEMP_SUFFIX);
    PathBuf::from(s)
}

/// **下载一个文件。**
///
/// 流程（每一步都对着一条不变量）：
///
/// ```text
///     ① 建父目录
///     ② 发请求（首次不带 Range；重试时可带 Range + 校验头 —— I5）
///     ③ **若请求了 Range 却拿到 200** ⇒ 删临时文件、从头开始（I4）
///     ④ 写进 <dest>.download                       （I1）
///     ⑤ 校验（注入的 Verifier）                     （I3）
///        不通过 ⇒ 删临时文件、attempt += 1、重试（最多 3 次）
///     ⑥ **提交前再检查一次取消**（I2：提交与取消互斥）
///     ⑦ rename → 正式文件名                        （I1 的收尾）
/// ```
pub fn download(
    transport: &dyn Transport,
    url: &str,
    dest: &Path,
    verifier: &dyn Verifier,
    cfg: &DownloadConfig,
    cancel: &CancelToken,
    on_progress: Option<&ProgressFn<'_>>,
) -> DownloadOutcome {
    if let Some(dir) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return DownloadOutcome::Failed {
                reason: format!("建目录失败：{}", e.kind()),
                attempts: 0,
                verify_attempts: 0,
            };
        }
    }

    let tmp = temp_path(dest);
    // **网络尝试**（只有请求失败 / HTTP 非 2xx 才 +1）
    let mut attempts: u32 = 0;
    // **校验失败次数**（由 `cfg.max_verify_retries` 限制）
    let mut verify_attempts: u32 = 0;
    let mut last_reason = String::new();
    // 首次不带 Range；若上一轮已经有了部分内容，则尝试续传
    let mut resume_from: u64 = 0;
    let mut if_match: Option<String> = None;
    let mut if_unmodified: Option<String> = None;

    loop {
        if cancel.is_cancelled() {
            return DownloadOutcome::Cancelled;
        }
        attempts += 1;
        if attempts > cfg.retry.max_attempts {
            return DownloadOutcome::Failed {
                reason: last_reason,
                attempts,
                verify_attempts,
            };
        }

        // ② 发请求
        let mut req = FetchRequest::get(url);
        if resume_from > 0 {
            // **续传必须带校验头**（I5）：服务端内容变了就必须重头下。
            req.range = Some((resume_from, u64::MAX));
            req.if_match = if_match.clone();
            req.if_unmodified_since = if_unmodified.clone();
        }
        let resp = match transport.fetch(&req) {
            Ok(r) => r,
            Err(e) => {
                last_reason = format!("请求失败：{e}");
                std::thread::sleep(std::time::Duration::from_millis(
                    cfg.retry.delay_before(attempts - 1),
                ));
                continue;
            }
        };

        // ③ **I4：请求了 Range 却拿到 200 ⇒ 必须删掉临时文件重头下**
        //
        // "接着写"会产出一个**长度对、内容错**的文件 ——
        // 而它随后会被校验拦住，表现为"反复校验失败"，
        // 而真正的原因在这里。
        let asked_range = resume_from > 0;
        if asked_range && !resp.is_partial() {
            let _ = std::fs::remove_file(&tmp);
            resume_from = 0;
            if_match = None;
            if_unmodified = None;
            last_reason = "服务端忽略了 Range 请求，已丢弃临时文件并改为从头下载".to_string();
            // 不 sleep：这是一个**协议层面的正常分支**，不是错误。
            //
            // ⚠️ 这里**不改 `attempts`**（第一版写了 `attempts -= 1`，那让计数非单调）。
            // 一个"忽略 Range"的服务器只会发生一次（第一次请求带 Range 之后
            // `resume_from` 就被清零了），所以它天然不消耗重试预算 ——
            // **靠逻辑保证，而不是靠把计数器减回来。**
            continue;
        }
        if !resp.is_success() {
            last_reason = format!("HTTP {}", resp.status);
            std::thread::sleep(std::time::Duration::from_millis(
                cfg.retry.delay_before(attempts - 1),
            ));
            continue;
        }

        // 记录续传所需的校验头（供下一轮用）
        if resp.is_partial() {
            let sig = resp.resume_signals();
            let s = sig.to_signals();
            if_match = s.strong_etag.clone();
            if_unmodified = s.last_modified.clone();
        }

        // ④ 写进 .download
        let write_res = write_body(&tmp, resp.is_partial(), &resp.body);
        if let Err(e) = write_res {
            last_reason = format!("写临时文件失败：{e}");
            continue;
        }

        // 进度（I7：两个口径分开）
        let total = total_from(&resp, resume_from);
        let mut prog = Progress::new(total);
        prog.resume_from(resume_from + resp.body.len() as u64);
        prog.add(0);
        if let Some(f) = on_progress {
            let mut th = Throttle::new(cfg.throttle_ms);
            if th.allow(0) {
                f(&prog);
            }
        }

        // ⑤ 校验（I3：注入的校验器）
        if let Err(why) = verifier.verify(&tmp) {
            verify_attempts += 1;
            if verify_attempts >= cfg.max_verify_retries {
                let _ = std::fs::remove_file(&tmp);
                return DownloadOutcome::Failed {
                    reason: format!(
                        "{} 不匹配，已重试 {} 次：{why}",
                        verifier.name(),
                        verify_attempts
                    ),
                    attempts,
                    verify_attempts,
                };
            }
            // 规格 §2：不匹配 ⇒ **删临时文件**、等 1 秒、重新传输
            let _ = std::fs::remove_file(&tmp);
            resume_from = 0;
            if_match = None;
            if_unmodified = None;
            last_reason = format!("{} 不匹配：{why}", verifier.name());
            std::thread::sleep(std::time::Duration::from_millis(1000));
            // ⚠️ **不改 `attempts`** —— 校验失败是另一类问题，
            // 它由 `verify_retries` 自己限制（见上面那个 `>= max_verify_retries` 分支）。
            //
            // 第一版在这里写了 `attempts -= 1`，于是计数非单调、
            // `max_verify_retries` 事实上失效。**缺陷是测试抓出来的。**
            continue;
        }

        // ⑥ **I2：提交前再检查一次取消。**
        //
        // 这一步是这个函数里最短、也最重要的一段：
        // 校验可能跑很久（大文件），而用户可能就在那期间按了取消。
        // 若不在这里再查一次，我们会**在用户取消之后把文件提交上去** ——
        // 而那是一个"我明明取消了但它还是完成了"的体验。
        if cancel.is_cancelled() {
            return DownloadOutcome::Cancelled;
        }

        // ⑦ rename 到正式文件名
        match commit(&tmp, dest) {
            Ok(()) => {
                let bytes = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                return DownloadOutcome::Done {
                    bytes,
                    attempts,
                    segmented: false,
                };
            }
            Err(e) => {
                last_reason = format!("提交失败：{e}");
                continue;
            }
        }
    }
}

/// 多段并行下载（规格 §5.1：**PCL 真正的那一招**）。
///
/// 与 [`download`] 的关系：**两者共用同一套 I1/I2/I4 纪律**，
/// 区别只在"写哪一段"。所以它们刻意放在一起，
/// 让人一眼看到**多段不是另写一套下载器，而是同一套的并行版**。
///
/// 本函数**串行地**下各段（不用线程）——
/// 这样它的正确性**不依赖任何并发原语**，而"并发"这件事留给上层的任务队列。
/// 一个多段下载器若把并发与分段算术缠在一起，两边都难测。
pub fn download_segmented(
    transport: &dyn Transport,
    url: &str,
    dest: &Path,
    total: u64,
    verifier: &dyn Verifier,
    cfg: &DownloadConfig,
    cancel: &CancelToken,
) -> DownloadOutcome {
    let segs: Vec<Segment> = match plan_segments(total, cfg.want_segments, true) {
        Some(s) => s,
        None => {
            // **不支持分段就退回单段** —— 而不是报错。
            // 分段是加速手段，不是正确性条件。
            return DownloadOutcome::Failed {
                reason: "该文件不满足分段条件（应由调用方退回单段下载）".to_string(),
                attempts: 0,
                verify_attempts: 0,
            };
        }
    };
    // **分段算术必须先自证完整** —— 一个漏几百字节的计划会产出长度不足的文件，
    // 而它随后被校验拦住，表现为"反复校验失败"，真正的原因在算术里。
    if let Err(e) = segments_cover_exactly(&segs, total) {
        return DownloadOutcome::Failed {
            reason: format!("分段计划不完整：{e}"),
            attempts: 0,
            verify_attempts: 0,
        };
    }

    if let Some(dir) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            return DownloadOutcome::Failed {
                reason: format!("建目录失败：{}", e.kind()),
                attempts: 0,
                verify_attempts: 0,
            };
        }
    }
    let tmp = temp_path(dest);
    // 预分配整份大小：这样"某一段没写"会留下一个**长度对但内容是空洞**的文件，
    // 而它**会被校验拦住** —— 比"长度不足"更容易被察觉（后者看起来像下载中断）。
    if let Err(e) = std::fs::File::create(&tmp).and_then(|f| f.set_len(total)) {
        return DownloadOutcome::Failed {
            reason: format!("预分配临时文件失败：{}", e.kind()),
            attempts: 0,
            verify_attempts: 0,
        };
    }

    for seg in &segs {
        if cancel.is_cancelled() {
            return DownloadOutcome::Cancelled;
        }
        let req = FetchRequest::get(url).with_range(seg.start, seg.end_inclusive);
        let resp = match transport.fetch(&req) {
            Ok(r) => r,
            Err(e) => {
                return DownloadOutcome::Failed {
                    reason: format!("第 {} 段请求失败：{e}", seg.index),
                    attempts: 1,
                    verify_attempts: 0,
                }
            }
        };
        // I4：**分段的每一段都必须真的是 206** —— 拿到 200 说明服务端忽略了 Range，
        // 而把整份内容写进某一段的偏移会**把文件写坏**。
        if !resp.is_partial() {
            let _ = std::fs::remove_file(&tmp);
            return DownloadOutcome::Failed {
                reason: format!(
                    "第 {} 段拿到了 HTTP {}（不是 206）—— 服务端忽略了 Range，\
                     分段下载不成立，必须退回单段",
                    seg.index, resp.status
                ),
                attempts: 1,
                verify_attempts: 0,
            };
        }
        let expect = seg.len();
        if resp.body.len() as u64 != expect {
            let _ = std::fs::remove_file(&tmp);
            return DownloadOutcome::Failed {
                reason: format!(
                    "第 {} 段长度不符：期望 {expect} 字节，实际 {}",
                    seg.index,
                    resp.body.len()
                ),
                attempts: 1,
                verify_attempts: 0,
            };
        }
        if let Err(e) = write_at(&tmp, seg.start, &resp.body) {
            return DownloadOutcome::Failed {
                reason: format!("第 {} 段写入失败：{e}", seg.index),
                attempts: 1,
                verify_attempts: 0,
            };
        }
    }

    if let Err(why) = verifier.verify(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return DownloadOutcome::Failed {
            reason: format!("{} 不匹配：{why}", verifier.name()),
            attempts: 1,
            verify_attempts: 0,
        };
    }
    if cancel.is_cancelled() {
        return DownloadOutcome::Cancelled;
    }
    match commit(&tmp, dest) {
        Ok(()) => DownloadOutcome::Done {
            bytes: total,
            attempts: 1,
            segmented: true,
        },
        Err(e) => DownloadOutcome::Failed {
            reason: format!("提交失败：{e}"),
            attempts: 1,
            verify_attempts: 0,
        },
    }
}

/// 把响应体写进临时文件。`partial` 决定"追加"还是"覆盖"。
fn write_body(tmp: &Path, partial: bool, body: &[u8]) -> Result<(), String> {
    let mut f = if partial {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(tmp)
            .map_err(|e| e.kind().to_string())?
    } else {
        std::fs::File::create(tmp).map_err(|e| e.kind().to_string())?
    };
    f.write_all(body).map_err(|e| e.kind().to_string())?;
    f.flush().map_err(|e| e.kind().to_string())?;
    Ok(())
}

/// 在指定偏移写入一段。
fn write_at(tmp: &Path, offset: u64, body: &[u8]) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open(tmp)
        .map_err(|e| e.kind().to_string())?;
    f.seek(SeekFrom::Start(offset))
        .map_err(|e| e.kind().to_string())?;
    f.write_all(body).map_err(|e| e.kind().to_string())?;
    f.flush().map_err(|e| e.kind().to_string())?;
    Ok(())
}

/// 从响应推断文件总大小。
fn total_from(resp: &FetchResponse, start: u64) -> Option<u64> {
    if let Some(cr) = resp.header("content-range") {
        // `bytes 100-199/1000`
        if let Some(total) = cr
            .rsplit('/')
            .next()
            .and_then(|s| s.trim().parse::<u64>().ok())
        {
            return Some(total);
        }
    }
    resp.header("content-length")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|len| start + len)
}

/// **提交**：`<dest>.download` → `<dest>`。
///
/// ⚠️ **本函数一旦开始就不可中断**（I2）。调用方**必须在调用前**检查取消 ——
/// 而那不是"自觉"，[`download`] 里那一步是显式写出来的。
fn commit(tmp: &Path, dest: &Path) -> Result<(), String> {
    // 目标已存在时先删：Windows 上 rename 到已存在的目标会失败，
    // 而"目标已存在"是**正常情形**（重新下载一份已存在的文件）。
    if dest.exists() {
        std::fs::remove_file(dest).map_err(|e| e.kind().to_string())?;
    }
    std::fs::rename(tmp, dest).map_err(|e| e.kind().to_string())
}

/// 给测试与调用方用的一行状态说明。
pub fn state_note(state: TaskState, outcome: &DownloadOutcome) -> String {
    format!(
        "{} → {:?}（状态机键：{}）",
        outcome_note(outcome),
        state,
        state.key()
    )
}

fn outcome_note(o: &DownloadOutcome) -> &'static str {
    match o {
        DownloadOutcome::Done { .. } => "完成",
        DownloadOutcome::Cancelled => "取消",
        DownloadOutcome::Failed { .. } => "失败",
    }
}

/// 把 `Outcome` 与下载结果对上（保持"取消不是失败"这条语义一致）。
pub fn outcome_of(r: DownloadOutcome) -> Outcome<u64> {
    match r {
        DownloadOutcome::Done { bytes, .. } => Outcome::Done(bytes),
        DownloadOutcome::Cancelled => Outcome::Cancelled,
        DownloadOutcome::Failed { reason, .. } => Outcome::Failed(reason),
    }
}

/// 一条续传可行性说明（给日志用）。
pub fn resume_note(resp: &FetchResponse) -> String {
    let sig = resp.resume_signals().to_signals();
    match sig.verdict() {
        ResumeVerdict::Ok => "该响应支持续传".to_string(),
        ResumeVerdict::Restart { missing } => format!("该响应不支持续传：{}", missing.join("；")),
    }
}
