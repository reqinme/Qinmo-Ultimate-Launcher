//! # ZIP 的落盘解压（M1 · 方案 §5.8 第 6 条与 §8 第 6 项）
//!
//! ## 本层只做两件事，而两件都是"只有碰文件系统才能做"的
//!
//! | 事 | 为什么不能在 `qul-core` |
//! |---|---|
//! | **路径安全判定** | 需要 `Path` 语义（盘符、`\` 与 `/` 的等价、`..` 消解） |
//! | **原子落盘** | 需要文件系统 |
//!
//! 结构解析与 inflate 在 `qul_core::{zip, inflate}`（纯函数，可穷举测试）。
//!
//! ## 三个安全关注点，各自对应一类真实攻击
//!
//! | 关注点 | 攻击 | 我们的处置 |
//! |---|---|---|
//! | **zip-slip**（`../../evil`） | 写实例目录之外 | `RelPath` 拒绝 + `ensure_within` **二次**校验 |
//! | **绝对路径条目** | 直接写 `C:\Windows\...` | `RelPath` 拒绝盘符与前导分隔符 |
//! | **符号链接条目** | 解出链接，**后续写入跟着它走出去** | `Entry::is_symlink` ⇒ **拒绝整条** |
//!
//! **"两层防护"不是冗余**：`RelPath` 是**字符串级**（拒绝已知的坏形态），
//! `ensure_within` 是**路径级**（比较规范化后的前缀）。
//! 两者的失败模式不同 —— 而"两种不同的检查都通过"才是我们要的保证。
//! 这与 ADR-0013 里那条"检测与匹配必须用不同思路"同源。
//!
//! ## 另外三类被拒绝的形态（都是真实存在的）
//!
//! | 形态 | 为什么危险 |
//! |---|---|
//! | **Windows 保留设备名**（`CON`/`NUL`/`COM1`…） | 写 `NUL` 会**静默丢弃**内容（它是个设备），而写 `CON` 在某些情形下**卡住进程** |
//! | **尾随空格或点**（`foo.` / `foo `） | Windows 会**默默去掉它们**，于是两个不同条目**落到同一个文件** |
//! | **NTFS 交替数据流**（名字含 `:`） | `file.txt:evil` 会写成一个**隐藏的流**，而常规列目录看不到它 |
//!
//! ## 原子性：**先写临时名，再改名**
//!
//! 方案 §5.8 第 6 条要求路径防护"沿用解压侧同一套策略"，
//! 而 I1（未校验的字节不以正式名存在）在这里的对应物是：
//! **每条条目先解到临时名、CRC 通过后才改名**。
//!
//! 于是"解压到一半崩了"留下的是**若干 `.part` 临时文件**，
//! 而不是一批**长度对但内容错**的正式文件。

use qul_core::layout::RelPath;
use qul_core::zip::{entry_data_range, parse_central_directory, Crc32, Entry, ParseError};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// 临时名后缀。**与下载引擎的 `.download` 同一个道理**：
/// 未通过校验的字节不以正式名存在。
pub const PART_SUFFIX: &str = ".part";

/// 一次解压的配置。
#[derive(Debug, Clone)]
pub struct ExtractConfig {
    /// **解出数据的总上限**（防 zip 炸弹）。默认 2 GB。
    pub max_total_bytes: u64,
    /// **单条条目的上限**（防单条膨胀）。默认 512 MB。
    pub max_entry_bytes: u64,
    /// 允许的最大条目数（防"几百万个空文件"把 inode 吃光）
    pub max_entries: usize,
    /// 是否**要求每个条目都有 CRC 并校验它**
    ///
    /// 默认 `true`。设成 `false` 只用于"解一条我们明知没有 CRC 的流"这种场景 ——
    /// 而那种场景在 Minecraft 里不存在。
    pub verify_crc: bool,
    /// **要跳过的路径前缀**（来自元数据的 `extract.exclude`）。
    ///
    /// ## ⚠️ 这个字段是补上的，而补它的理由是一个真缺口
    ///
    /// M2 的实测确认：旧式 natives 条目带 `extract: {"exclude": ["META-INF/"]}`，
    /// 而**新式（1.19.3 起）连这个字段都没有了**。
    ///
    /// 我在写 `install.rs` 时才发现在这里**从来没实现过排除** ——
    /// 也就是说在它之前，`META-INF/` 会被原样解出来。
    ///
    /// **`META-INF/` 正是签名相关文件所在处。** 把它解进实例不是什么灾难，
    /// 但它是"**我们照元数据说的做了**"与"我们忽略了元数据"的分界 ——
    /// 而一条被忽略的元数据字段，是那种会在别的场景下变成真问题的东西。
    ///
    /// ## 为什么是前缀而不是通配
    ///
    /// 实测里的形态是目录前缀（`META-INF/`），而不是通配符。
    /// 支持通配会引入一套自己的匹配语义，**而元数据里没有用到它**。
    pub excluded_prefixes: Vec<String>,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        Self {
            max_total_bytes: 2 * 1024 * 1024 * 1024,
            max_entry_bytes: 512 * 1024 * 1024,
            max_entries: 200_000,
            verify_crc: true,
            // **默认排除 `META-INF/`。**
            //
            // 它同时是「旧式元数据会显式说的那一项」与「新式元数据不再说、
            // 但我们仍然该排除的那一项」—— 因为 `META-INF/` 在**任何** jar 里
            // 都是签名与清单的元数据，而不是 natives 的运行期内容。
            excluded_prefixes: vec!["META-INF/".to_string()],
        }
    }
}

