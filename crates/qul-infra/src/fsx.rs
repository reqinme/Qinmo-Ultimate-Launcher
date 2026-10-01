//! # 原子写、文件锁与单实例（M1 · "并发写同一实例不出错"）
//!
//! ## 为什么这一块不能"以后再补"
//!
//! 方案 M1 的出口条件写着 **"并发写同一实例不出错"**，而它旁边写着
//! **原子写 + 文件锁 + 单实例主进程**。三件事是一件事：
//!
//! - **没有原子写**：用户点了"保存设置"时断电 → 配置文件变成**半截 JSON**
//!   → 下次启动读不出来 → 用户的**设置、账户、实例全部丢失**。
//!   这不是"数据损坏"，这是**数据丢失**。
//! - **没有文件锁**：两个进程（或两个任务）同时写同一实例 → 后写的赢，
//!   而先写的那些字节**静默消失**。用户看到的是"我改的东西过一会儿又变回去了"。
//! - **没有单实例**：用户双击两次图标 → 两个实例同时下载同一批文件
//!   → 互相覆盖 → **校验和永远不通过**，而报错只说"下载失败"。
//!
//! ## Windows 上"原子替换"的真实约束（这一条踩过才知道）
//!
//! `std::fs::rename` 在 Windows 上**目标存在时失败**（POSIX 会覆盖）。
//! 所以不能简单地"写临时文件 → rename 覆盖"。
//!
//! 而 `MoveFileExW` 带 `MOVEFILE_REPLACE_EXISTING` 可以，但要引 Win32 依赖；
//! **尖刺阶段每多一条依赖就多一条要审的许可**，所以我们改用**可恢复的两步重命名**：
//!
//! ```text
//!     写 <target>.tmp  →  fsync
//!     <target> 存在?  →  rename <target> → <target>.bak
//!     rename <target>.tmp → <target>
//!     删除 <target>.bak
//! ```
//!
//! **中间任何一步崩溃，都能恢复**：看到 `.bak` 就说明"替换进行到一半"，
//! 而 `.bak` 里是**完整的旧内容**（它是一次 rename 的产物，不是半截写入）。
//! 恢复函数 `recover()` 就是做这件事。
//!
//! **代价说清**：两步重命名有一个极短窗口（`.bak` 已存在、target 尚未出现）。
//! 但因为**每次都是整文件替换**，恢复后的内容**要么是完整的旧版、要么是完整的新版**，
//! 永远不会是半截。这正是我们要的保证。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// 备份后缀。**恢复逻辑靠它识别"替换进行到一半"**，所以不许改。
const BAK: &str = "bak";
/// 临时文件后缀。同名 `.tmp` 存在说明上次写到一半崩了。
const TMP: &str = "tmp";
/// 写入锁后缀。**为什么原子写内部要加锁**见 [`write_atomic`] 的文档。
const LOCK: &str = "lock";

/// 本层的错误。**不含"给用户看的话术"**——那是 `qul_core::error::QulError` 的事。
#[derive(Debug)]
pub enum IoError {
    /// 目标路径不在允许的根之内（**安全边界，不是文件错误**）
    EscapesRoot { path: PathBuf, root: PathBuf },
    /// 拿不到锁（别的进程/任务正在写）
    Locked { path: PathBuf },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoError::EscapesRoot { path, root } => {
                write!(
                    f,
                    "路径 {} 不在允许的根 {} 之内",
                    path.display(),
                    root.display()
                )
            }
            IoError::Locked { path } => write!(f, "{} 已被占用", path.display()),
            IoError::Io { path, source } => write!(f, "{}：{source}", path.display()),
        }
    }
}

impl std::error::Error for IoError {}

fn io_err(path: &Path) -> impl Fn(std::io::Error) -> IoError + '_ {
    move |source| IoError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// 把路径规范化成可比形式（小写 + 统一分隔符 + **消解 `.` 与 `..`**）。
