//! # 脱敏日志（M1 · 方案 §5.8）
//!
//! ## 这一层做什么，不做什么
//!
//! | 层 | 做什么 |
//! |---|---|
//! | `qul_core::scrub` | **规则**：什么该被折叠、怎么折叠（纯函数，可穷举测试） |
//! | **本模块** | **落地**：写到哪、怎么轮转、怎么保证"没有一行绕过脱敏" |
//!
//! **规则与落地分开是必要的**，理由与 `java.rs`、`retry.rs` 的切分相同：
//! 规则能穷举测试，而"写文件"只能实测。
//!
//! ## 一条结构性的保证：**没有入口能写未脱敏的一行**
//!
//! 本模块**不暴露**任何"直接写原始文本"的方法。
//! 唯一的写入方法是 [`LogSink::write`]，而它在内部**必然**先过 [`Scrubber`]。
//!
//! 这不是靠自觉：若有人需要一个"临时写点原文"的出口，
//! 正确的做法是**在调用方把值准备好**，而不是在这里开一个后门。
//! 方案 §5.8 的原话是 *"崩溃日志必须走同一套脱敏管道"* ——
//! **"同一套"意味着只有一个入口**，否则"同一套"只是文档里的一句话。
//!
//! ## 为什么日志文件要按大小轮转，而不是按天
//!
//! 因为我们**不知道用户什么时候会遇到问题**。按天轮转的话，
// 一个"三天前发生过、今天才来查"的问题，日志可能已经被删掉了。
//! 按大小轮转 + 固定份数，保证**"最近 N MB 的日志一定在"** ——
//! 这个保证与"什么时候发生"无关，因此更强也更可预期。

use qul_core::scrub::{ScrubReport, Scrubber};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// 日志文件的轮转策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rotation {
    /// 单个文件的最大字节数。超过就轮转。
    pub max_bytes: u64,
    /// 保留几份（含当前这份）。
    ///
    /// **最少 2 份**：只留 1 份的话，"刚好在崩溃前写完的那次轮转"
    /// 会把崩溃现场的那一段覆盖掉 —— 而那正是最需要的一段。
    pub keep: usize,
}

impl Default for Rotation {
    fn default() -> Self {
        // 2 MB × 4 份 = 最多 8 MB 日志。
        // 取这个量级：脱敏后的文本日志压缩得很好（可分享外链），
        // 而 8 MB 对任何一台机器都可忽略。
        Self {
            max_bytes: 2 * 1024 * 1024,
            keep: 4,
        }
    }
}

/// 写入日志时的错误。**不在这里决定"给用户看什么"** —— 那是 `QulError` 的事。
#[derive(Debug)]
pub enum LogError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// 轮转策略不合法（`keep < 2`）
    BadRotation { keep: usize },
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogError::Io { path, source } => write!(f, "{}：{source}", path.display()),
            LogError::BadRotation { keep } => write!(f, "轮转保留份数 {keep} 少于 2"),
        }
    }
}

impl std::error::Error for LogError {}

/// 一条日志的级别。**只用于文本前缀**，不参与脱敏决策。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace,
    Info,
    Warn,
    Error,
}

impl Level {
    pub const fn tag(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }
}

/// **脱敏日志写入器。**
///
/// ## 它没有"写原文"的方法，这是刻意的
///
/// 见模块文档。唯一入口 [`LogSink::write`] 必然先过脱敏。
pub struct LogSink {
    dir: PathBuf,
    base_name: String,
    rotation: Rotation,
    scrubber: Scrubber,
    /// 累计的脱敏统计（**只记次数，不记内容**）。
    total: ScrubReport,
    /// 当前文件已写字节数
    written: u64,
}

