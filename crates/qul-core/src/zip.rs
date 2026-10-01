//! # ZIP 的结构解析（**纯规则，零 IO**）
//!
//! ## 为什么自己实现而不引 `zip` crate
//!
//! 三条理由，与自写 SHA-1（`qul-infra::check`）完全同源：
//!
//! 1. **尖刺阶段每多一条依赖就多一条要审的许可**；
//! 2. **ZIP 是规格封闭的格式**（PKWARE APPNOTE）—— 正确性可以被
//!    **真实 zip 文件**与**构造的边界样本**钉死，没有"靠实现者猜"的空间；
//! 3. **解压是"恶意 zip 写任意文件"的攻击面。** 把它掌握在自己手里意味着
//!    **我们能用测试穷举它的拒绝行为**，而不是信任一个第三方实现的默认策略。
//!
//! ## ⚠️ 本模块只做"解析"与"解码"，**不做"落盘"**
//!
//! | 层 | 做什么 | 为什么分开 |
//! |---|---|---|
//! | **本模块（`qul-core`）** | 解析中央目录、inflate 解码 | **纯函数 ⇒ 可穷举测试** |
//! | `qul-infra::zip` | 落盘、路径防护、原子性 | 需要文件系统 |
//!
//! **尤其"路径防护"不在本模块** —— 它需要 `Path` 语义（Windows 盘符、
//! `\` 与 `/` 的等价、`..` 的消解），而那些是 IO 侧的事。
//! 本模块只保证**取出的路径字符串是原始的、没被歧义的**。
//!
//! ## 四条必须知道的 ZIP 事实（每一条都是一个坑）
//!
//! 1. **文件名编码不一定是 UTF-8。** 通用标志位 bit 11 表示"文件名是 UTF-8"。
//!    未置位时它通常是 CP437 或本地编码 —— 而我们的 zip 来自中文环境，
//!    所以"未置位就按 UTF-8 解码"是常见的错法。本模块的做法：
//!    **置位则严格 UTF-8；未置位则原样保留字节**，由 IO 侧决定怎么处理。
//! 2. **`..` 可以出现在文件名里，而且合法的 zip 里也能出现。**
//!    所以"拒绝 `..`"必须在**路径解析之后**做，不能在字符串上做 ——
//!    `a/../b` 是无害的，而 `..\..\evil` 在字符串上与 `..` 长得不一样。
//! 3. **长度可以放在数据之后**（数据描述符模式，flag bit 3）。本模块**明确拒绝**
//!    这种条目 —— 支持它需要"流式猜测边界"，而那正是 zip 解析里 bug 最多的地方。
//!    **拒绝比猜好**：Minecraft 的官方 zip 都用中央目录里的长度。
//! 4. **符号链接条目必须被拒绝。** 一个 zip 里可以放一个指向任意位置的链接，
//!    而"解压出一个链接"与"解压出一个文件"在安全上完全不同 ——
//!    后续写入会**跟着链接走到外面**。所以 `Entry` 有一个 `is_symlink` 字段。
//! 5. **CRC-32 在中央目录里是权威的**，而**本地头里的可能与它不一致**
//!    （APPNOTE 允许）。我们**只信中央目录**。

use serde::{Deserialize, Serialize};

/// ZIP 的四种签名（PKWARE APPNOTE §4.3）。
pub const SIG_LOCAL: u32 = 0x0403_4b50;
pub const SIG_CENTRAL: u32 = 0x0201_4b50;
pub const SIG_EOCD: u32 = 0x0605_4b50;
/// ZIP64 的 EOCD 定位器（我们不支持 zip64）
pub const SIG_ZIP64_LOCATOR: u32 = 0x0706_4b50;

/// 压缩方法。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    /// 不压缩（`0`）
    #[serde(rename = "stored")]
    Stored,
    /// deflate（`8`）—— **Minecraft 的 zip 几乎全是它**
    #[serde(rename = "deflate")]
    Deflate,
}

impl Method {
    pub const fn from_u16(v: u16) -> Option<Self> {
        match v {
            0 => Some(Method::Stored),
            8 => Some(Method::Deflate),
            _ => None,
        }
    }
    pub const fn as_u16(self) -> u16 {
        match self {
            Method::Stored => 0,
            Method::Deflate => 8,
        }
    }
    pub const fn key(self) -> &'static str {
        match self {
            Method::Stored => "stored",
            Method::Deflate => "deflate",
        }
    }
}

