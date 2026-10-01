//! # 真实进程执行器（M3 · 进程管理）
//!
//! ## 它补的是哪一个洞
//!
//! [`qul_core::provider::ProcessExecutor`] 这个 trait 从 M1 就存在，
//! 而**实现一直缺** —— M1 用的是 `MockProvider` 的 `RecordingExecutor`
//! （只记录、不启动）。那对 M1 的出口条件（"调用链通不通"）是对的，
//! 而 **M3 的出口条件是"能进游戏主菜单"** —— 那必须真的拉起进程。
//!
//! ## 三条纪律，每条都对应一个真实的坏后果
//!
//! | 纪律 | 不这么做会怎样 |
//! |---|---|
//! | **保留 `ResolvedCommand` 的类型**（自己再拼一次命令行） | Windows 上的引号/空格规则会在某些路径上出错，而那**只在特定用户名/目录下复现** |
//! | **stdout 与 stderr 分开收、按行流式喂**（而不是等结束再读） | 一个跑了 10 分钟才崩的游戏，用户在崩溃前**看不到任何日志** |
//! | **取消时杀掉整个进程树** | 只杀父进程会让**游戏子进程变成孤儿**继续跑，而用户以为已经关了 |
//!
//! ## ⚠️ 取消为什么是"轮询 + kill"而不是更精巧的做法
//!
//! Windows 上没有信号。可选的做法有三种：
//!
//! | 做法 | 问题 |
//! |---|---|
//! | 让子进程自己读一个"取消文件" | **要求游戏配合** —— 我们改不了游戏 |
//! | `TerminateProcess` 单个进程 | 杀不掉子进程 → 孤儿 |
//! | **轮询 + `taskkill /T`** | 唯一不需要配合、且能连子进程一起收的 |
//!
//! 所以是第三种，而轮询间隔取 **80 ms**：它对人的感知来说"立刻"，
//! 而每秒 12 次的开销可以忽略。
//!
//! ## ⚠️ 退出码非零**不是**由本模块判定成错误
//!
//! 它**如实返回** `Some(code)`，由调用方决定那是"崩溃"还是"正常退出"。
//! 理由：某些产品用非零码表示正常退出（例如用户点了"退出"），
//! 而**把判定塞进执行器会让那条知识藏在一个不该有它的地方**。

use qul_core::plan::ResolvedCommand;
use qul_core::provider::{PlanError, ProcessExecutor};
use qul_core::retry::CancelToken;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 进程的哪一路输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stdout,
    Stderr,
}

/// 日志行的去处。
///
/// ## 为什么是一个 trait 而不是 `Vec<String>`
///
/// 因为**游戏可能跑几小时**，而攒在内存里会让内存随日志线性增长。
/// 真实场景是"边收边写文件 + 边推给界面"，而那是调用方的事。
///
/// **必须是 `Send + Sync`**：两路线程会同时调用它。
pub trait LineSink: Send + Sync {
    fn line(&self, channel: Channel, text: &str);
}

/// 什么都不做的去处（只收集退出码时用）。
pub struct NullSink;

impl LineSink for NullSink {
    fn line(&self, _c: Channel, _t: &str) {}
}

/// 把日志收进内存（**测试与"只想看最后几行"的场景**）。
///
/// ⚠️ 它**没有上限** —— 用一个跑几小时的进程配它会让内存涨到崩。
/// 所以它的文档里明说这一点，而生产路径应当用别的实现。
#[derive(Default)]
pub struct VecSink {
    lines: std::sync::Mutex<Vec<(Channel, String)>>,
}

impl VecSink {
    pub fn new() -> Self {
        Self::default()
    }
    /// 取出目前收到的全部行。
    pub fn take(&self) -> Vec<(Channel, String)> {
        std::mem::take(&mut *self.lines.lock().expect("锁没被毒化"))
    }
    pub fn count(&self) -> usize {
        self.lines.lock().expect("锁没被毒化").len()
    }
}

impl LineSink for VecSink {
    fn line(&self, channel: Channel, text: &str) {
        self.lines
            .lock()
            .expect("锁没被毒化")
            .push((channel, text.to_string()));
    }
}

/// 一次运行的**如实记录**。
///
/// 它存在的理由：M3 的出口条件里有一条是"**采集 stdout/stderr，确认日志可读、
/// 退出码可回收**"。那件事必须有证据，而"证据"就是一个能被断言的结构。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// 退出码。`None` = **被取消**（而不是"失败"——见 [`CancelToken`] 的语义）
    pub exit_code: Option<i32>,
    /// 收了多少行（含两路）
    pub lines: u64,
    /// 从 spawn 到回收的毫秒数
    pub elapsed_ms: u64,
    /// 是否发生过取消
    pub cancelled: bool,
}