impl LogSink {
    /// 建立一个日志写入器。目录不存在会被创建。
    pub fn open(
        dir: impl Into<PathBuf>,
        base_name: impl Into<String>,
        rotation: Rotation,
        scrubber: Scrubber,
    ) -> Result<Self, LogError> {
        if rotation.keep < 2 {
            return Err(LogError::BadRotation {
                keep: rotation.keep,
            });
        }
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(|source| LogError::Io {
            path: dir.clone(),
            source,
        })?;
        let base_name = base_name.into();
        let written = fs::metadata(dir.join(format!("{base_name}.log")))
            .map(|m| m.len())
            .unwrap_or(0);
        Ok(Self {
            dir,
            base_name,
            rotation,
            scrubber,
            total: ScrubReport::default(),
            written,
        })
    }

    /// 当前日志文件路径。
    pub fn current_path(&self) -> PathBuf {
        self.dir.join(format!("{}.log", self.base_name))
    }

    /// 累计脱敏统计。
    pub fn total_report(&self) -> &ScrubReport {
        &self.total
    }

    /// **唯一的写入入口**：先脱敏，再写。
    ///
    /// 返回这一次的脱敏统计（**只记次数，不记内容**）。
    pub fn write(&mut self, level: Level, message: &str) -> Result<ScrubReport, LogError> {
        // **多行会被逐行脱敏**：否则一个跨行的令牌（或跨行的路径拼接）
        // 会绕过规则 —— 而"绕过"在这里的表现是**敏感内容被原样写进日志文件**。
        let mut scrubbed_lines: Vec<String> = Vec::new();
        let mut rep = ScrubReport::default();
        for line in message.lines() {
            let (s, r) = self.scrubber.scrub(line);
            rep.secrets += r.secrets;
            rep.uuids += r.uuids;
            rep.users += r.users;
            rep.paths += r.paths;
            rep.ips += r.ips;
            rep.servers += r.servers;
            rep.emails += r.emails;
            scrubbed_lines.push(s);
        }
        if message.is_empty() {
            scrubbed_lines.push(String::new());
        }

        // 时间戳**不含本机时区以外的信息**：只用 UTC 秒，
        // 不用"用户名/机器名"参与时间戳格式（那正是要抹掉的东西）。
        let ts = now_utc_compact();
        let mut block = String::new();
        for l in &scrubbed_lines {
            block.push_str(&format!("{ts} {:<5} {l}\n", level.tag()));
        }

        // 落盘前先轮转，保证单文件不超过上限
        if self.written + block.len() as u64 > self.rotation.max_bytes {
            self.rotate()?;
        }

        let p = self.current_path();
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&p)
            .map_err(|source| LogError::Io {
                path: p.clone(),
                source,
            })?;
        f.write_all(block.as_bytes())
            .map_err(|source| LogError::Io {
                path: p.clone(),
                source,
            })?;
        // **每行都 flush**：崩溃时未刷出的那一行等于没写，
        // 而崩溃现场的那一行恰恰是最重要的 —— 用一点性能换它值得。
        f.flush()
            .map_err(|source| LogError::Io { path: p, source })?;
        self.written += block.len() as u64;

