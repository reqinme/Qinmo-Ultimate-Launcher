//! # 磁盘预分配与进度落盘（M1 · 《下载引擎设计规格》§6）
//!
//! ## 一、⚠️ 为什么**不能用 `set_len`**（规格明确点名）
//!
//! 规格 §6 的依据（NexBox `:1640, 2143`，理由在 `:3243` 的注释）：
//!
//! > **Windows 预分配**：`SetFileInformationByHandle(FileAllocationInfo)`，
//! > **不要用 `set_len`** —— **`set_len` 产生稀疏文件，导致碎片与延迟 ENOSPC**。
//!
//! 两条后果各自都很具体：
//!
//! | 后果 | 用户看到什么 |
//! |---|---|
//! | **碎片** | 下载一个大文件后磁盘变得很碎，后续读写变慢 |
//! | **延迟 ENOSPC** | **写到一半才发现磁盘满** —— 而那时已经下完了 90% |
//!
//! 第二条尤其恶劣：`set_len` 让文件系统**以为**空间已经占住，
//! 而实际数据是在写入时才分配的。所以"预分配成功"这个信号**是假的**。
//!
//! ### 我们怎么做（**零依赖**）
//!
//! 规格给的是 Win32 API，而我们**不引 `windows-sys`**（每多一条依赖就多一条要审的许可）。
//! 所以用一条**语义等价、零依赖**的做法：**实际写零**。
//!
//! | 做法 | 是否非稀疏 | 是否零依赖 | 代价 |
//! |---|---|---|---|
//! | `set_len` | ❌ **稀疏** | ✅ | **正是规格禁止的那个** |
//! | `windows-sys` + `SetFileInformationByHandle` | ✅ | ❌ 一条依赖 | — |
//! | **实际写零**（本模块） | ✅ | ✅ | 一次 O(n) 的写 |
//!
//! **代价说清**：它要**真的写一遍零**，所以比"分配"慢。
//! 但下载一个 4 MB 的库文件时，这几十毫秒被网络完全掩盖；
//! 而**"预分配信号是真的"**这件事值得那点代价。
//!
//! ⚠️ **它与"校验"是互补的两道防线**：预分配保证"空间真的占住了"，
//! 校验保证"内容是完整的"。少了前者，后者的失败会发生在**磁盘已满**之后。

use qul_core::download::Progress;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 预分配的写块大小（规格 §6：**128 KB 读取块**）。
pub const CHUNK: usize = 128 * 1024;

/// **非稀疏预分配**：真的把文件写成 `size` 字节的零。
///
/// 返回实际写入的字节数。
///
/// ## 为什么这是 `pub` 而不是内部函数
///
/// 因为它有一个**可验证的性质**（"实际占盘 = 请求长度"），
/// 而那正是规格 §8 验收项 10 要测的东西。把它暴露出来，
/// 测试才能不经过整个下载流程去验它。
pub fn preallocate_non_sparse(path: &Path, size: u64) -> Result<u64, String> {
    let mut f = std::fs::File::create(path).map_err(|e| e.kind().to_string())?;
    let zero = vec![0u8; CHUNK];
    let mut left = size;
    while left > 0 {
        let n = left.min(CHUNK as u64) as usize;
        f.write_all(&zero[..n]).map_err(|e| e.kind().to_string())?;
        left -= n as u64;
    }
    // **必须 sync**：否则"预分配成功"只到操作系统缓存里，
    // 而那正是我们要避免的那种**假信号**（延迟 ENOSPC 就是它的一个后果）。
    f.sync_all().map_err(|e| e.kind().to_string())?;
    Ok(size)
}

/// 一个文件是不是**非稀疏的**（实际占盘 ≈ 逻辑长度）。
///
/// 判据只用标准库能给的东西：**文件长度**与**分配到的块数**。
/// 在 Windows 上 `std` 不给块数，所以那里退化为"长度对不对" ——
/// 而**这一点必须写在实现里**，否则测试会以为它验了稀疏性其实没验。
///
/// 返回 `(逻辑长度, 是否可判定为非稀疏)`。
pub fn sparseness_probe(path: &Path) -> Result<(u64, Option<bool>), String> {
    let meta = std::fs::metadata(path).map_err(|e| e.kind().to_string())?;
    let len = meta.len();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // 512 字节块口径（`st_blocks` 的单位）
        let allocated = meta.blocks() * 512;
        // 允许一点元数据开销：分配 >= 长度即视为非稀疏
        Ok((len, Some(allocated >= len)))
    }
    #[cfg(not(unix))]
    {
        // Windows 上 `std` 不给"实际占盘"。**不假装能给。**
        Ok((len, None))
    }
}