///
/// **不做 canonicalize**：目标文件可能还不存在，而 `canonicalize` 要求存在。
/// 所以这里做的是**词法**归一化，**不解析符号链接**——
/// 符号链接那一层由操作系统在真正打开文件时处理，
/// 我们这里拦的是"路径字符串本身就指向别处"这类越界。
///
/// ## ⚠️ 必须消解 `..`（这一条是被测试抓出来的）
///
/// 第一版只做小写与前缀比较，于是
/// `C:\data\..\..\Windows\System32\x.dll` **通过了检查**——
/// 因为它的字符串**确实以** `c:\data\` 开头。
/// 于是一个 `.zip` 里的 `../../..` 条目就能写任意文件，
/// **而注释里还写着"这条挡住了目录穿越"。**
///
/// 这正是"检测与匹配必须用不同思路"的又一实例：
/// **前缀比较检查的是字符串，而逃逸是路径语义**，两者不等价。
fn normalize(p: &Path) -> String {
    use std::path::Component;
    let mut out: Vec<String> = Vec::new();
    for c in p.components() {
        match c {
            // 盘符 / UNC 前缀：直接起头
            Component::Prefix(pre) => {
                out.clear();
                out.push(pre.as_os_str().to_string_lossy().to_lowercase());
            }
            // 根：丢弃之前的一切（`\x` 是根相对路径）
            Component::RootDir => out.clear(),
            Component::CurDir => {}
            Component::ParentDir => {
                // 有可回退的普通段才回退；退到根就停（`..` 在根上无意义）
                match out.last() {
                    Some(last) if !last.ends_with(':') && !last.ends_with('\\') => {
                        out.pop();
                    }
                    _ => {}
                }
            }
            Component::Normal(seg) => {
                out.push(seg.to_string_lossy().to_lowercase());
            }
        }
    }
    out.join("\\")
}

/// **安全边界**：确认 `path` 落在 `root` 之内。
///
/// 方案 §5.8 的安全基线要求"前端无法绕过命令层直接操作文件"，
/// 而命令层这一侧的对等要求就是：**任何来自外部的路径都不能越界**。
/// 解压时尤其要紧——一个 `..\..\Windows\System32\x.dll` 的 zip 条目
/// 就是一次写任意文件。
///
/// **比较的是规范化后的前缀，且强制加分隔符**：
/// 否则 `C:\data-evil` 会被判成 `C:\data` 的子路径（前缀匹配的经典漏洞）。
pub fn ensure_within(path: &Path, root: &Path) -> Result<(), IoError> {
    let p = normalize(path);
    let mut r = normalize(root);
    if !r.ends_with('\\') {
        r.push('\\');
    }
    let p_with_sep = if p.ends_with('\\') {
        p.clone()
    } else {
        format!("{p}\\")
    };
    if p_with_sep.starts_with(&r) || p == r.trim_end_matches('\\') {
        Ok(())
    } else {
        Err(IoError::EscapesRoot {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        })
    }
}