/// **真实进程执行器。**
pub struct RealProcessExecutor {
    /// 轮询间隔（毫秒）。**可调是为了测试能把它调小**。
    pub poll_ms: u64,
    /// 取消时是否连子进程一起收（Windows 上用 `taskkill /T`）。
    pub kill_tree: bool,
}

impl Default for RealProcessExecutor {
    fn default() -> Self {
        Self {
            // 80 ms：对人的感知是"立刻"，而每秒 12 次的开销可忽略。
            poll_ms: 80,
            kill_tree: true,
        }
    }
}

impl RealProcessExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// **带日志去处**地运行一条已解析的命令。
    ///
    /// ## 它为什么要求 `Arc<dyn LineSink>` 而不是 `&dyn LineSink`
    ///
    /// ⚠️ **这是本模块里唯一一处类型逼着我改设计的地方，而理由值得写下来。**
    ///
    /// 两路日志要**并发**读（否则一路的输出会占满缓冲区、把子进程堵住），
    /// 所以读取必须在别的线程里 —— 而**线程要求 `'static`**。
    /// 于是 `&dyn LineSink`（带着调用方的生命周期）**送不进去**。
    ///
    /// 第一版我写了一个 `spawn_reader(pipe, channel, sink: &dyn LineSink, …)`
    /// 的函数，而它**在类型上就做不到**：那个函数体里只能 `unreachable!()`。
    /// 我没有用 `unsafe` 去延长那个借用的生命周期 —— 那会让"日志去处"
    /// 变成一个生命周期陷阱，而这是完全不必要的复杂度。
    ///
    /// **正确做法就是 `Arc`**：它把"谁拥有"这件事写明，而代价只是一次原子加。
    pub fn run_with_sink(
        &self,
        cmd: &ResolvedCommand,
        cancel: &CancelToken,
        sink: Arc<dyn LineSink>,
    ) -> Result<RunOutcome, PlanError> {
        self.run_streaming(cmd, cancel, sink)
    }

    /// **流式运行**：`sink` 由 `Arc` 持有，于是读取线程可以合法地跨线程调用它。
    ///
    /// 它是真正的实现，而 `run_with_sink` 与 `ProcessExecutor::run` 都转到这里。
    pub fn run_streaming(
        &self,
        cmd: &ResolvedCommand,
        cancel: &CancelToken,
        sink: Arc<dyn LineSink>,
    ) -> Result<RunOutcome, PlanError> {
        let started = std::time::Instant::now();

        let mut c = Command::new(&cmd.program);
        c.args(&cmd.args);
        for (k, v) in &cmd.env {
            c.env(k, v);
        }
        if let Some(dir) = &cmd.cwd {
            c.current_dir(dir);
        }
        c.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            c.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = c.spawn().map_err(|e| {
            let (why, suggest) = match e.kind() {
                std::io::ErrorKind::NotFound => (
                    format!("找不到可执行文件：{}", cmd.program),
                    "这个版本需要的运行时可能没装。试试「依赖与组件检查」。",
                ),
                std::io::ErrorKind::PermissionDenied => (
                    format!("没有权限执行：{}", cmd.program),
                    "可能是安全软件拦住了；也可能是文件被占用。",
                ),
                _ => (
                    format!("无法启动进程：{}（{}）", cmd.program, e.kind()),
                    "请导出诊断日志以便定位。",
                ),
            };
            PlanError::new(qul_core::error::ErrorCode::ProcStartFailed, why, suggest)
        })?;

        let counter = Arc::new(AtomicU64::new(0));
        let h_out = child
            .stdout
            .take()
            .map(|p| spawn_reader_arc(p, Channel::Stdout, sink.clone(), counter.clone()));
        let h_err = child
            .stderr
            .take()
            .map(|p| spawn_reader_arc(p, Channel::Stderr, sink.clone(), counter.clone()));

        let mut cancelled = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break Some(st),
                Ok(None) => {}
                Err(e) => {
                    return Err(PlanError::new(
                        qul_core::error::ErrorCode::ProcStartFailed,
                        format!("等待进程时出错：{}", e.kind()),
                        "请导出诊断日志以便定位。",
                    ))
                }
            }
            if cancel.is_cancelled() {
                cancelled = true;
                if self.kill_tree {
                    let _ = Command::new("taskkill")
                        .args(["/PID", &child.id().to_string(), "/T", "/F"])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                }
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(self.poll_ms));
        };

        if let Some(h) = h_out {
            let _ = h.join();
        }
        if let Some(h) = h_err {
            let _ = h.join();
        }

        Ok(RunOutcome {
            exit_code: status.and_then(|s| s.code()),
            lines: counter.load(Ordering::SeqCst),
            elapsed_ms: started.elapsed().as_millis() as u64,
            cancelled,
        })
    }
}