// ───────────────────────── 进度落盘（每 3 秒批量）─────────────────────────

/// 落盘间隔（规格 §6：`DB_SAVE_INTERVAL_SECS = 3`）。
pub const SAVE_INTERVAL_MS: u64 = 3_000;

/// 一个任务的**可续传状态**。
///
/// 它要跨进程存活（用户关掉启动器再打开），所以是可序列化的。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TaskRecord {
    /// 目标的稳定标识（**相对路径**，不是绝对路径 ——
    /// 绝对路径里有用户名，而这份状态可能要进诊断包）
    pub key: String,
    pub url: String,
    /// 已完成的总字节（**含续传历史**，与 I7 的 `file_bytes` 同一个口径）
    pub file_bytes: u64,
    pub total: Option<u64>,
    /// 期望哈希（有的话）
    pub expected_sha1: Option<String>,
}

/// **进度落盘器**：批量、原子、按时间节流。
///
/// ## 为什么是"批量"而不是"每次变更都写"
///
/// 因为下载期间进度**每几百毫秒就变一次**，而每次都写盘意味着
/// **下载速度被磁盘 I/O 吃掉一部分**。
///
/// ## 但"每次变更原子"这一条不能省（规格原文）
///
/// 规格 §6：
///
/// > 状态持久化：**每 3 秒**批量落盘，且**每次变更原子（tmp + rename）**
///
/// 也就是两件事**并不冲突**：
///
/// | 维度 | 策略 |
/// |---|---|
/// | **何时写** | 每 3 秒批量（省 I/O） |
/// | **怎么写** | 每次都是 tmp + rename（**所以断电不会留下半截状态**） |
///
/// 一个"直接覆盖写"的实现会在断电时留下一份**半截 JSON** ——
/// 而它下次启动时无法解析，于是**已下载的几 GB 进度全部作废**。
pub struct ProgressStore {
    path: PathBuf,
    pending: Vec<TaskRecord>,
    last_save_ms: u64,
    /// **是否曾经落过盘。**
    ///
    /// 见 `should_save` 的文档 —— **不能用 `last_save_ms == 0` 当哨兵**。
    ever_saved: bool,
    interval_ms: u64,
    /// 累计落盘次数（测试要断言"批量"真的发生了）
    saves: u64,
}