/// **原子写**：要么目标文件是完整的新内容，要么是完整的旧内容，**不会是半截**。
///
/// ## ⚠️ 为什么内部要加锁（这一条是被并发测试抓出来的）
///
/// 第一版没有加锁，只用"两步重命名"。**单线程一万次都能过**，但两个线程同时写
/// 同一个文件时立刻暴露：
///
/// ```text
///     线程 A: rename(target -> .bak)   成功（target 没了）
///     线程 B: rename(target -> .bak)   失败：系统找不到指定的文件
/// ```
///
/// 也就是 **`Os { code: 2, NotFound }`**。
///
/// 根因不只是"报错"：**`.bak` 是固定名**，所以两个写入者会抢同一个 `.bak`，
/// 而"检查 `target` 是否存在"与"rename 它"之间**没有任何东西保证不变**——
/// 这是典型的 **TOCTOU**（check 与 use 之间状态被改掉）。
///
/// 修法：**写入前对目标路径加一把锁**，把"检查 + 替换 + 清理"整段串行化。
/// 锁文件是 `<target>.lock`，与 `.tmp` / `.bak` **同名不同后缀**，
/// 由 [`write_lock_path`] 统一生成（**前缀相同的三个路径必须来自同一个函数**，
/// 否则改一处就会留下一类孤儿文件）。
///
/// ## 锁文件会留在磁盘上
///
/// 这是**有意的**：用"文件存在与否"当锁哨兵，会在崩溃后留下一个**永远拿不到的锁**；
/// 而用操作系统的文件锁（本函数用的）**进程一死锁就自动释放**，
/// 所以文件留着没有害处——它只是一个 0 字节的把手。
///
/// ## 与单实例守卫的关系
///
/// 单实例守卫防的是**用户双击两次图标**；本函数防的是**同一进程内两个任务
/// 同时写同一个文件**。两者都要有：前者拦不住前者，后者拦不住后者。
pub fn write_atomic(path: &Path, bytes: &[u8], root: Option<&Path>) -> Result<(), IoError> {
    if let Some(root) = root {
        ensure_within(path, root)?;
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io_err(dir))?;
    }

    // 整段替换过程串行化：从"清 .tmp"到"删 .bak"之间，同一个目标只有一个写入者。
    let _guard = FileLock::acquire(&write_lock_path(path))?;

    // 锁内先做恢复：上一次可能崩在"target 已挪走、新的还没就位"。
    // 用 `recover_locked` 而不是 `recover`——后者会再取一次锁，**自死锁**。
    recover_locked(path)?;

    let tmp = with_ext(path, TMP);
    // 上一次崩在"写完 tmp 之前"会留下一个 tmp。**必须清掉**，
    // 否则新内容会写到一个残留文件后面，读出来是两段拼起来的。
    let _ = fs::remove_file(&tmp);

    {
        let mut f = File::create(&tmp).map_err(io_err(&tmp))?;
        f.write_all(bytes).map_err(io_err(&tmp))?;
        // 没有 sync_all 的话，"写完了"只是到了操作系统缓存里，
        // 断电时它可能还在缓存里 —— 那样 rename 出来的就是空文件。
        f.sync_all().map_err(io_err(&tmp))?;
    }

    let bak = with_ext(path, BAK);
    let _ = fs::remove_file(&bak);

    let had_target = path.exists();
    if had_target {
        // Step 1: 旧的挪到 .bak（这一步失败就没有破坏任何东西）
        fs::rename(path, &bak).map_err(io_err(path))?;
    }
    // Step 2: 新的就位。**这一步失败时旧内容还在 .bak 里，可恢复。**
    if let Err(e) = fs::rename(&tmp, path) {
        if had_target {
            // 回滚，让调用方看到的是一个仍然可用的旧文件
            let _ = fs::rename(&bak, path);
        }
        let _ = fs::remove_file(&tmp);
        return Err(io_err(path)(e));
    }
    let _ = fs::remove_file(&bak);
    Ok(())
}

/// 一个目标路径对应的三个派生路径**必须来自同一个函数**。
///
/// 否则"改了一处、忘了另一处"会留下孤儿文件，而孤儿文件的表现是
/// **磁盘上莫名多出东西**——很难归因到某次改名。
fn write_lock_path(path: &Path) -> PathBuf {
    with_ext(path, LOCK)
}

/// 读文件；若发现"替换进行到一半"（`.bak` 在而 target 不在），**先恢复再读**。
///
/// **恢复与读取必须在同一个锁内**（见 [`recover_locked`]）。
/// 否则读者可能在"target 已被挪到 `.bak`、新的还没就位"这个极短窗口里进来，
/// 看到的是**"文件不存在"**——那是一种比"读到半截"更隐蔽的失败：
/// 半截至少能被解析器报出来，而"不存在"会被上层当成**首次运行**，
/// 于是用户看到自己的设置被重置。
pub fn read_recovering(path: &Path) -> Result<Vec<u8>, IoError> {
    let _guard = FileLock::acquire(&write_lock_path(path))?;
    recover_locked(path)?;
    fs::read(path).map_err(io_err(path))
}