/// 一次解压的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractOutcome {
    /// 写出的文件数（**不含目录**）
    pub files: usize,
    /// 创建的目录数
    pub dirs: usize,
    /// 解出的总字节
    pub bytes: u64,
    /// 被跳过的条目及原因（**如实记录，不静默忽略**）
    pub skipped: Vec<(String, String)>,
}

/// 一条条目被拒绝的原因（**可被穷举**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryReject {
    /// 路径不合规（绝对、`..`、空、NUL）
    BadPath { name: String, why: String },
    /// 路径规范化后落在根之外（二次校验失败）
    EscapesRoot { name: String },
    /// 符号链接条目
    Symlink { name: String },
    /// Windows 保留设备名
    ReservedName { name: String },
    /// 名字以空格或点结尾（Windows 会默默去掉它们 → 两个条目落到同一个文件）
    TrailingSpaceOrDot { name: String },
    /// 名字里含 `:` 但**不是**在盘符位置（NTFS 交替数据流）
    AlternateDataStream { name: String },
    /// 名字不是合法 UTF-8 **且**没置 UTF-8 标志位
    ///
    /// ⚠️ 这一条是**保守的**：未置位时它可能是 CP437 或本地编码。
    /// 我们**不猜** —— 猜错会让文件名变成乱码，而乱码会被写进磁盘。
    /// 调用方若知道编码，应当先转换再用 `extract_with_names`。
    NonUtf8Name { raw_len: usize },
    /// 条目太大
    TooLarge { name: String, size: u64, limit: u64 },
    /// 总量超限
    TotalTooLarge { limit: u64 },
    /// 条目数超限
    TooManyEntries { limit: usize },
    /// 解出的字节数与声明的 `uncompressed_size` 不符
    SizeMismatch {
        name: String,
        declared: u64,
        actual: u64,
    },
    /// CRC 不匹配
    CrcMismatch {
        name: String,
        declared: u32,
        actual: u32,
    },
    /// inflate 失败
    InflateFailed { name: String, why: String },
}

impl std::fmt::Display for EntryReject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EntryReject::BadPath { name, why } => write!(f, "路径不合规（{name}）：{why}"),
            EntryReject::EscapesRoot { name } => write!(f, "路径规范化后落在目标目录之外：{name}"),
            EntryReject::Symlink { name } => write!(
                f,
                "符号链接条目 {name} —— 解出链接与解出文件在安全上完全不同（后续写入会跟着它走出去）"
            ),
            EntryReject::ReservedName { name } => {
                write!(f, "Windows 保留设备名：{name}（写它会静默丢弃内容或卡住进程）")
            }
            EntryReject::TrailingSpaceOrDot { name } => write!(
                f,
                "名字以空格或点结尾：{name}（Windows 会默默去掉它们 → 两个条目落到同一个文件）"
            ),
            EntryReject::AlternateDataStream { name } => write!(
                f,
                "名字含 `:` 而不在盘符位置：{name}（NTFS 交替数据流，常规列目录看不到它）"
            ),
            EntryReject::NonUtf8Name { raw_len } => write!(
                f,
                "名字不是合法 UTF-8（{raw_len} 字节）且未置 UTF-8 标志位 —— \
                 我们不做有损猜测（猜错会把乱码写进磁盘）"
            ),
            EntryReject::TooLarge { name, size, limit } => {
                write!(f, "条目 {name} 声称 {size} 字节，超过单条上限 {limit}")
            }
            EntryReject::TotalTooLarge { limit } => {
                write!(f, "解出总量超过上限 {limit} 字节（可能是构造出来的膨胀包）")
            }
            EntryReject::TooManyEntries { limit } => {
                write!(f, "条目数超过上限 {limit}")
            }
            EntryReject::SizeMismatch {
                name,
                declared,
                actual,
            } => write!(
                f,
                "条目 {name} 解出 {actual} 字节，与声明的 {declared} 不符"
            ),
            EntryReject::CrcMismatch {
                name,
                declared,
                actual,
            } => write!(
                f,
                "条目 {name} 的 CRC 不匹配（声明 {declared:08x}、实际 {actual:08x}）"
            ),
            EntryReject::InflateFailed { name, why } => {
                write!(f, "条目 {name} 解压失败：{why}")
            }
        }
    }
}