/// 一条目录项（**以中央目录为准**）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// 原始文件名字节。
    ///
    /// **为什么保留字节而不是直接给 `String`**：见模块文档第 1 条 ——
    /// 未置 UTF-8 标志位时它可能是 CP437 或本地编码，而我们**不做有损猜测**。
    pub name_bytes: Vec<u8>,
    /// 已解码的名字（**仅当 UTF-8 标志位置位且真的能解码时才有**）
    pub name_utf8: Option<String>,
    pub method: Method,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    /// 本地头的偏移（要拿它去读数据）
    pub local_header_offset: u64,
    /// 是不是目录（**以 `/` 结尾** —— 这是 ZIP 的约定，不是属性位）
    pub is_dir: bool,
    /// **是不是符号链接。** 详见模块文档第 4 条 —— 这类条目必须被拒绝。
    pub is_symlink: bool,
    /// CRC-32（来自中央目录，**权威值**）
    pub crc32: u32,
    /// 通用标志位（原样保留，便于诊断）
    pub flags: u16,
}

impl Entry {
    /// 给日志/错误信息用的名字（**有损转换，只用于展示**）。
    pub fn display_name(&self) -> String {
        match &self.name_utf8 {
            Some(s) => s.clone(),
            None => String::from_utf8_lossy(&self.name_bytes).into_owned(),
        }
    }

    /// 名字里有没有 `..` 段。
    ///
    /// ⚠️ **它只是一个"值得注意"的信号，不是安全判定。**
    /// 安全判定必须在**路径解析之后**做（模块文档第 2 条）。
    /// 有这条方法是为了**让"这条 zip 里含 `..`"成为一个可被断言的事实**，
    /// 而不是一个散在代码里的正则。
    pub fn has_parent_segment(&self) -> bool {
        let raw = String::from_utf8_lossy(&self.name_bytes);
        raw.split(['/', '\\']).any(|s| s == "..")
    }

    /// 是不是**绝对路径**（前导分隔符或盘符）。
    ///
    /// 同样是"值得注意"的信号。它是 [`crate::zip`] 之外的快速早退路径，
    /// 而**真正的判定**仍在路径解析之后。
    pub fn is_absolute(&self) -> bool {
        let raw = String::from_utf8_lossy(&self.name_bytes);
        raw.starts_with('/')
            || raw.starts_with('\\')
            || (raw.len() >= 2 && raw.as_bytes()[1] == b':')
    }

    /// 从 Unix 外部属性高 16 位判定 `S_IFLNK`。
    pub fn mode_is_symlink(external_attrs: u32) -> bool {
        let mode = external_attrs >> 16;
        // `0o120000` = S_IFLNK
        (mode & 0o170_000) == 0o120_000
    }
}

/// 解析错误。**每一条都对应一个可被穷举的拒绝理由。**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    TooShort,
    NoEndOfCentralDirectory,
    CentralDirectoryOutOfRange,
    BadCentralSignature {
        at: u64,
    },
    EntryCountMismatch {
        declared: usize,
        found: usize,
    },
    UnsupportedMethod {
        method: u16,
        name: String,
    },
    /// **数据描述符模式**：长度在数据之后 —— 我们明确拒绝
    DataDescriptorUnsupported {
        name: String,
    },
    BadLocalHeader {
        at: u64,
    },
    /// 不支持分卷 zip
    MultiDiskUnsupported,
    /// 不支持 ZIP64（Minecraft 的 zip 远小于 4 GB）
    Zip64Unsupported,
    /// 加密条目
    EncryptedUnsupported {
        name: String,
    },
    /// 本地头声明的尺寸与中央目录不一致（`flags` 里没开数据描述符时）
    LocalSizeMismatch {
        name: String,
        local_comp: u64,
        central_comp: u64,
    },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::TooShort => write!(f, "文件太短，放不下中央目录结束记录"),
            ParseError::NoEndOfCentralDirectory => {
                write!(f, "找不到中央目录结束记录（可能不是 zip）")
            }
            ParseError::CentralDirectoryOutOfRange => write!(f, "中央目录的位置或长度超出文件"),
            ParseError::BadCentralSignature { at } => {
                write!(f, "偏移 {at} 处的中央目录签名不对")
            }
            ParseError::EntryCountMismatch { declared, found } => {
                write!(f, "条目数不符：声明 {declared}、实际 {found}")
            }
            ParseError::UnsupportedMethod { method, name } => {
                write!(f, "不支持的压缩方法 {method}（条目 {name}）")
            }
            ParseError::DataDescriptorUnsupported { name } => write!(
                f,
                "条目 {name} 用了数据描述符模式（长度在数据之后）—— \
                 我们明确拒绝它，因为支持它需要流式猜测边界"
            ),
            ParseError::BadLocalHeader { at } => write!(f, "偏移 {at} 处的本地头签名不对"),
            ParseError::MultiDiskUnsupported => write!(f, "不支持分卷 zip"),
            ParseError::Zip64Unsupported => write!(f, "不支持 ZIP64（本项目的 zip 远小于 4 GB）"),
            ParseError::EncryptedUnsupported { name } => {
                write!(f, "条目 {name} 是加密的（我们不做密码解压）")
            }
            ParseError::LocalSizeMismatch {
                name,
                local_comp,
                central_comp,
            } => write!(
                f,
                "条目 {name} 的本地头与中央目录尺寸不一致（{local_comp} vs {central_comp}）"
            ),
        }
    }
}