/// 读文件；**target 不存在时返回 `Ok(None)`** 而不是报错。
///
/// 与 [`read_recovering`] 的区别：这个入口把"不存在"当成**一种正常状态**
/// （首次运行就是这样），而那个入口把"不存在"当成错误。
///
/// 两个入口都要有，因为**两种语义都真实存在**：
/// 配置文件不存在 = 首次运行（正常）；而我们要读的那份已下载文件不存在 = 故障。
pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, IoError> {
    let _guard = FileLock::acquire(&write_lock_path(path))?;
    recover_locked(path)?;
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err(path)(e)),
    }
}

/// 从"替换进行到一半"的状态恢复（**自己加锁**）。
///
/// 判据很简单：
/// - `.bak` **在** 且 target **不在** → 上次崩在两步之间 → **把 `.bak` 放回 target**
/// - target **在** → 正常（顺手清掉残留的 `.bak` 与 `.tmp`）
///
/// **这个函数是幂等的**，可以在每次读取前调用——它的成本是两次 `exists`。
pub fn recover(path: &Path) -> Result<(), IoError> {
    let _guard = FileLock::acquire(&write_lock_path(path))?;
    recover_locked(path)
}

/// 与 [`recover`] 同一件事，但**要求调用方已经持有该路径的写锁**。
///
/// ## ⚠️ 为什么必须区分这两个入口（被并发测试抓出来的）
///
/// 第一版的 `recover` 不加锁，而 `read_recovering` 每次都调它。于是：
///
/// ```text
///     写入者: 创建 .tmp  ────────────────────────► rename(.tmp -> target)
///     读者:            recover() 把 .tmp 删了 ☠
/// ```
///
/// 写入者的 rename 随后失败，报 **`Os { code: 2, NotFound }`**。
///
/// **也就是说："恢复"和"写入"是同一份状态的两种操作，必须互斥。**
/// 把 `.tmp` 当成"垃圾"随手清掉，是把它当成了别人的东西——
/// 而它此刻可能正是**某人正在写的文件**。
///
/// 所以：`recover` 自己加锁（给外部读者用），`recover_locked` 假定锁已持有
/// （给 [`write_atomic`] 用，它已经在锁内，再加锁会**自死锁**）。
fn recover_locked(path: &Path) -> Result<(), IoError> {
    let bak = with_ext(path, BAK);
    let tmp = with_ext(path, TMP);

    if !path.exists() && bak.exists() {
        fs::rename(&bak, path).map_err(io_err(path))?;
        return Ok(());
    }
    // target 在 → 上次是完整的，清掉可能残留的中间产物
    let _ = fs::remove_file(&bak);
    let _ = fs::remove_file(&tmp);
    Ok(())
}