impl std::error::Error for EntryReject {}

/// 整体的解压失败（**与"某条被跳过"不同**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractError {
    /// zip 本身解析不了
    Parse(ParseError),
    /// 目标目录建不出来
    Dest { why: String },
    /// **条目数超限**（在开始解之前就拒绝，而不是解到一半才发现）
    TooManyEntries { limit: usize },
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::Parse(e) => write!(f, "zip 解析失败：{e}"),
            ExtractError::Dest { why } => write!(f, "目标目录不可用：{why}"),
            ExtractError::TooManyEntries { limit } => {
                write!(f, "条目数超过上限 {limit}")
            }
        }
    }
}

impl std::error::Error for ExtractError {}

/// Windows 保留设备名（**大小写不敏感**，且**带扩展名的形式也算** ——
/// `NUL.txt` 在 Windows 上仍然是 NUL 设备）。
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// 一个名字段是不是 Windows 保留设备名。
fn is_reserved(seg: &str) -> bool {
    // 去掉扩展名（`NUL.txt` 仍是 NUL）
    let stem = seg.split('.').next().unwrap_or(seg);
    let lower = stem.to_ascii_lowercase();
    RESERVED.contains(&lower.as_str())
}

/// 把条目名规范化成一个**相对路径字符串**（统一用 `/`）。
///
/// ZIP 规范要求用 `/`，但现实中**有工具用 `\`** —— 所以两种都要当分隔符。
fn normalize_name(raw: &str) -> String {
    raw.replace('\\', "/")
}

/// **判定一条条目是否被拒绝。** 返回 `None` 表示可以解。
///
/// 它是**纯字符串判定**（不看文件系统），所以它的全部规则都能被穷举测试。
/// 而"落在根之外"那一层由 `ensure_within` 在路径构造之后做。
pub fn check_entry(e: &Entry) -> Option<EntryReject> {
    // 目录条目：只校验路径形态（不写文件）
    let name = match &e.name_utf8 {
        Some(s) => s.clone(),
        None => {
            // **不做有损猜测**：未置 UTF-8 标志位时名字可能是 CP437 或本地编码。
            // 只有"恰好也是合法 UTF-8"时才继续 —— 那不是猜测，那是事实。
            match std::str::from_utf8(&e.name_bytes) {
                Ok(s) => s.to_string(),
                Err(_) => {
                    return Some(EntryReject::NonUtf8Name {
                        raw_len: e.name_bytes.len(),
                    })
                }
            }
        }
    };
    let normalized = normalize_name(&name);

    // 符号链接：**任何路径形态都不放行**
    if e.is_symlink {
        return Some(EntryReject::Symlink { name });
    }

    // 路径形态（复用内核那一套 —— 方案 §5.8 第 6 条要求"沿用同一套策略"）
    if let Err(why) = RelPath::new(normalized.clone()) {
        return Some(EntryReject::BadPath {
            name,
            why: why.to_string(),
        });
    }

    for seg in normalized.split('/') {
        if seg.is_empty() {
            // 目录条目以 `/` 结尾会产生空段；那不算错，跳过
            continue;
        }
        if is_reserved(seg) {
            return Some(EntryReject::ReservedName { name });
        }
        // 尾随空格或点：Windows 会默默去掉 → 两个条目落到同一个文件
        if seg.ends_with(' ') || seg.ends_with('.') {
            return Some(EntryReject::TrailingSpaceOrDot { name });
        }
        // NTFS 交替数据流：`file.txt:evil`
        if seg.contains(':') {
            return Some(EntryReject::AlternateDataStream { name });
        }
    }
    None
}