impl std::error::Error for ParseError {}

fn rd_u16(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at + 2)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn rd_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// **找到中央目录结束记录（EOCD）。**
///
/// 从文件尾部往前扫，因为 EOCD 之后还可以有注释（最多 65535 字节）。
///
/// ⚠️ 一个常见的错法是"从尾部找第一个 `PK\x05\x06`" —— 而**注释里可以出现
/// 那四个字节**。所以本函数在命中后**交叉验证**：
/// `i + 22 + comment_len == 文件长度`。只有自洽的那个才是真 EOCD。
fn find_eocd(b: &[u8]) -> Option<usize> {
    if b.len() < 22 {
        return None;
    }
    let max_back = 22 + 65_535;
    let start = b.len().saturating_sub(max_back);
    let mut i = b.len() - 22;
    loop {
        if rd_u32(b, i) == Some(SIG_EOCD) {
            if let Some(comment_len) = rd_u16(b, i + 20) {
                if i + 22 + comment_len as usize == b.len() {
                    return Some(i);
                }
            }
        }
        if i == start {
            return None;
        }
        i -= 1;
    }
}

/// **解析一个 zip 的中央目录。**
///
/// 返回全部条目。**以中央目录为准**（APPNOTE 的要求，也是"两个头不一致时
/// 该信哪个"的唯一正确答案）。
pub fn parse_central_directory(b: &[u8]) -> Result<Vec<Entry>, ParseError> {
    if b.len() < 22 {
        return Err(ParseError::TooShort);
    }
    // ZIP64 定位器就在 EOCD 之前 20 字节处；出现它说明这是 zip64
    if b.len() >= 20 {
        for probe in [b.len() - 22, b.len().saturating_sub(42)] {
            if rd_u32(b, probe) == Some(SIG_ZIP64_LOCATOR) {
                return Err(ParseError::Zip64Unsupported);
            }
        }
    }

    let eocd = find_eocd(b).ok_or(ParseError::NoEndOfCentralDirectory)?;

    let disk = rd_u16(b, eocd + 4).unwrap_or(0);
    let cd_disk = rd_u16(b, eocd + 6).unwrap_or(0);
    if disk != 0 || cd_disk != 0 {
        return Err(ParseError::MultiDiskUnsupported);
    }
    let declared = rd_u16(b, eocd + 10).unwrap_or(0) as usize;
    let cd_size = rd_u32(b, eocd + 12).unwrap_or(0) as u64;
    let cd_off = rd_u32(b, eocd + 16).unwrap_or(0) as u64;

    let cd_end = cd_off
        .checked_add(cd_size)
        .ok_or(ParseError::CentralDirectoryOutOfRange)?;
    if cd_end > b.len() as u64 {
        return Err(ParseError::CentralDirectoryOutOfRange);
    }

    let mut out: Vec<Entry> = Vec::with_capacity(declared.min(4096));
    let mut p = cd_off as usize;
    while p + 46 <= b.len() {
        let sig = rd_u32(b, p).unwrap_or(0);
        if sig != SIG_CENTRAL {
            // 走到中央目录末尾是正常终止；**但若还没到声明数量就断了**，那是损坏
            if out.len() == declared {
                break;
            }
            return Err(ParseError::BadCentralSignature { at: p as u64 });
        }
        let bad = ParseError::BadCentralSignature { at: p as u64 };
        let flags = rd_u16(b, p + 8).ok_or(bad.clone())?;
        let method_raw = rd_u16(b, p + 10).ok_or(bad.clone())?;
        let crc32 = rd_u32(b, p + 16).unwrap_or(0);
        let comp_size = rd_u32(b, p + 20).unwrap_or(0) as u64;
        let uncomp_size = rd_u32(b, p + 24).unwrap_or(0) as u64;
        let name_len = rd_u16(b, p + 28).unwrap_or(0) as usize;
        let extra_len = rd_u16(b, p + 30).unwrap_or(0) as usize;
        let comment_len = rd_u16(b, p + 32).unwrap_or(0) as usize;
        let ext_attrs = rd_u32(b, p + 38).unwrap_or(0);
        let local_off = rd_u32(b, p + 42).unwrap_or(0) as u64;

        let name_at = p + 46;
        let name_bytes = b
            .get(name_at..name_at + name_len)
            .ok_or(bad.clone())?
            .to_vec();
        let name_utf8 = if flags & 0x0800 != 0 {
            std::str::from_utf8(&name_bytes).ok().map(|s| s.to_string())
        } else {
            None
        };
        let display = match &name_utf8 {
            Some(s) => s.clone(),
            None => String::from_utf8_lossy(&name_bytes).into_owned(),
        };

        if flags & 0x0001 != 0 {
            return Err(ParseError::EncryptedUnsupported { name: display });
        }
        // **数据描述符模式**：长度在数据之后 —— 明确拒绝
        if flags & 0x0008 != 0 && (comp_size == 0 || uncomp_size == 0) {
            return Err(ParseError::DataDescriptorUnsupported { name: display });
        }
        let method = Method::from_u16(method_raw).ok_or(ParseError::UnsupportedMethod {
            method: method_raw,
            name: display.clone(),
        })?;

        out.push(Entry {
            name_bytes,
            name_utf8,
            method,
            compressed_size: comp_size,
            uncompressed_size: uncomp_size,
            local_header_offset: local_off,
            is_dir: display.ends_with('/'),
            is_symlink: Entry::mode_is_symlink(ext_attrs),
            crc32,
            flags,
        });

        let next = name_at
            .checked_add(name_len)
            .and_then(|x| x.checked_add(extra_len))
            .and_then(|x| x.checked_add(comment_len))
            .ok_or(bad)?;
        // **必须前进**：一个损坏的 comment_len 会让 p 不前进而无限循环。
        if next <= p {
            return Err(ParseError::BadCentralSignature { at: p as u64 });
        }
        p = next;
        // 宽松上界：声明数 + 64（防御损坏文件造成的内存膨胀）
        if out.len() > declared.saturating_add(64) {
            return Err(ParseError::EntryCountMismatch {
                declared,
                found: out.len(),
            });
        }
    }

    if out.len() != declared {
        return Err(ParseError::EntryCountMismatch {
            declared,
            found: out.len(),
        });
    }
    Ok(out)
}