impl ProgressStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            pending: Vec::new(),
            last_save_ms: 0,
            ever_saved: false,
            interval_ms: SAVE_INTERVAL_MS,
            saves: 0,
        }
    }

    /// 换一个落盘间隔（测试用它把 3 秒缩短）。
    pub fn with_interval(mut self, ms: u64) -> Self {
        self.interval_ms = ms;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 落盘次数（测试断言用）。
    pub fn saves(&self) -> u64 {
        self.saves
    }

    /// 待落盘的记录数。
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// **提交一条变更**（内存里），返回"这一刻要不要落盘"。
    ///
    /// 同一个 `key` 的多次提交**只保留最后一条** —— 这正是"批量"的意义：
    /// 一份被下了 300 次的文件在内存里只有一条记录。
    pub fn update(&mut self, rec: TaskRecord, now_ms: u64) -> bool {
        match self.pending.iter_mut().find(|r| r.key == rec.key) {
            Some(existing) => *existing = rec,
            None => self.pending.push(rec),
        }
        self.should_save(now_ms)
    }

    /// 到时间了吗。
    /// 到时间了吗。
    ///
    /// ## ⚠️ "第一次"必须靠一个显式标记判断，不能靠 `last_save_ms == 0`
    ///
    /// 第一版写的是 `now_ms - self.last_save_ms >= interval` ——
    /// 而**第一次调用时 `last_save_ms` 就是 0**，于是
    /// `now(1000) - 0 = 1000 < 3000` → **首次不落盘**。
    ///
    /// 那意味着"刚开始下载就崩了"会让那次进度**完全不存在**，
    /// 而用户下次得从头下。
    ///
    /// **这与 `qul_core::download::Throttle` 里那个缺陷是同一个**：
    /// 用"某个合法取值范围里的值"（这里是 0）当哨兵。
    /// 我在那个类型里修过一次，**在另一个类型里又犯了一次** ——
    /// 所以这一条纪律现在写进两处的注释里。
    pub fn should_save(&self, now_ms: u64) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        // **第一次必须落盘**：否则"刚开始下载就崩了"会让那次进度
        // **完全不存在**，而用户下次得从头下。
        if !self.ever_saved {
            return true;
        }
        now_ms.saturating_sub(self.last_save_ms) >= self.interval_ms
    }

    /// **真的落盘**（原子：tmp + rename）。返回写入的记录数。
    ///
    /// 调用方**不必**先判 `should_save` —— 它可以直接调，
    /// 而"没到时间"时这里会返回 0（**不写盘**）。
    pub fn flush(&mut self, now_ms: u64) -> Result<usize, String> {
        if self.pending.is_empty() {
            return Ok(0);
        }
        if self.ever_saved && now_ms.saturating_sub(self.last_save_ms) < self.interval_ms {
            return Ok(0);
        }
        self.write_now(now_ms)
    }

    /// **强制落盘**（无视节流）。用于"任务完成/取消/退出前" ——
    /// 那三个时刻的进度**必须**是准的，因为它决定下次要不要重下。
    pub fn flush_forced(&mut self, now_ms: u64) -> Result<usize, String> {
        if self.pending.is_empty() {
            return Ok(0);
        }
        self.write_now(now_ms)
    }

    fn write_now(&mut self, now_ms: u64) -> Result<usize, String> {
        let n = self.pending.len();
        let json = serde_json::to_vec_pretty(&self.pending).map_err(|e| e.to_string())?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.kind().to_string())?;
        }
        // **原子**：tmp + rename（规格原文要求"每次变更原子"）
        let tmp = self.path.with_extension("progress-tmp");
        {
            let mut f = std::fs::File::create(&tmp).map_err(|e| e.kind().to_string())?;
            f.write_all(&json).map_err(|e| e.kind().to_string())?;
            f.sync_all().map_err(|e| e.kind().to_string())?;
        }
        if self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| e.kind().to_string())?;
        self.last_save_ms = now_ms;
        self.ever_saved = true;
        self.saves += 1;
        Ok(n)
    }

    /// 读回已落盘的状态。**不存在或损坏都返回空** ——
    /// 一份读不出来的进度状态不该阻断下载，它只意味着"从头开始"。
    pub fn load(&self) -> Vec<TaskRecord> {
        match std::fs::read_to_string(&self.path) {
            Ok(t) => serde_json::from_str(&t).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }
}

/// 从 [`Progress`] 造一条记录。
pub fn record_from(
    key: &str,
    url: &str,
    p: &Progress,
    expected_sha1: Option<String>,
) -> TaskRecord {
    TaskRecord {
        key: key.to_string(),
        url: url.to_string(),
        file_bytes: p.file_bytes,
        total: p.total,
        expected_sha1,
    }
}