/// **解一个 zip 到目标目录。**
///
/// 顺序刻意如此（每一步都有理由）：
///
/// ```text
///     ① 解析中央目录                    （纯计算）
///     ② 条目数闸门                      （在解任何东西之前就拒绝）
///     ③ 逐条：形态检查 → 路径构造 → ensure_within → 解压 → CRC → 原子改名
///     ④ 目录条目 create_dir_all
/// ```
///
/// **第 ② 步在解压之前**：「几百万个空文件」的包会在开始就被拒，
/// 而不是把 inode 吃到一半才失败。
pub fn extract(
    zip_bytes: &[u8],
    dest_root: &Path,
    cfg: &ExtractConfig,
    cancel: Option<&qul_core::retry::CancelToken>,
) -> Result<ExtractOutcome, ExtractError> {
    let entries = parse_central_directory(zip_bytes).map_err(ExtractError::Parse)?;
    if entries.len() > cfg.max_entries {
        return Err(ExtractError::TooManyEntries {
            limit: cfg.max_entries,
        });
    }
    std::fs::create_dir_all(dest_root).map_err(|e| ExtractError::Dest {
        why: e.kind().to_string(),
    })?;

    let mut out = ExtractOutcome {
        files: 0,
        dirs: 0,
        bytes: 0,
        skipped: Vec::new(),
    };
    // 用集合跟踪已写路径，用来发现"两个条目落到同一个文件"
    let mut seen: BTreeSet<String> = BTreeSet::new();

    for e in &entries {
        if let Some(c) = cancel {
            if c.is_cancelled() {
                break;
            }
        }
        let display = e.display_name();

        // 形态检查
        if let Some(rej) = check_entry(e) {
            out.skipped.push((display, rej.to_string()));
            continue;
        }

        // **元数据说的"不要解这些"** —— 见 `ExtractConfig::excluded_prefixes`。
        //
        // ⚠️ 它**记进 `skipped` 而不是静默丢掉**：跳过与"解不出来"是两件事，
        // 而一份"少了个文件但没有记录"的日志会让排查变成猜。
        {
            let name_norm = normalize_name(&display);
            if let Some(hit) = cfg
                .excluded_prefixes
                .iter()
                .find(|p| name_norm.starts_with(p.as_str()))
            {
                out.skipped.push((
                    display,
                    format!("按元数据的 extract.exclude 跳过（前缀 `{hit}`）"),
                ));
                continue;
            }
        }

        let name = match &e.name_utf8 {
            Some(s) => s.clone(),
            None => match std::str::from_utf8(&e.name_bytes) {
                Ok(s) => s.to_string(),
                Err(_) => {
                    out.skipped.push((
                        display,
                        EntryReject::NonUtf8Name {
                            raw_len: e.name_bytes.len(),
                        }
                        .to_string(),
                    ));
                    continue;
                }
            },
        };
        let normalized = normalize_name(&name);
        // 去掉目录条目尾部的 `/`，再拼到根上
        let rel = normalized.trim_end_matches('/');

        let target = dest_root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        // **二次校验（路径级）**：规范化后的前缀比较。
        // 与 `RelPath` 的字符串级检查是**两种不同的失败模式**。
        if let Err(why) = crate::fsx::ensure_within(&target, dest_root) {
            out.skipped.push((
                display,
                EntryReject::EscapesRoot {
                    name: why.to_string(),
                }
                .to_string(),
            ));
            continue;
        }
        let key = target.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            // **两个条目落到同一个文件**：这是一个值得知道的事实
            // （它可能是精心构造的，也可能只是打包工具的毛病）。
            out.skipped.push((
                display,
                "另一个条目已经写过同一个路径（后到的被跳过）".to_string(),
            ));
            continue;
        }

        // 目录条目：建目录即可
        if e.is_dir {
            if std::fs::create_dir_all(&target).is_ok() {
                out.dirs += 1;
            }
            continue;
        }

        // 大小闸门
        if e.uncompressed_size > cfg.max_entry_bytes {
            out.skipped.push((
                display,
                EntryReject::TooLarge {
                    name: rel.to_string(),
                    size: e.uncompressed_size,
                    limit: cfg.max_entry_bytes,
                }
                .to_string(),
            ));
            continue;
        }
        if out.bytes + e.uncompressed_size > cfg.max_total_bytes {
            out.skipped.push((
                display,
                EntryReject::TotalTooLarge {
                    limit: cfg.max_total_bytes,
                }
                .to_string(),
            ));
            continue;
        }

        // 取压缩数据
        let range = match entry_data_range(zip_bytes, e) {
            Ok(r) => r,
            Err(why) => {
                out.skipped.push((display.clone(), why.to_string()));
                continue;
            }
        };
        let raw = &zip_bytes[range.start..range.start + range.len];

        // 解压（inflate 的上限取"声明大小 + 1"，这样"多解出一个字节"也能被察觉）
        let limit = (e.uncompressed_size as usize).saturating_add(1);
        let data = match e.method {
            qul_core::zip::Method::Stored => raw.to_vec(),
            qul_core::zip::Method::Deflate => match qul_core::inflate::inflate(raw, limit) {
                Ok(d) => d,
                Err(why) => {
                    out.skipped.push((
                        display,
                        EntryReject::InflateFailed {
                            name: rel.to_string(),
                            why: why.to_string(),
                        }
                        .to_string(),
                    ));
                    continue;
                }
            },
        };

        // **长度必须与声明一致。**
        // 一个"照写不误"的实现会产出一个**长度不对但看似成功**的文件，
        // 而它随后被 CRC 拦住 —— 排查方向会被引向"数据坏了"。
        if data.len() as u64 != e.uncompressed_size {
            out.skipped.push((
                display,
                EntryReject::SizeMismatch {
                    name: rel.to_string(),
                    declared: e.uncompressed_size,
                    actual: data.len() as u64,
                }
                .to_string(),
            ));
            continue;
        }

        // CRC（**中央目录里的值是权威的**）
        if cfg.verify_crc {
            let mut c = Crc32::new();
            c.update(&data);
            let actual = c.finalize();
            if actual != e.crc32 {
                out.skipped.push((
                    display,
                    EntryReject::CrcMismatch {
                        name: rel.to_string(),
                        declared: e.crc32,
                        actual,
                    }
                    .to_string(),
                ));
                continue;
            }
        }

        // **原子落盘**：先写 `.part`，成功才改名。
        // 于是"解压到一半崩了"留下的是若干 `.part`，而不是一批错内容。
        if let Some(dir) = target.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                out.skipped
                    .push((display, format!("建目录失败：{}", dir.display())));
                continue;
            }
        }
        let part = part_path(&target);
        if let Err(why) = std::fs::write(&part, &data) {
            out.skipped
                .push((display, format!("写临时文件失败：{}", why.kind())));
            continue;
        }
        if target.exists() {
            let _ = std::fs::remove_file(&target);
        }
        if let Err(why) = std::fs::rename(&part, &target) {
            let _ = std::fs::remove_file(&part);
            out.skipped
                .push((display, format!("改名失败：{}", why.kind())));
            continue;
        }
        out.files += 1;
        out.bytes += data.len() as u64;
    }

    Ok(out)
}