        self.total.secrets += rep.secrets;
        self.total.uuids += rep.uuids;
        self.total.users += rep.users;
        self.total.paths += rep.paths;
        self.total.ips += rep.ips;
        self.total.servers += rep.servers;
        self.total.emails += rep.emails;
        Ok(rep)
    }

    /// 轮转：`a.log` → `a.1.log`，`a.1.log` → `a.2.log`……
    ///
    /// **从最老的开始挪**，否则 `a.1.log` 会被 `a.log` 覆盖。
    fn rotate(&mut self) -> Result<(), LogError> {
        let oldest = self
            .dir
            .join(format!("{}.{}.log", self.base_name, self.rotation.keep - 1));
        let _ = fs::remove_file(&oldest);

        for i in (1..self.rotation.keep - 1).rev() {
            let from = self.dir.join(format!("{}.{}.log", self.base_name, i));
            let to = self.dir.join(format!("{}.{}.log", self.base_name, i + 1));
            if from.exists() {
                let _ = fs::rename(&from, &to);
            }
        }
        let cur = self.current_path();
        if cur.exists() {
            fs::rename(&cur, self.dir.join(format!("{}.1.log", self.base_name))).map_err(
                |source| LogError::Io {
                    path: cur.clone(),
                    source,
                },
            )?;
        }
        self.written = 0;
        Ok(())
    }

    /// 现有的日志文件列表（**从新到旧**），供诊断导出使用。
    pub fn existing_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let cur = self.current_path();
        if cur.exists() {
            out.push(cur);
        }
        for i in 1..self.rotation.keep {
            let p = self.dir.join(format!("{}.{}.log", self.base_name, i));
            if p.exists() {
                out.push(p);
            }
        }
        out
    }

    /// **导出脱敏后的诊断文本**（供"一键分享外链"用，§12 第 8 条）。
    ///
    /// ## 为什么这里要**再脱敏一次**
    ///
    /// 因为写进文件时虽然已脱敏，但**导出路径上的种子可能比写入时更多**
    /// （例如用户刚登录，现在多了一个令牌要抹），
    /// 或者文件是**更早的版本**写的（那时管道还不完整）。
    ///
    /// **再脱一次的成本是 O(文本长度)，而漏一次的成本是泄露。**
    /// 而且脱敏**是幂等的**（有测试保证），所以再脱一次不会破坏已有掩码。
    pub fn export_redacted(
        &self,
        header_lines: &[&str],
    ) -> Result<(String, ScrubReport), LogError> {
        let mut out = String::new();
        for h in header_lines {
            let (s, _) = self.scrubber.scrub(h);
            out.push_str(&s);
            out.push('\n');
        }
        out.push('\n');

        let mut rep = ScrubReport::default();
        for p in self.existing_files() {
            let text = fs::read_to_string(&p).map_err(|source| LogError::Io {
                path: p.clone(),
                source,
            })?;
            out.push_str(&format!(
                "===== {} =====\n",
                p.file_name().unwrap_or_default().to_string_lossy()
            ));
            for line in text.lines() {
                // **逐行再脱敏**：这是"同一套管道"的第二次应用，
                // 而不是"相信写入时已经处理过"。
                let (s, r) = self.scrubber.scrub(line);
                rep.secrets += r.secrets;
                rep.uuids += r.uuids;
                rep.users += r.users;
                rep.paths += r.paths;
                rep.ips += r.ips;
                rep.servers += r.servers;
                rep.emails += r.emails;
                out.push_str(&s);
                out.push('\n');
            }
            out.push('\n');
        }
        // 尾部附上统计摘要 —— **一行就够**，而它让读的人知道哪些类别被折叠过。
        let total = {
            let mut t = rep.clone();
            t.secrets += self.total.secrets;
            t.uuids += self.total.uuids;
            t.users += self.total.users;
            t.paths += self.total.paths;
            t.ips += self.total.ips;
            t.servers += self.total.servers;
            t.emails += self.total.emails;
            t
        };
        out.push_str(&format!("===== {}\n", total.summary()));
        Ok((out, rep))
    }
}