/// 从一条记录恢复 [`Progress`]（**两个字节口径都要对**）。
///
/// 恢复后 `session_bytes` **必须是 0**：本次会话还没下任何东西。
/// 设成 `file_bytes` 会让界面显示"本次已下载 3 GB"——
/// 而用户刚刚打开启动器。
pub fn progress_from(rec: &TaskRecord) -> Progress {
    let mut p = Progress::new(rec.total);
    p.resume_from(rec.file_bytes);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "qul-store-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|x| x.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rec(key: &str, bytes: u64) -> TaskRecord {
        TaskRecord {
            key: key.into(),
            url: format!("http://x/{key}"),
            file_bytes: bytes,
            total: Some(1_000_000),
            expected_sha1: None,
        }
    }

    // ───────────────── 预分配 ─────────────────

    #[test]
    fn 预分配真的写出那么多字节() {
        let d = tmpdir("prealloc");
        let p = d.join("a.bin");
        let n = preallocate_non_sparse(&p, 300_000).unwrap();
        assert_eq!(n, 300_000);
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 300_000);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 预分配的内容是零() {
        let d = tmpdir("prealloc-zero");
        let p = d.join("b.bin");
        preallocate_non_sparse(&p, 1000).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(bytes.len(), 1000);
        assert!(
            bytes.iter().all(|b| *b == 0),
            "预分配必须是零而不是未初始化数据"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 预分配跨块边界也对() {
        // CHUNK 是 128 KB，而预分配要处理"不是块整数倍"的长度。
        let d = tmpdir("prealloc-odd");
        for size in [
            CHUNK as u64 - 1,
            CHUNK as u64,
            CHUNK as u64 + 1,
            3 * CHUNK as u64 + 7,
        ] {
            let p = d.join(format!("s{size}.bin"));
            preallocate_non_sparse(&p, size).unwrap();
            assert_eq!(std::fs::metadata(&p).unwrap().len(), size, "size={size}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 零长度预分配是可接受的() {
        // 一个 0 字节的文件（例如一个空标记文件）不该让预分配报错。
        let d = tmpdir("prealloc-zero-len");
        let p = d.join("empty.bin");
        assert_eq!(preallocate_non_sparse(&p, 0).unwrap(), 0);
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 稀疏性探测在_windows_上诚实地说不知道() {
        // ⚠️ Windows 上 `std` 不给"实际占盘"，所以这里**不假装能给**。
        // 这条测试的作用是**把那件事写下来** ——
        // 否则规格 §8 验收项 10（"断言下载中的文件是非稀疏"）
        // 会被误以为在这个平台上也验过了。
        let d = tmpdir("sparse");
        let p = d.join("c.bin");
        preallocate_non_sparse(&p, 4096).unwrap();
        let (len, verdict) = sparseness_probe(&p).unwrap();
        assert_eq!(len, 4096);
        #[cfg(unix)]
        assert_eq!(verdict, Some(true), "unix 上应当能判定非稀疏");
        #[cfg(not(unix))]
        assert_eq!(
            verdict, None,
            "windows 上 std 不给实际占盘 —— 必须返回 None 而不是假装知道"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── 进度落盘 ─────────────────

    #[test]
    fn 批量提交只保留每个任务的最后一条() {
        // 一份被下了 300 次的文件在内存里只有一条记录 ——
        // 这正是"批量"的意义。
        let d = tmpdir("batch");
        let mut s = ProgressStore::new(d.join("p.json")).with_interval(3000);
        for i in 0..300u64 {
            s.update(rec("a.jar", i * 1000), i);
        }
        assert_eq!(s.pending(), 1, "同一个 key 只保留一条");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 第一次提交会立刻落盘_之后才按间隔节流() {
        // ⚠️ 这条测试的第一版断言"第一次也不该落盘"，而那是错的：
        // **第一次落盘是必要的** —— 否则"刚开始下载就崩了"会让
        // 那次进度完全不存在，而用户下次得从头下。
        //
        // 而节流的意义是"之后的密集变更不必每次都写盘"（省 I/O）。
        // 两件事不冲突：**第一次写、之后每 3 秒写一次**。
        let d = tmpdir("throttle");
        let mut s = ProgressStore::new(d.join("p.json")).with_interval(3000);

        // 第一次：`last_save_ms == 0` ⇒ 立刻落盘
        s.update(rec("a", 10), 1000);
        assert!(s.should_save(1000), "第一次必须允许落盘");
        assert_eq!(s.flush(1000).unwrap(), 1);
        assert_eq!(s.saves(), 1);

        // 之后被节流
        s.update(rec("a", 20), 1500);
        assert!(!s.should_save(1500), "1500ms 距上次落盘只有 500ms");
        assert_eq!(s.flush(1500).unwrap(), 0, "没到时间不该写盘");
        assert_eq!(s.saves(), 1);

        // 到 3 秒后又能落
        s.update(rec("a", 30), 4200);
        assert!(s.should_save(4200), "4200-1000 = 3200 >= 3000");
        assert_eq!(s.flush(4200).unwrap(), 1);
        assert_eq!(s.saves(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 强制落盘无视节流() {
        // 完成/取消/退出前的进度**必须**是准的 ——
        // 它决定下次要不要重下。
        let d = tmpdir("forced");
        let mut s = ProgressStore::new(d.join("p.json")).with_interval(3000);
        s.update(rec("a", 10), 1000);
        s.flush(1000).unwrap();
        s.update(rec("a", 999), 1100);
        assert!(!s.should_save(1100));
        assert_eq!(s.flush_forced(1100).unwrap(), 1, "强制应当写盘");
        assert_eq!(s.load()[0].file_bytes, 999);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 落盘是原子的且不留半截文件() {
        // 规格原文：**每次变更原子（tmp + rename）**。
        // 一个"直接覆盖写"的实现会在断电时留下半截 JSON，
        // 而它下次启动时无法解析 → **已下载的几 GB 进度全部作废**。
        let d = tmpdir("atomic");
        let p = d.join("p.json");
        let mut s = ProgressStore::new(&p).with_interval(0);
        s.update(rec("a", 123), 1);
        s.flush(1).unwrap();
        assert!(p.is_file());
        assert!(
            !p.with_extension("progress-tmp").exists(),
            "不该留下临时文件"
        );
        // 内容必须是完整合法的 JSON
        let text = std::fs::read_to_string(&p).unwrap();
        let back: Vec<TaskRecord> = serde_json::from_str(&text).unwrap();
        assert_eq!(back[0].file_bytes, 123);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 读回损坏的状态返回空而不是报错() {
        // 一份读不出来的进度状态不该阻断下载，它只意味着"从头开始"。
        let d = tmpdir("corrupt");
        let p = d.join("p.json");
        std::fs::write(&p, "这不是 JSON").unwrap();
        let s = ProgressStore::new(&p);
        assert!(s.load().is_empty());
        // 文件不存在也返回空
        let s2 = ProgressStore::new(d.join("nope.json"));
        assert!(s2.load().is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 状态能跨会话往返() {
        let d = tmpdir("roundtrip");
        let p = d.join("p.json");
        let mut s = ProgressStore::new(&p).with_interval(0);
        s.update(
            TaskRecord {
                key: "libs/a.jar".into(),
                url: "http://x/a.jar".into(),
                file_bytes: 4096,
                total: Some(8192),
                expected_sha1: Some("da39a3ee5e6b4b0d3255bfef95601890afd80709".into()),
            },
            1,
        );
        s.flush(1).unwrap();

        // 新会话读回
        let s2 = ProgressStore::new(&p);
        let recs = s2.load();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].file_bytes, 4096);
        assert_eq!(
            recs[0].expected_sha1.as_deref(),
            Some("da39a3ee5e6b4b0d3255bfef95601890afd80709")
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── 与 Progress 的对接（I7）─────────────────

    #[test]
    fn 从记录恢复时本次会话字节是零() {
        // ⚠️ 设成 `file_bytes` 会让界面显示"本次已下载 3 GB"——
        // 而用户刚刚打开启动器。
        let r = TaskRecord {
            key: "a".into(),
            url: "http://x".into(),
            file_bytes: 3_000_000,
            total: Some(10_000_000),
            expected_sha1: None,
        };
        let p = progress_from(&r);
        assert_eq!(p.file_bytes, 3_000_000, "总进度含历史");
        assert_eq!(p.session_bytes, 0, "本次会话必须是 0");
        assert_eq!(p.ratio(), Some(0.3));
    }

    #[test]
    fn 从_progress_造记录用的是总进度口径() {
        let mut p = Progress::new(Some(1000));
        p.resume_from(400);
        p.add(50);
        let r = record_from("k", "http://x", &p, None);
        assert_eq!(r.file_bytes, 450, "记录的是文件总进度，不是本次新增");
        assert_eq!(r.total, Some(1000));
        // 往返一致
        let back = progress_from(&r);
        assert_eq!(back.file_bytes, 450);
    }

    #[test]
    fn 空进度不触发落盘() {
        // 一个"没有待写内容也要写一次"的实现会在空闲时反复动磁盘。
        let d = tmpdir("empty");
        let mut s = ProgressStore::new(d.join("p.json")).with_interval(0);
        assert!(!s.should_save(999999));
        assert_eq!(s.flush(999999).unwrap(), 0);
        assert_eq!(s.saves(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