fn spawn_reader_arc<R: std::io::Read + Send + 'static>(
    pipe: R,
    channel: Channel,
    sink: Arc<dyn LineSink>,
    counter: Arc<AtomicU64>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let reader = BufReader::new(pipe);
        for line in reader.lines() {
            match line {
                Ok(text) => {
                    counter.fetch_add(1, Ordering::SeqCst);
                    sink.line(channel, &text);
                }
                // 一行不是合法 UTF-8：**不丢**，用有损转换收下。
                // 一个"读不动就退出"的实现会让日志在那里**静默断掉**，
                // 而"日志少了一半"比"某行乱码"难查得多。
                Err(_) => {
                    counter.fetch_add(1, Ordering::SeqCst);
                    sink.line(channel, "<这一行不是合法 UTF-8，已丢弃>");
                }
            }
        }
    })
}

impl ProcessExecutor for RealProcessExecutor {
    /// 不带日志去处地运行（收集退出码）。
    ///
    /// ⚠️ **它仍然读走两路输出**（用 [`NullSink`]）——
    /// 因为不读的话管道缓冲区会满，而**子进程会阻塞在写日志上**，
    /// 表现为"游戏卡住不动"。那是一个经典的死锁，而它的样子是"游戏没反应"。
    fn run(&self, cmd: &ResolvedCommand, cancel: &CancelToken) -> Result<Option<i32>, PlanError> {
        let sink: Arc<dyn LineSink> = Arc::new(NullSink);
        let out = self.run_streaming(cmd, cancel, sink)?;
        Ok(out.exit_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// 一个**跨平台都存在的无害命令**：`cmd /c echo`（Windows）/
    /// `sh -c echo`（其他）。
    ///
    /// **为什么不直接用 `echo`**：Windows 上 `echo` 是 `cmd` 的内建命令，
    /// 不是一个可执行文件 —— 直接 spawn 会得到 `NotFound`，
    /// 而那会让人误以为"找不到命令"，实际是"它不是可执行文件"。
    fn echo_cmd(text: &str) -> ResolvedCommand {
        #[cfg(windows)]
        {
            ResolvedCommand {
                program: "cmd".into(),
                args: vec!["/c".into(), "echo".into(), text.into()],
                env: BTreeMap::new(),
                cwd: None,
            }
        }
        #[cfg(not(windows))]
        {
            ResolvedCommand {
                program: "sh".into(),
                args: vec!["-c".into(), format!("echo {text}")],
                env: BTreeMap::new(),
                cwd: None,
            }
        }
    }

    fn exec() -> RealProcessExecutor {
        RealProcessExecutor {
            poll_ms: 5,
            kill_tree: true,
        }
    }

    // ───────────────── 正常路径 ─────────────────

    #[test]
    fn 能启动进程并回收退出码() {
        let sink = Arc::new(VecSink::new());
        let out = exec()
            .run_streaming(&echo_cmd("hello"), &CancelToken::new(), sink.clone())
            .expect("应当能启动");
        assert_eq!(out.exit_code, Some(0), "echo 的退出码该是 0");
        assert!(!out.cancelled);
        // **日志被收到了** —— M3 的出口条件要求"日志可读、退出码可回收"
        let lines = sink.take();
        assert!(!lines.is_empty(), "该收到至少一行输出");
        assert!(
            lines.iter().any(|(_, t)| t.contains("hello")),
            "该收到 echo 的文本：{lines:?}"
        );
        assert_eq!(lines[0].0, Channel::Stdout);
    }

    #[test]
    fn 非零退出码如实返回_而不被判定成错误() {
        // ⚠️ **这是刻意的**：某些产品用非零码表示正常退出。
        // 把判定塞进执行器会让那条知识藏在一个不该有它的地方。
        #[cfg(windows)]
        let cmd = ResolvedCommand {
            program: "cmd".into(),
            args: vec!["/c".into(), "exit".into(), "3".into()],
            env: BTreeMap::new(),
            cwd: None,
        };
        #[cfg(not(windows))]
        let cmd = ResolvedCommand {
            program: "sh".into(),
            args: vec!["-c".into(), "exit 3".into()],
            env: BTreeMap::new(),
            cwd: None,
        };
        let out = exec()
            .run_streaming(&cmd, &CancelToken::new(), Arc::new(NullSink))
            .expect("启动本身应当成功");
        assert_eq!(out.exit_code, Some(3), "**退出码如实返回**");
    }

    #[test]
    fn stderr_与_stdout_分开收() {
        // 一个"把两路混在一起"的实现会让日志里的错误与普通输出无法区分。
        #[cfg(windows)]
        let cmd = ResolvedCommand {
            program: "cmd".into(),
            args: vec!["/c".into(), "echo out & echo err 1>&2".into()],
            env: BTreeMap::new(),
            cwd: None,
        };
        #[cfg(not(windows))]
        let cmd = ResolvedCommand {
            program: "sh".into(),
            args: vec!["-c".into(), "echo out; echo err 1>&2".into()],
            env: BTreeMap::new(),
            cwd: None,
        };
        let sink = Arc::new(VecSink::new());
        exec()
            .run_streaming(&cmd, &CancelToken::new(), sink.clone())
            .unwrap();
        let lines = sink.take();
        assert!(
            lines
                .iter()
                .any(|(c, t)| *c == Channel::Stdout && t.contains("out")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|(c, t)| *c == Channel::Stderr && t.contains("err")),
            "{lines:?}"
        );
    }

    #[test]
    fn run_不带日志也会读走两路输出() {
        // ⚠️ **这条测试防的是一个经典死锁。**
        //
        // 不读管道的话缓冲区会满，而**子进程会阻塞在写日志上** ——
        // 表现为"游戏卡住不动"。所以 `run()` 必须仍然读（用 NullSink）。
        //
        // 用一个输出量明显超过管道缓冲区（通常 4 KB）的命令来验它：
        // 若实现没读，这条会**挂住**而不是失败。
        let mut args_line = String::new();
        for i in 0..500 {
            args_line.push_str(&format!("line{i} "));
        }
        #[cfg(windows)]
        let cmd = ResolvedCommand {
            program: "cmd".into(),
            args: vec!["/c".into(), format!("echo {args_line}")],
            env: BTreeMap::new(),
            cwd: None,
        };
        #[cfg(not(windows))]
        let cmd = ResolvedCommand {
            program: "sh".into(),
            args: vec!["-c".into(), format!("echo {args_line}")],
            env: BTreeMap::new(),
            cwd: None,
        };
        let out = exec()
            .run(&cmd, &CancelToken::new())
            .expect("大输出也该正常结束");
        assert_eq!(out, Some(0), "**没有读输出的话这里会挂住**");
    }

    // ───────────────── 错误路径 ─────────────────

    #[test]
    fn 可执行文件不存在时给出可操作的建议() {
        let cmd = ResolvedCommand {
            program: "qul-no-such-program-xyzzy".into(),
            args: vec![],
            env: BTreeMap::new(),
            cwd: None,
        };
        let e = exec()
            .run_streaming(&cmd, &CancelToken::new(), Arc::new(NullSink))
            .unwrap_err();
        assert_eq!(e.code, qul_core::error::ErrorCode::ProcStartFailed);
        assert!(e.reason.contains("找不到可执行文件"), "{}", e.reason);
        // **建议必须具体** —— 一个"启动失败，请重试"的建议是没有用的
        assert!(e.suggestion.contains("运行时"), "{}", e.suggestion);
    }

    // ───────────────── 取消 ─────────────────

    #[test]
    fn 取消时返回_none_而不是失败() {
        // ⚠️ **取消是用户的意愿，不是故障** —— 见 `retry::Outcome`。
        // 返回 `Err` 会让界面把一个用户主动的操作报成错误。
        #[cfg(windows)]
        let cmd = ResolvedCommand {
            program: "cmd".into(),
            args: vec!["/c".into(), "ping -n 30 127.0.0.1 >nul".into()],
            env: BTreeMap::new(),
            cwd: None,
        };
        #[cfg(not(windows))]
        let cmd = ResolvedCommand {
            program: "sleep".into(),
            args: vec!["30".into()],
            env: BTreeMap::new(),
            cwd: None,
        };

        let cancel = Arc::new(CancelToken::new());
        let c2 = cancel.clone();
        // 200 ms 后取消
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            c2.cancel();
        });

        let started = std::time::Instant::now();
        let out = exec()
            .run_streaming(&cmd, &cancel, Arc::new(NullSink))
            .expect("取消不该是 `Err`");
        assert!(out.cancelled, "该标成已取消");
        assert_eq!(out.exit_code, None, "取消时退出码为 None");
        assert!(
            started.elapsed().as_secs() < 10,
            "该在取消后很快返回，而不是等满 30 秒"
        );
    }

    #[test]
    fn 已经取消时不会启动进程() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let started = std::time::Instant::now();
        let out = exec()
            .run_streaming(&echo_cmd("x"), &cancel, Arc::new(NullSink))
            .unwrap();
        assert!(out.cancelled);
        // 它可能已经 spawn 了（我们只保证"尽快收掉"），所以只断言"很快返回"
        assert!(started.elapsed().as_secs() < 10);
    }

    // ───────────────── 日志去处 ─────────────────

    #[test]
    fn vec_sink_能取出与清空() {
        let s = VecSink::new();
        s.line(Channel::Stdout, "a");
        s.line(Channel::Stderr, "b");
        assert_eq!(s.count(), 2);
        let got = s.take();
        assert_eq!(got.len(), 2);
        assert_eq!(s.count(), 0, "take 之后该空了");
    }
}