/// 临时路径（`<target>.part`）。
pub fn part_path(target: &Path) -> PathBuf {
    let mut s = target.as_os_str().to_os_string();
    s.push(PART_SUFFIX);
    PathBuf::from(s)
}

/// **清掉目标目录下残留的 `.part` 文件**（上次解压崩在中间留下的）。
///
/// 返回清掉的个数。它是幂等的，所以可以在每次解压前调。
pub fn sweep_parts(dir: &Path) -> usize {
    let mut n = 0usize;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            n += sweep_parts(&p);
        } else if p
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.ends_with(PART_SUFFIX))
            && std::fs::remove_file(&p).is_ok()
        {
            n += 1;
        }
    }
    n
}

/// **natives 的解压去处的建议。**
///
/// 方案 §8 第 6 项的原话：
///
/// > natives | 按平台解压到**实例内** natives 目录（不落系统 `%temp%`）
///
/// 而它后面的说明更值得记：
///
/// > 教程明确警告官方启动器把 natives 解压到 `%temp%`，
/// > **可能被垃圾清理软件删除导致无法启动**。
///
/// 也就是说：**这不是"我们更喜欢这样"，而是"那样会坏"**。
/// 所以这个函数存在的意义是让"natives 去哪"有一个**唯一的、可被引用的答案**，
/// 而不是散在各处的字符串拼接。
pub fn natives_dir(instance_dir: &Path, natives_key: &str) -> PathBuf {
    instance_dir.join("natives").join(natives_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qul_core::zip::{crc32, SIG_CENTRAL, SIG_EOCD, SIG_LOCAL};

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "qul-zx-{tag}-{}-{}",
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

    fn attrs_file() -> u32 {
        (0o100_644u32) << 16
    }
    fn attrs_symlink() -> u32 {
        (0o120_777u32) << 16
    }

    /// 造一个 **stored（不压缩）** 的真实 zip。
    fn build_zip(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, content, ext) in entries {
            let nb = name.as_bytes();
            let off = out.len() as u32;
            let crc = crc32(content);
            out.extend_from_slice(&SIG_LOCAL.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0x0800u16.to_le_bytes()); // **置 UTF-8 标志位**
            out.extend_from_slice(&0u16.to_le_bytes()); // stored
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(content.len() as u32).to_le_bytes());
            out.extend_from_slice(&(content.len() as u32).to_le_bytes());
            out.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(nb);
            out.extend_from_slice(content);

            central.extend_from_slice(&SIG_CENTRAL.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0x0800u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(nb.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&ext.to_le_bytes());
            central.extend_from_slice(&off.to_le_bytes());
            central.extend_from_slice(nb);
        }
        let cd_off = out.len() as u32;
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&SIG_EOCD.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    // ───────────────── 正常解压 ─────────────────

    #[test]
    fn 解出文件且内容正确() {
        let z = build_zip(&[
            ("a.txt", b"hello", attrs_file()),
            ("dir/b.txt", b"world", attrs_file()),
        ]);
        let d = tmpdir("ok");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert_eq!(out.files, 2);
        assert_eq!(out.bytes, 10);
        assert!(out.skipped.is_empty(), "{:?}", out.skipped);
        assert_eq!(std::fs::read(d.join("a.txt")).unwrap(), b"hello");
        assert_eq!(
            std::fs::read(d.join("dir").join("b.txt")).unwrap(),
            b"world"
        );
        // **不留 .part**
        assert_eq!(sweep_parts(&d), 0, "解压完不该留下临时文件");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 目录条目会建出目录() {
        let z = build_zip(&[
            ("dir/", b"", attrs_file()),
            ("dir/f.txt", b"x", attrs_file()),
        ]);
        let d = tmpdir("dirs");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert!(d.join("dir").is_dir());
        assert_eq!(std::fs::read(d.join("dir").join("f.txt")).unwrap(), b"x");
        assert_eq!(out.files, 1, "目录条目不算文件");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 反斜杠分隔的名字也能解() {
        // ZIP 规范要求 `/`，但**现实中确有工具用 `\`**。
        let z = build_zip(&[("a\\b.txt", b"x", attrs_file())]);
        let d = tmpdir("backslash");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert_eq!(out.files, 1, "{:?}", out.skipped);
        assert!(d.join("a").join("b.txt").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── zip-slip（最重要的一类）─────────────────

    #[test]
    fn 父段穿越被拒绝且没有写出去() {
        // ⚠️ **这是解压最重要的一条安全断言。**
        let z = build_zip(&[
            ("ok.txt", b"fine", attrs_file()),
            ("../escaped.txt", b"evil", attrs_file()),
            ("a/../../escaped2.txt", b"evil", attrs_file()),
            ("..\\win-escaped.txt", b"evil", attrs_file()),
        ]);
        let d = tmpdir("slip");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();

        assert_eq!(out.files, 1, "只有 ok.txt 该被写出");
        assert_eq!(out.skipped.len(), 3, "三条穿越都该被拒：{:?}", out.skipped);
        // **关键：确实没有写到外面去**
        let parent = d.parent().unwrap();
        assert!(
            !parent.join("escaped.txt").exists(),
            "**绝不能写到目标目录之外**"
        );
        assert!(!parent.join("escaped2.txt").exists());
        assert!(!parent.join("win-escaped.txt").exists());
        for (_, why) in &out.skipped {
            assert!(why.contains("路径"), "拒绝原因要说清是路径问题：{why}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 绝对路径条目被拒绝() {
        let z = build_zip(&[
            ("/abs.txt", b"evil", attrs_file()),
            ("\\abs2.txt", b"evil", attrs_file()),
            ("C:/drive.txt", b"evil", attrs_file()),
        ]);
        let d = tmpdir("abs");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert_eq!(out.files, 0, "{:?}", out.skipped);
        assert_eq!(out.skipped.len(), 3);
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── 符号链接 ─────────────────

    #[test]
    fn 符号链接条目被拒绝() {
        // **一个 zip 里可以放一个指向任意位置的链接**，
        // 而"解出链接"与"解出文件"在安全上完全不同 —— 后续写入会跟着它走出去。
        let z = build_zip(&[
            ("regular.txt", b"ok", attrs_file()),
            ("link", b"../../etc/passwd", attrs_symlink()),
        ]);
        let d = tmpdir("symlink");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert_eq!(out.files, 1);
        assert_eq!(out.skipped.len(), 1);
        assert!(
            out.skipped[0].1.contains("符号链接"),
            "{}",
            out.skipped[0].1
        );
        assert!(!d.join("link").exists(), "绝不能解出链接");
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── 三类"真实但容易被忽略"的形态 ─────────────────

    #[test]
    fn windows_保留设备名被拒绝() {
        // 写 `NUL` 会**静默丢弃**内容（它是个设备），
        // 而写 `CON` 在某些情形下会**卡住进程**。
        let z = build_zip(&[
            ("NUL", b"x", attrs_file()),
            ("con.txt", b"x", attrs_file()),
            ("COM1", b"x", attrs_file()),
            ("normal.txt", b"ok", attrs_file()),
        ]);
        let d = tmpdir("reserved");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert_eq!(out.files, 1, "{:?}", out.skipped);
        assert_eq!(out.skipped.len(), 3);
        for (_, why) in &out.skipped {
            assert!(why.contains("保留设备名"), "{why}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 尾随空格或点被拒绝() {
        // Windows 会**默默去掉它们** → 两个不同条目**落到同一个文件**。
        let z = build_zip(&[
            ("foo.", b"1", attrs_file()),
            ("bar ", b"2", attrs_file()),
            ("ok.txt", b"3", attrs_file()),
        ]);
        let d = tmpdir("trailing");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert_eq!(out.files, 1);
        assert_eq!(out.skipped.len(), 2);
        for (_, why) in &out.skipped {
            assert!(why.contains("空格或点"), "{why}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ntfs_交替数据流被拒绝() {
        // `file.txt:evil` 会写成一个**隐藏的流**，常规列目录看不到它。
        let z = build_zip(&[
            ("file.txt:evil", b"hidden", attrs_file()),
            ("ok.txt", b"1", attrs_file()),
        ]);
        let d = tmpdir("ads");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert_eq!(out.files, 1, "{:?}", out.skipped);
        assert!(
            out.skipped[0].1.contains("交替数据流"),
            "{}",
            out.skipped[0].1
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 两个条目落到同一个路径时后者被跳过并记录() {
        // 这可能只是打包工具的毛病，也可能是精心构造的
        // （"先放一个正常的、再放一个恶意的覆盖它"）。
        let z = build_zip(&[
            ("same.txt", b"first", attrs_file()),
            ("same.txt", b"second", attrs_file()),
        ]);
        let d = tmpdir("dup");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert_eq!(out.files, 1);
        assert_eq!(out.skipped.len(), 1);
        assert!(
            out.skipped[0].1.contains("同一个路径"),
            "{}",
            out.skipped[0].1
        );
        // 第一个赢（后到的被跳过）
        assert_eq!(std::fs::read(d.join("same.txt")).unwrap(), b"first");
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── CRC 与长度 ─────────────────

    #[test]
    fn crc_不匹配时该条目被跳过且不留部分文件() {
        // 造一个"内容与 CRC 不符"的 zip：把内容改了但不改 CRC。
        let mut z = build_zip(&[("bad.txt", b"hello", attrs_file())]);
        // 本地头之后是内容；找到它并改一个字节
        let pos = z
            .windows(5)
            .position(|w| w == b"hello")
            .expect("应当能找到内容");
        z[pos] = b'H';
        let d = tmpdir("crc");
        let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
        assert_eq!(out.files, 0, "CRC 不匹配不该写出文件：{:?}", out.skipped);
        assert!(out.skipped[0].1.contains("CRC"), "{}", out.skipped[0].1);
        assert!(!d.join("bad.txt").exists());
        // **没有 .part 残留**（原子性）
        assert_eq!(sweep_parts(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 关闭_crc_校验时该条目会被写出() {
        // 这条测试的意义：证明 `verify_crc` 这个开关**真的在起作用** ——
        // 否则上面那条测试可能只是因为别的原因而通过。
        let mut z = build_zip(&[("bad.txt", b"hello", attrs_file())]);
        let pos = z.windows(5).position(|w| w == b"hello").unwrap();
        z[pos] = b'H';
        let d = tmpdir("nocrc");
        let cfg = ExtractConfig {
            verify_crc: false,
            ..Default::default()
        };
        let out = extract(&z, &d, &cfg, None).unwrap();
        assert_eq!(out.files, 1, "{:?}", out.skipped);
        assert_eq!(std::fs::read(d.join("bad.txt")).unwrap(), b"Hello");
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── 闸门 ─────────────────

    #[test]
    fn 条目数超限在解压之前就拒绝() {
        // 「几百万个空文件」的包会在**开始就被拒**，
        // 而不是把 inode 吃到一半才失败。
        let mut entries: Vec<(String, Vec<u8>, u32)> = Vec::new();
        for i in 0..20 {
            entries.push((format!("f{i}.txt"), b"x".to_vec(), attrs_file()));
        }
        let refs: Vec<(&str, &[u8], u32)> = entries
            .iter()
            .map(|(n, c, a)| (n.as_str(), c.as_slice(), *a))
            .collect();
        let z = build_zip(&refs);
        let d = tmpdir("count");
        let cfg = ExtractConfig {
            max_entries: 5,
            ..Default::default()
        };
        match extract(&z, &d, &cfg, None) {
            Err(ExtractError::TooManyEntries { limit }) => assert_eq!(limit, 5),
            other => panic!("应当在解压前拒绝，实际 {other:?}"),
        }
        // **一个文件都没写**
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 单条超限被跳过() {
        let big = vec![0u8; 4096];
        let z = build_zip(&[
            ("big.bin", &big, attrs_file()),
            ("small.txt", b"x", attrs_file()),
        ]);
        let d = tmpdir("entrysize");
        let cfg = ExtractConfig {
            max_entry_bytes: 1024,
            ..Default::default()
        };
        let out = extract(&z, &d, &cfg, None).unwrap();
        assert_eq!(out.files, 1, "小的该被写出");
        assert_eq!(out.skipped.len(), 1);
        assert!(
            out.skipped[0].1.contains("超过单条上限"),
            "{}",
            out.skipped[0].1
        );
        assert!(!d.join("big.bin").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 总量超限被跳过() {
        let chunk = vec![0u8; 1024];
        let z = build_zip(&[
            ("a.bin", &chunk, attrs_file()),
            ("b.bin", &chunk, attrs_file()),
            ("c.bin", &chunk, attrs_file()),
        ]);
        let d = tmpdir("totalsize");
        let cfg = ExtractConfig {
            max_total_bytes: 2048, // 只够两个
            ..Default::default()
        };
        let out = extract(&z, &d, &cfg, None).unwrap();
        assert_eq!(out.files, 2, "{:?}", out.skipped);
        assert_eq!(out.skipped.len(), 1);
        assert!(
            out.skipped[0].1.contains("总量超过上限"),
            "{}",
            out.skipped[0].1
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 不是_zip_的输入返回解析错误() {
        let d = tmpdir("notzip");
        match extract(&[0x41u8; 100], &d, &ExtractConfig::default(), None) {
            Err(ExtractError::Parse(_)) => {}
            other => panic!("应当报解析错误，实际 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── 取消与清理 ─────────────────

    #[test]
    fn 已取消时一个文件都不写() {
        let mut entries: Vec<(String, Vec<u8>, u32)> = Vec::new();
        for i in 0..10 {
            entries.push((format!("f{i}.txt"), b"x".to_vec(), attrs_file()));
        }
        let refs: Vec<(&str, &[u8], u32)> = entries
            .iter()
            .map(|(n, c, a)| (n.as_str(), c.as_slice(), *a))
            .collect();
        let z = build_zip(&refs);
        let d = tmpdir("cancel");
        let c = qul_core::retry::CancelToken::new();
        c.cancel();
        let out = extract(&z, &d, &ExtractConfig::default(), Some(&c)).unwrap();
        assert_eq!(out.files, 0);
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 清理能删掉残留的_part_文件() {
        // "解压到一半崩了"会留下 `.part`；这个函数是那个情形的事后清理。
        let d = tmpdir("sweep");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.txt.part"), b"x").unwrap();
        std::fs::write(d.join("sub").join("b.part"), b"x").unwrap();
        std::fs::write(d.join("good.txt"), b"keep").unwrap();
        let n = sweep_parts(&d);
        assert_eq!(n, 2, "递归清理");
        assert!(d.join("good.txt").is_file(), "正常文件不该被动");
        // 幂等
        assert_eq!(sweep_parts(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── natives 的去处 ─────────────────

    #[test]
    fn natives_落在实例内而不是_temp() {
        // 方案 §8 第 6 项的落点。官方启动器把 natives 解到 `%temp%`，
        // 而**教程明确警告那可能被垃圾清理软件删除导致无法启动**。
        let inst = Path::new(r"C:\data\instances\abc");
        let n = natives_dir(inst, "natives-1.21");
        assert!(n.starts_with(inst), "**必须在实例内**：{}", n.display());
        let s = n.to_string_lossy().to_lowercase();
        assert!(!s.contains("temp"), "不该落在 %temp% 下：{s}");
        assert!(s.contains("natives"), "{s}");
    }

    // ───────────────── 纯字符串判定的穷举 ─────────────────

    #[test]
    fn 保留名判定覆盖全部设备名且大小写不敏感() {
        for name in RESERVED {
            assert!(is_reserved(name), "{name}");
            assert!(is_reserved(&name.to_uppercase()), "{name} 大写也该认");
            assert!(is_reserved(&format!("{name}.txt")), "{name}.txt 仍指向设备");
        }
        // 正常名字不该被误判
        for ok in [
            "console",
            "null",
            "com0",
            "com10",
            "lpt0",
            "auxiliary",
            "file.txt",
        ] {
            assert!(!is_reserved(ok), "{ok} 不该被判为保留名");
        }
    }
}