fn with_ext(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

/// **文件锁**：保证同一时刻只有一个写入者。
///
/// 用 `std::fs::File::lock`（Rust 1.89 起稳定，本机 1.98.1 实测可用）。
/// **锁的生命周期 = 这个值的生命周期**，drop 即释放——
/// 这样"忘记解锁"在类型上就不可能发生。
#[derive(Debug)]
pub struct FileLock {
    _file: File,
    path: PathBuf,
}

impl FileLock {
    /// 阻塞直到拿到锁。
    ///
    /// **默认是阻塞的**，因为我们的写入都很短（毫秒级），
    /// 而"立刻失败"会让调用方被迫自己写重试循环——那是重复且容易写错的。
    pub fn acquire(lock_path: &Path) -> Result<Self, IoError> {
        if let Some(dir) = lock_path.parent() {
            fs::create_dir_all(dir).map_err(io_err(dir))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
            .map_err(io_err(lock_path))?;
        file.lock().map_err(io_err(lock_path))?;
        Ok(Self {
            _file: file,
            path: lock_path.to_path_buf(),
        })
    }

    /// 不等待：拿不到就返回 `Locked`。
    ///
    /// **单实例检查必须用这个**——否则第二个实例会**一直等**第一个退出，
    /// 而用户看到的是"双击没反应"（他甚至不知道有个实例在跑）。
    pub fn try_acquire(lock_path: &Path) -> Result<Self, IoError> {
        if let Some(dir) = lock_path.parent() {
            fs::create_dir_all(dir).map_err(io_err(dir))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(lock_path)
            .map_err(io_err(lock_path))?;
        match file.try_lock() {
            Ok(()) => Ok(Self {
                _file: file,
                path: lock_path.to_path_buf(),
            }),
            Err(_) => Err(IoError::Locked {
                path: lock_path.to_path_buf(),
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// **单实例守卫**。
///
/// 与裸 `FileLock` 的区别：它带着**数据根**，因为"锁文件放哪儿"
/// 必须由数据根决定（放临时目录的话，两个用不同临时目录的进程
/// 会拿到两个不同的锁，而它们写的是同一份数据）。
#[derive(Debug)]
pub struct SingleInstance {
    _lock: FileLock,
    pub data_root: PathBuf,
}

impl SingleInstance {
    /// 锁文件固定叫 `qul.lock`，**放在数据根里**。
    pub fn acquire(data_root: &Path) -> Result<Self, IoError> {
        let lock_path = data_root.join("qul.lock");
        let lock = FileLock::try_acquire(&lock_path)?;
        Ok(Self {
            _lock: lock,
            data_root: data_root.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "qul-infra-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 边界检查挡住前缀相似的越界路径() {
        // 前缀匹配的经典漏洞：C:\data-evil 会被判成 C:\data 的子路径。
        let root = Path::new(r"C:\data");
        assert!(ensure_within(Path::new(r"C:\data\a.json"), root).is_ok());
        assert!(ensure_within(Path::new(r"C:\data\sub\a.json"), root).is_ok());
        assert!(ensure_within(Path::new(r"C:\data-evil\a.json"), root).is_err());
        assert!(ensure_within(Path::new(r"C:\other\a.json"), root).is_err());
        // 大小写与斜杠不该影响判断
        assert!(ensure_within(Path::new(r"c:/DATA/a.json"), root).is_ok());
    }

    #[test]
    fn 边界检查挡住目录穿越() {
        let root = Path::new(r"C:\data");
        assert!(ensure_within(Path::new(r"C:\data\..\..\Windows\System32\x.dll"), root).is_err());
    }

    #[test]
    fn 原子写产出完整内容() {
        let d = tmpdir("atomic");
        let f = d.join("a.json");
        write_atomic(&f, br#"{"v":1}"#, None).unwrap();
        assert_eq!(fs::read(&f).unwrap(), br#"{"v":1}"#);
        // 覆盖写
        write_atomic(&f, br#"{"v":2}"#, None).unwrap();
        assert_eq!(fs::read(&f).unwrap(), br#"{"v":2}"#);
        // 不留中间产物
        assert!(!with_ext(&f, BAK).exists(), "不该留下 .bak");
        assert!(!with_ext(&f, TMP).exists(), "不该留下 .tmp");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 崩在两步之间可恢复且内容完整() {
        let d = tmpdir("recover");
        let f = d.join("a.json");
        // 造出"替换进行到一半"的状态：.bak 在，target 不在
        fs::write(with_ext(&f, BAK), br#"{"old":true}"#).unwrap();
        assert!(!f.exists());

        let got = read_recovering(&f).unwrap();
        assert_eq!(got, br#"{"old":true}"#, "必须恢复出完整的旧内容");
        assert!(f.exists(), "恢复后 target 应当就位");
        assert!(!with_ext(&f, BAK).exists(), "恢复后不该留下 .bak");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 残留的_tmp_会被清掉() {
        let d = tmpdir("stale");
        let f = d.join("a.json");
        fs::write(&f, b"good").unwrap();
        fs::write(with_ext(&f, TMP), b"half-writ").unwrap();
        recover(&f).unwrap();
        assert!(!with_ext(&f, TMP).exists());
        assert_eq!(fs::read(&f).unwrap(), b"good", "target 不该被动过");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 恢复是幂等的() {
        let d = tmpdir("idem");
        let f = d.join("a.json");
        write_atomic(&f, b"x", None).unwrap();
        for _ in 0..5 {
            recover(&f).unwrap();
        }
        assert_eq!(fs::read(&f).unwrap(), b"x");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 单实例第二次拿不到锁() {
        let d = tmpdir("single");
        let a = SingleInstance::acquire(&d).expect("第一次应当成功");
        let b = SingleInstance::acquire(&d);
        assert!(b.is_err(), "第二个实例必须拿不到锁");
        match b {
            Err(IoError::Locked { .. }) => {}
            other => panic!("应当是 Locked，实际 {other:?}"),
        }
        drop(a);
        // 释放后应当能再拿
        let c = SingleInstance::acquire(&d).expect("释放后应当能拿到");
        drop(c);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 原子写会建立缺失的父目录() {
        let d = tmpdir("mkdir");
        let f = d.join("a").join("b").join("c.json");
        write_atomic(&f, b"deep", None).unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"deep");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 原子写拒绝越界路径() {
        let d = tmpdir("guard");
        let root = d.join("data");
        fs::create_dir_all(&root).unwrap();
        let outside = d.join("outside.json");
        let e = write_atomic(&outside, b"x", Some(&root));
        assert!(matches!(e, Err(IoError::EscapesRoot { .. })));
        assert!(!outside.exists(), "越界写必须没有产生任何文件");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn read_optional_把不存在当成正常() {
        // 首次运行时配置文件不存在，这是正常状态，不是故障。
        // 把它报成错误会让上层走进"错误处理"路径，而正确路径是"用默认值"。
        let d = tmpdir("optional");
        let f = d.join("cfg.json");
        assert_eq!(
            read_optional(&f).unwrap(),
            None,
            "不存在应当是 None 而不是 Err"
        );

        write_atomic(&f, br#"{"a":1}"#, None).unwrap();
        assert_eq!(read_optional(&f).unwrap().unwrap(), br#"{"a":1}"#);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn read_optional_也会从半途状态恢复() {
        let d = tmpdir("optional-recover");
        let f = d.join("cfg.json");
        // 造出"target 已被挪走、新的还没就位"的状态
        fs::write(with_ext(&f, BAK), br#"{"old":1}"#).unwrap();
        assert_eq!(
            read_optional(&f).unwrap().unwrap(),
            br#"{"old":1}"#,
            "必须恢复出旧内容，而不是报\"不存在\" —— 报不存在会被上层当成首次运行，用户看到设置被重置"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn 裸_recover_不会自我死锁() {
        // `recover` 自己取锁；`write_atomic` 在锁内调的是 `recover_locked`。
        // 如果哪次改错了（锁内又调 recover），这里会**挂住**而不是报错 ——
        // 所以这条测试的意义是"它跑得完"。
        let d = tmpdir("recover-nolock");
        let f = d.join("a.json");
        write_atomic(&f, b"v", None).unwrap();
        recover(&f).unwrap();
        write_atomic(&f, b"v2", None).unwrap();
        assert_eq!(fs::read(&f).unwrap(), b"v2");
        let _ = fs::remove_dir_all(&d);
    }
}