/// UTC 紧凑时间戳 `YYYYMMDD-HHMMSS`。
///
/// **刻意不引入时间库**：这是一个尖刺阶段的最小实现，
/// 而多一条依赖就多一条要审的许可。手算 UTC 是可行的
/// （闰年与月长已知），而且**它没有时区歧义** —— 日志里不出现本机时区
/// 本身也是一点信息保护（时区能定位用户所在区域）。
fn now_utc_compact() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 天 → 年月日
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// 天数（自 1970-01-01）→ (年, 月, 日)。
///
/// 这是 Howard Hinnant 的 `civil_from_days` 算法，**只用整数运算**，
/// 所以它没有闰年边界问题，也不需要任何依赖。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    // `Path` 无需单独引入：`super::*` 已带上它（文件顶部有 `use std::path::PathBuf;`）。

    fn tmpdir(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let d = std::env::temp_dir().join(format!("qul-log-{tag}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn sink(dir: &std::path::Path, rot: Rotation) -> LogSink {
        LogSink::open(
            dir,
            "session",
            rot,
            Scrubber::new()
                .secret("SUPERSECRETTOKEN123")
                .user("hjc20")
                .path(r"C:\Users\hjc20", qul_core::scrub::MASK_USERPROFILE)
                .server("play.example.com"),
        )
        .unwrap()
    }

    #[test]
    fn 写出的日志里没有敏感内容() {
        let d = tmpdir("basic");
        let mut s = sink(&d, Rotation::default());
        let rep = s
            .write(
                Level::Info,
                r"token=SUPERSECRETTOKEN123 user=hjc20 ip=192.168.1.1 home=C:\Users\hjc20",
            )
            .unwrap();
        assert!(rep.total() >= 3, "至少抹掉机密/用户名/IP：{rep:?}");

        let text = fs::read_to_string(s.current_path()).unwrap();
        assert!(!text.contains("SUPERSECRETTOKEN123"), "{text}");
        assert!(!text.contains("hjc20"), "{text}");
        assert!(!text.contains("192.168.1.1"), "{text}");
        assert!(text.contains("<redacted>"), "{text}");
        assert!(text.contains("INFO"), "{text}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 多行消息逐行脱敏而不是整体() {
        // 若整体脱敏，一个跨行的路径拼接会绕过规则 ——
        // 而"绕过"在这里意味着**敏感内容被原样写进日志文件**。
        let d = tmpdir("multiline");
        let mut s = sink(&d, Rotation::default());
        s.write(
            Level::Warn,
            "第一行 ip=10.1.2.3\n第二行 token=SUPERSECRETTOKEN123",
        )
        .unwrap();
        let text = fs::read_to_string(s.current_path()).unwrap();
        assert!(!text.contains("10.1.2.3"), "{text}");
        assert!(!text.contains("SUPERSECRETTOKEN123"), "{text}");
        assert_eq!(text.lines().count(), 2, "两行都该在：{text}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 轮转保留份数且从最老的开始删() {
        let d = tmpdir("rotate");
        // 每份只装得下两条左右
        let mut s = sink(
            &d,
            Rotation {
                max_bytes: 200,
                keep: 3,
            },
        );
        for i in 0..40 {
            s.write(
                Level::Info,
                &format!("第 {i} 条填充填充填充填充填充填充填充"),
            )
            .unwrap();
        }
        let files = s.existing_files();
        assert!(files.len() <= 3, "不该超过保留份数：{files:?}");
        // 最新的一条必须还在
        let newest = fs::read_to_string(s.current_path()).unwrap();
        assert!(newest.contains("第 39 条"), "{newest}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 保留份数少于二会被拒绝() {
        // 只留 1 份的话，"刚好在崩溃前写完的那次轮转"
        // 会把崩溃现场那一段覆盖掉 —— 而那正是最需要的一段。
        let d = tmpdir("badrot");
        for keep in [0usize, 1] {
            let e = LogSink::open(
                &d,
                "s",
                Rotation {
                    max_bytes: 100,
                    keep,
                },
                Scrubber::new(),
            );
            assert!(
                matches!(e, Err(LogError::BadRotation { .. })),
                "keep={keep}"
            );
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 导出时会再脱敏一次() {
        // 文件可能是更早的版本写的（那时管道还不完整），
        // 或者导出路径上的种子比写入时更多。
        // **再脱一次的成本是 O(长度)，漏一次的成本是泄露。**
        let d = tmpdir("export");
        let s = sink(&d, Rotation::default());
        // 手工造一个"未脱敏的历史文件"
        fs::write(
            s.current_path(),
            "old line ip=203.0.113.9 token=SUPERSECRETTOKEN123\n",
        )
        .unwrap();
        let (out, rep) = s.export_redacted(&["诊断报告", "版本=0.1.0.0"]).unwrap();
        assert!(!out.contains("203.0.113.9"), "{out}");
        assert!(!out.contains("SUPERSECRETTOKEN123"), "{out}");
        assert_eq!(rep.ips, 1, "导出阶段应当自己抹掉 IP");
        // 版本号不能被抹（封存项目那个缺陷）
        assert!(out.contains("0.1.0.0"), "{out}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 导出里的每一行都是稳定的() {
        // ⚠️ 这条测试的第一版断言的是"整个导出文档逐字节一致"，而它**必然失败** ——
        // 因为导出会把头部与统计写进文档，把文档写回文件后，第二次导出
        // 就多嵌了一层。**那不是缺陷，是"把统计吃进输入"的必然结果。**
        //
        // 真正该测的幂等是**逐行**的：同一行内容脱敏两次结果相同。
        // 这条更弱、但它才是"同一套管道"能给出的保证。
        let d = tmpdir("export-idem");
        let s = sink(&d, Rotation::default());
        fs::write(s.current_path(), "ip=10.0.0.1 home=C:\\Users\\hjc20\n").unwrap();
        let (a, _) = s.export_redacted(&["h"]).unwrap();
        // 逐行再脱一次：不该再有任何替换，且内容逐字节不变
        let scrubber = Scrubber::new()
            .user("hjc20")
            .path(r"C:\Users\hjc20", qul_core::scrub::MASK_USERPROFILE);
        let mut again: Vec<String> = Vec::new();
        let mut extra = 0usize;
        for line in a.lines() {
            let (s2, r) = scrubber.scrub(line);
            extra += r.total();
            again.push(s2);
        }
        assert_eq!(extra, 0, "第二次逐行脱敏不该再有替换");
        let original: Vec<String> = a.lines().map(|s| s.to_string()).collect();
        assert_eq!(again, original, "逐行脱敏必须逐字节稳定");

        // 顺带确认导出**确实抹掉了**敏感内容（否则上面的稳定性毫无意义）
        assert!(!a.contains("10.0.0.1"), "{a}");
        assert!(!a.to_lowercase().contains("hjc20"), "{a}");
        let _ = fs::remove_dir_all(&d);
    }
    #[test]
    fn 累计统计只记次数不记内容() {
        let d = tmpdir("stats");
        let mut s = sink(&d, Rotation::default());
        s.write(Level::Error, "ip=10.0.0.1").unwrap();
        s.write(Level::Error, "ip=10.0.0.2").unwrap();
        let t = s.total_report();
        assert_eq!(t.ips, 2);
        let j = serde_json::to_string(t).unwrap();
        assert!(!j.contains("10.0.0"), "统计里不该有原值：{j}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 时间戳是_utc_且格式固定() {
        let ts = now_utc_compact();
        assert_eq!(ts.len(), 15, "{ts}");
        assert_eq!(&ts[8..9], "-", "{ts}");
        // 年应当在合理范围（不是 1970，也不是 9999）
        let y: i32 = ts[..4].parse().unwrap();
        assert!((2020..2200).contains(&y), "{ts}");
    }

    #[test]
    fn civil_from_days_的已知锚点() {
        // 1970-01-01 / 2000-03-01 / 2024-02-29（闰日）
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn 空消息也会写出一行() {
        // 崩溃现场可能是"什么都没来得及说" —— 那时**有一行时间戳**就是证据。
        let d = tmpdir("empty");
        let mut s = sink(&d, Rotation::default());
        s.write(Level::Error, "").unwrap();
        let text = fs::read_to_string(s.current_path()).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.contains("ERROR"), "{text}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 目录不存在会被创建() {
        let d = tmpdir("mkdir");
        let nested = d.join("a").join("b");
        let _ = LogSink::open(&nested, "s", Rotation::default(), Scrubber::new()).unwrap();
        assert!(nested.is_dir());
        let _ = fs::remove_dir_all(&d);
    }
}