/// 一条条目的**压缩数据**在整体字节里的位置与长度。
///
/// 它由 [`entry_data_range`] 算出，而**它是"读数据"这一步的全部知识** ——
/// 把本地头的解析集中在这一个函数里，IO 侧就不必再懂 ZIP 的布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRange {
    pub start: usize,
    pub len: usize,
    /// 本地头里声明的压缩尺寸（用于与中央目录交叉验证）
    pub local_comp_size: u64,
}

/// **算出条目的压缩数据在文件里的位置。**
///
/// 它做两件事：解析本地头，以及**交叉验证本地头与中央目录**。
///
/// ## 为什么必须交叉验证
///
/// APPNOTE 说中央目录是权威的，但**本地头的尺寸被篡改过**是攻击手法之一
/// （"zip 混淆"）：让解压器读一段越界或重叠的数据。
/// 我们只在**两者一致时**才继续 —— 不一致就拒绝并说清差在哪。
///
/// **例外**：开了数据描述符标志（bit 3）时本地头里的尺寸允许为 0 ——
/// 但那种条目我们在解析阶段就已经拒绝了，所以这里看不到它。
pub fn entry_data_range(b: &[u8], e: &Entry) -> Result<DataRange, ParseError> {
    let off = e.local_header_offset as usize;
    if rd_u32(b, off) != Some(SIG_LOCAL) {
        return Err(ParseError::BadLocalHeader {
            at: e.local_header_offset,
        });
    }
    let bad = ParseError::BadLocalHeader {
        at: e.local_header_offset,
    };
    let name_len = rd_u16(b, off + 26).ok_or(bad.clone())? as usize;
    let extra_len = rd_u16(b, off + 28).ok_or(bad.clone())? as usize;
    let local_comp = rd_u32(b, off + 18).ok_or(bad.clone())? as u64;

    if local_comp != e.compressed_size {
        return Err(ParseError::LocalSizeMismatch {
            name: e.display_name(),
            local_comp,
            central_comp: e.compressed_size,
        });
    }

    let start = off + 30 + name_len + extra_len;
    let len = e.compressed_size as usize;
    let end = start.checked_add(len).ok_or(bad)?;
    if end > b.len() {
        return Err(ParseError::CentralDirectoryOutOfRange);
    }
    Ok(DataRange {
        start,
        len,
        local_comp_size: local_comp,
    })
}

// ───────────────────────── CRC-32 ─────────────────────────

/// CRC-32（IEEE 802.3，ZIP 用的那个）查表。
fn crc_table() -> &'static [u32; 256] {
    use std::sync::OnceLock;
    static T: OnceLock<[u32; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        t
    })
}

/// 流式 CRC-32。
#[derive(Debug, Clone)]
pub struct Crc32 {
    state: u32,
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub const fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }
    pub fn update(&mut self, data: &[u8]) {
        let t = crc_table();
        for b in data {
            let idx = ((self.state ^ *b as u32) & 0xFF) as usize;
            self.state = t[idx] ^ (self.state >> 8);
        }
    }
    pub const fn finalize(self) -> u32 {
        self.state ^ 0xFFFF_FFFF
    }
}

/// 一次性算 CRC-32。
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(data);
    c.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────── CRC-32 的已知值 ─────────────────

    #[test]
    fn crc32_的已知值() {
        // 最广为引用的 CRC-32 向量
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"a"), 0xE8B7_BE43);
        assert_eq!(crc32(b"abc"), 0x3524_41C2);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn crc32_流式与一次性一致() {
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let one = crc32(&data);
        for chunk in [1usize, 7, 63, 64, 65, 1000] {
            let mut c = Crc32::new();
            for part in data.chunks(chunk) {
                c.update(part);
            }
            assert_eq!(c.finalize(), one, "分块 {chunk}");
        }
    }

    // ───────────────── 构造最小 zip ─────────────────

    /// 造一个**真实结构**的 stored（不压缩）zip。
    ///
    /// 手写而不是找一个现成的：**这样每一个字节的含义都在我手里**，
    /// 于是"边界样本"（超长注释、含 `PK\x05\x06` 的注释、坏签名）都能精确构造。
    fn build_stored_zip(entries: &[(&[u8], &[u8], u32)]) -> Vec<u8> {
        // entries: (name_bytes, content, external_attrs)
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, content, ext) in entries {
            let off = out.len() as u32;
            let crc = crc32(content);
            // 本地头
            out.extend_from_slice(&SIG_LOCAL.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&0u16.to_le_bytes()); // method = stored
            out.extend_from_slice(&0u16.to_le_bytes()); // mod time
            out.extend_from_slice(&0u16.to_le_bytes()); // mod date
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(content.len() as u32).to_le_bytes());
            out.extend_from_slice(&(content.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra
            out.extend_from_slice(name);
            out.extend_from_slice(content);
            // 中央目录项
            central.extend_from_slice(&SIG_CENTRAL.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central.extend_from_slice(&20u16.to_le_bytes()); // version needed
            central.extend_from_slice(&0u16.to_le_bytes()); // flags
            central.extend_from_slice(&0u16.to_le_bytes()); // method
            central.extend_from_slice(&0u16.to_le_bytes()); // time
            central.extend_from_slice(&0u16.to_le_bytes()); // date
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(content.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // extra
            central.extend_from_slice(&0u16.to_le_bytes()); // comment
            central.extend_from_slice(&0u16.to_le_bytes()); // disk start
            central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            central.extend_from_slice(&ext.to_le_bytes());
            central.extend_from_slice(&off.to_le_bytes());
            central.extend_from_slice(name);
        }
        let cd_off = out.len() as u32;
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        // EOCD
        out.extend_from_slice(&SIG_EOCD.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // disk
        out.extend_from_slice(&0u16.to_le_bytes()); // cd disk
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment len
        out
    }

    fn attrs_file() -> u32 {
        // Unix 普通文件 0o100644，放到高 16 位
        (0o100_644u32) << 16
    }
    fn attrs_symlink() -> u32 {
        (0o120_777u32) << 16
    }

    // ───────────────── 正常解析 ─────────────────

    #[test]
    fn 解析一个最小_zip() {
        let z = build_stored_zip(&[(b"a.txt", b"hello", attrs_file())]);
        let e = parse_central_directory(&z).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].display_name(), "a.txt");
        assert_eq!(e[0].method, Method::Stored);
        assert_eq!(e[0].uncompressed_size, 5);
        assert_eq!(e[0].crc32, crc32(b"hello"));
        assert!(!e[0].is_dir);
        assert!(!e[0].is_symlink);
    }

    #[test]
    fn 解析多个条目且顺序保持() {
        let z = build_stored_zip(&[
            (b"one.txt", b"1", attrs_file()),
            (b"dir/two.txt", b"22", attrs_file()),
            (b"three.txt", b"333", attrs_file()),
        ]);
        let e = parse_central_directory(&z).unwrap();
        let names: Vec<String> = e.iter().map(|x| x.display_name()).collect();
        assert_eq!(names, vec!["one.txt", "dir/two.txt", "three.txt"]);
    }

    #[test]
    fn 目录条目以斜杠结尾判定() {
        let z = build_stored_zip(&[
            (b"dir/", b"", attrs_file()),
            (b"dir/f.txt", b"x", attrs_file()),
        ]);
        let e = parse_central_directory(&z).unwrap();
        assert!(e[0].is_dir);
        assert!(!e[1].is_dir);
    }

    #[test]
    fn 能取到压缩数据的精确位置() {
        let z = build_stored_zip(&[(b"x.bin", b"PAYLOAD", attrs_file())]);
        let e = parse_central_directory(&z).unwrap();
        let r = entry_data_range(&z, &e[0]).unwrap();
        assert_eq!(&z[r.start..r.start + r.len], b"PAYLOAD");
    }

    // ───────────────── 边界：EOCD 的位置 ─────────────────

    #[test]
    fn 带注释的_zip_仍能解析() {
        // EOCD 后面可以有注释（最多 65535）。**不加注释的实现在这里会过，
        // 而带注释的真实 zip 会失败** —— 所以这条是必要的。
        let mut z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        let comment = b"this is a comment";
        let n = z.len();
        // 改写 EOCD 的 comment_len 并追加注释
        let eocd = find_eocd(&z).unwrap();
        z[eocd + 20..eocd + 22].copy_from_slice(&(comment.len() as u16).to_le_bytes());
        z.extend_from_slice(comment);
        assert_eq!(z.len(), n + comment.len());
        let e = parse_central_directory(&z).unwrap();
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn 注释里出现_eocd_签名不会骗过解析() {
        // ⚠️ 这是「从尾部找第一个签名」那个常见错法的直接反驳。
        // 注释里可以出现 PK 05 06 那四个字节，而真正的 EOCD 在它之前。
        //
        // **测试的第一版构造错了**：我把假签名放在注释的**最末尾**，
        // 于是它恰好也自洽（i + 22 + 0 == 文件长度）—— 那是一个退化情形，
        // 而不是现实中会遇到的形态。现实里的注释是任意文本，
        // 假签名会在注释中间，而真 EOCD 仍是最后一个自洽的候选。
        let mut z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        let eocd = find_eocd(&z).unwrap();
        let mut comment = Vec::new();
        comment.extend_from_slice(b"junk");
        comment.extend_from_slice(&SIG_EOCD.to_le_bytes()); // 注释中间的假签名
        comment.extend_from_slice(b"more junk after the fake signature");
        z[eocd + 20..eocd + 22].copy_from_slice(&(comment.len() as u16).to_le_bytes());
        z.extend_from_slice(&comment);
        let e = parse_central_directory(&z).expect("真 EOCD 应当被找到");
        assert_eq!(e.len(), 1, "假签名不该把条目数读成 0");
        assert_eq!(e[0].display_name(), "a");
    }

    // ───────────────── 拒绝路径（每一条都是一个可穷举的理由）─────────────────

    #[test]
    fn 太短的文件被拒绝() {
        assert_eq!(
            parse_central_directory(b"PK").unwrap_err(),
            ParseError::TooShort
        );
        assert_eq!(
            parse_central_directory(&[0u8; 10]).unwrap_err(),
            ParseError::TooShort
        );
    }

    #[test]
    fn 不是_zip_的文件被拒绝() {
        let junk = vec![0x41u8; 1000];
        assert_eq!(
            parse_central_directory(&junk).unwrap_err(),
            ParseError::NoEndOfCentralDirectory
        );
    }

    #[test]
    fn 中央目录越界被拒绝() {
        let mut z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        let eocd = find_eocd(&z).unwrap();
        // 把 cd_off 改成一个越界值
        z[eocd + 16..eocd + 20].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
        assert_eq!(
            parse_central_directory(&z).unwrap_err(),
            ParseError::CentralDirectoryOutOfRange
        );
    }

    #[test]
    fn 条目数不符被拒绝() {
        let mut z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        let eocd = find_eocd(&z).unwrap();
        z[eocd + 10..eocd + 12].copy_from_slice(&5u16.to_le_bytes());
        match parse_central_directory(&z).unwrap_err() {
            ParseError::EntryCountMismatch { declared, .. } => assert_eq!(declared, 5),
            other => panic!("应当是条目数不符，实际 {other:?}"),
        }
    }

    #[test]
    fn 加密条目被拒绝() {
        // 我们不支持密码解压 —— 而**假装支持**会让用户看到一堆乱码。
        let mut z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        let eocd = find_eocd(&z).unwrap();
        let cd_off = rd_u32(&z, eocd + 16).unwrap() as usize;
        // 在中央目录条目里把 flags 置上 bit 0（加密）
        z[cd_off + 8..cd_off + 10].copy_from_slice(&1u16.to_le_bytes());
        match parse_central_directory(&z).unwrap_err() {
            ParseError::EncryptedUnsupported { name } => assert_eq!(name, "a"),
            other => panic!("应当报加密，实际 {other:?}"),
        }
    }

    #[test]
    fn 不支持的压缩方法被拒绝() {
        let mut z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        let eocd = find_eocd(&z).unwrap();
        let cd_off = rd_u32(&z, eocd + 16).unwrap() as usize;
        // 方法改成 12（bzip2）
        z[cd_off + 10..cd_off + 12].copy_from_slice(&12u16.to_le_bytes());
        match parse_central_directory(&z).unwrap_err() {
            ParseError::UnsupportedMethod { method, .. } => assert_eq!(method, 12),
            other => panic!("应当报不支持的方法，实际 {other:?}"),
        }
    }

    #[test]
    fn 数据描述符模式被明确拒绝并说清理由() {
        // 支持它需要**流式猜测边界**，而那正是 zip 解析里 bug 最多的地方。
        // **拒绝比猜好**：Minecraft 的官方 zip 都用中央目录里的长度。
        let mut z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        let eocd = find_eocd(&z).unwrap();
        let cd_off = rd_u32(&z, eocd + 16).unwrap() as usize;
        // flags bit 3 = 数据描述符；同时把尺寸清零
        z[cd_off + 8..cd_off + 10].copy_from_slice(&8u16.to_le_bytes());
        z[cd_off + 20..cd_off + 24].copy_from_slice(&0u32.to_le_bytes());
        z[cd_off + 24..cd_off + 28].copy_from_slice(&0u32.to_le_bytes());
        match parse_central_directory(&z).unwrap_err() {
            ParseError::DataDescriptorUnsupported { name } => assert_eq!(name, "a"),
            other => panic!("应当报数据描述符，实际 {other:?}"),
        }
    }

    #[test]
    fn 分卷_zip_被拒绝() {
        let mut z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        let eocd = find_eocd(&z).unwrap();
        z[eocd + 4..eocd + 6].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            parse_central_directory(&z).unwrap_err(),
            ParseError::MultiDiskUnsupported
        );
    }

    #[test]
    fn zip64_被拒绝() {
        // 本项目要处理的 zip 远小于 4 GB，而支持 zip64 会引入额外分支。
        // 明确拒绝它，而不是"部分支持"。
        let z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        let eocd = find_eocd(&z).unwrap();
        // 在 EOCD 之前插一个 zip64 定位器
        let mut z2 = z[..eocd].to_vec();
        z2.extend_from_slice(&SIG_ZIP64_LOCATOR.to_le_bytes());
        z2.extend_from_slice(&[0u8; 16]);
        z2.extend_from_slice(&z[eocd..]);
        assert_eq!(
            parse_central_directory(&z2).unwrap_err(),
            ParseError::Zip64Unsupported
        );
    }

    #[test]
    fn 本地头签名不对时被拒绝() {
        let mut z = build_stored_zip(&[(b"a", b"1", attrs_file())]);
        z[0..4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let e = parse_central_directory(&z).unwrap();
        assert!(matches!(
            entry_data_range(&z, &e[0]).unwrap_err(),
            ParseError::BadLocalHeader { .. }
        ));
    }

    #[test]
    fn 本地头与中央目录尺寸不一致时被拒绝() {
        // ⚠️ **这是"zip 混淆"攻击的一类**：篡改本地头让解压器读写越界或重叠的数据。
        // 我们只在两者一致时才继续。
        let mut z = build_stored_zip(&[(b"a", b"1234567890", attrs_file())]);
        let e = parse_central_directory(&z).unwrap();
        // 只改本地头里的压缩尺寸
        z[18..22].copy_from_slice(&3u32.to_le_bytes());
        match entry_data_range(&z, &e[0]).unwrap_err() {
            ParseError::LocalSizeMismatch {
                local_comp,
                central_comp,
                ..
            } => {
                assert_eq!(local_comp, 3);
                assert_eq!(central_comp, 10);
            }
            other => panic!("应当报尺寸不一致，实际 {other:?}"),
        }
    }

    // ───────────────── 路径危险信号（**只是信号，不是判定**）─────────────────

    #[test]
    fn 含父段的条目会被标出来() {
        // 注意：**`a/../b` 是合法的、无害的。** 所以这只是"值得注意"，
        // 而真正的拒绝在路径解析之后（`qul-infra::zip`）。
        let z = build_stored_zip(&[
            (b"safe.txt", b"1", attrs_file()),
            (b"../evil.txt", b"2", attrs_file()),
            (b"a/../b.txt", b"3", attrs_file()),
            (b"..\\win-evil.txt", b"4", attrs_file()),
        ]);
        let e = parse_central_directory(&z).unwrap();
        assert!(!e[0].has_parent_segment());
        assert!(e[1].has_parent_segment());
        assert!(e[2].has_parent_segment(), "a/../b 含父段");
        assert!(e[3].has_parent_segment(), "反斜杠形式也要认出来");
    }

    #[test]
    fn 绝对路径的条目会被标出来() {
        let z = build_stored_zip(&[
            (b"rel.txt", b"1", attrs_file()),
            (b"/abs.txt", b"2", attrs_file()),
            (b"\\abs2.txt", b"3", attrs_file()),
            (b"C:/drive.txt", b"4", attrs_file()),
        ]);
        let e = parse_central_directory(&z).unwrap();
        assert!(!e[0].is_absolute());
        assert!(e[1].is_absolute());
        assert!(e[2].is_absolute());
        assert!(e[3].is_absolute(), "盘符形式也要认出来");
    }

    #[test]
    fn 符号链接条目被识别() {
        // **一个 zip 里可以放一个指向任意位置的链接**，而
        // "解压出一个链接"与"解压出一个文件"在安全上完全不同 ——
        // 后续写入会跟着链接走到外面。所以它必须是 `Entry` 的一个字段。
        let z = build_stored_zip(&[
            (b"regular.txt", b"1", attrs_file()),
            (b"link", b"../../etc/passwd", attrs_symlink()),
        ]);
        let e = parse_central_directory(&z).unwrap();
        assert!(!e[0].is_symlink);
        assert!(e[1].is_symlink, "S_IFLNK 必须被识别");
        // 交叉验证判定函数本身
        assert!(Entry::mode_is_symlink(attrs_symlink()));
        assert!(!Entry::mode_is_symlink(attrs_file()));
        assert!(!Entry::mode_is_symlink(0));
    }

    // ───────────────── 非 UTF-8 名字 ─────────────────

    #[test]
    fn 名字不是合法_utf8_时保留字节而不做有损猜测() {
        // ⚠️ 未置 UTF-8 标志位时，名字可能是 CP437 或**本地编码**（中文环境常见）。
        // 而"未置位就按 UTF-8 解码"是常见的错法 —— 它会产出乱码，
        // 而乱码会被用来当文件名。
        let bad_name: &[u8] = &[0xD6, 0xD0, 0xCE, 0xC4]; // GBK 的"中文"
        let z = build_stored_zip(&[(bad_name, b"x", attrs_file())]);
        let e = parse_central_directory(&z).unwrap();
        assert_eq!(e[0].name_bytes, bad_name, "原始字节必须被保住");
        assert!(e[0].name_utf8.is_none(), "无效 UTF-8 不该被解出来");
        // 而展示名是有损的（只用于日志）
        assert!(!e[0].display_name().is_empty());
    }

    #[test]
    fn 置了_utf8_标志位时严格解码() {
        let mut z = build_stored_zip(&[(b"plain.txt", b"x", attrs_file())]);
        let eocd = find_eocd(&z).unwrap();
        let cd_off = rd_u32(&z, eocd + 16).unwrap() as usize;
        // 置 bit 11
        z[cd_off + 8..cd_off + 10].copy_from_slice(&0x0800u16.to_le_bytes());
        let e = parse_central_directory(&z).unwrap();
        assert_eq!(e[0].name_utf8.as_deref(), Some("plain.txt"));
        assert_eq!(e[0].flags & 0x0800, 0x0800);
    }

    // ───────────────── 方法枚举 ─────────────────

    #[test]
    fn 方法枚举的往返与键() {
        for m in [Method::Stored, Method::Deflate] {
            assert_eq!(Method::from_u16(m.as_u16()), Some(m), "{m:?}");
            assert!(m.key().is_ascii());
        }
        assert_eq!(Method::from_u16(99), None);
        assert_eq!(Method::Stored.as_u16(), 0);
        assert_eq!(Method::Deflate.as_u16(), 8);
    }
}
