//! # deflate（inflate）解码器（**纯规则，零 IO**）
//!
//! RFC 1951。三种块类型都要支持：
//!
//! | 类型 | 名字 | 用在哪 |
//! |---|---|---|
//! | `00` | 存储块 | 已压缩数据里的"没压动"片段 |
//! | `01` | 固定 Huffman | 小文件（Minecraft 的 `natives` 里常见） |
//! | `10` | 动态 Huffman | 大多数真实 zip 条目 |
//! | `11` | 保留 | **非法的，必须拒绝** |
//!
//! ## 三条纪律
//!
//! 1. **不许无限膨胀。** 一条精心构造的 deflate 流可以解出远大于输入的数据
//!    （"zip 炸弹"）。所以解码器接受一个**上限**，超了就拒绝 ——
//!    而不是先把内存吃光再报错。
//! 2. **任何越界读都必须被拒绝**，而不是"读到什么算什么"。
//!    一个"宽容"的解码器会在畸形输入上产出**看似成功但内容错**的数据 ——
//!    那种数据随后会被 CRC 拦住，表现为"CRC 不匹配"，而真正的原因在解码器里。
//! 3. **距离不能指到"还没输出"的地方。** 这是 deflate 里最经典的越界
//!    （LZ77 的 back-reference 只能指已输出的数据）。
//!
//! ## 为什么"超上限就拒绝"而不是"截断"
//!
//! 因为截断会产出一个**长度不对但看起来像成功的**结果，而它随后被 CRC 拦住 ——
//! 于是排查方向会被引向"数据坏了"而不是"这条流在膨胀"。
//! **明确拒绝并说清"解出了多少、上限是多少"**，比截断有用得多。

/// 解码错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InflateError {
    /// 输入在读到一个完整符号之前就用完了
    UnexpectedEof { at_bit: usize },
    /// 遇到了保留的块类型（`11`）
    ReservedBlockType,
    /// 存储块的长度字段与它的补码不一致
    StoredLengthMismatch { len: u16, nlen: u16 },
    /// Huffman 码表非法（码长超出、或码表不完整到无法解出任何符号）
    BadHuffmanTable { why: &'static str },
    /// 解出的数据超过了声明的上限
    TooLarge { produced: usize, limit: usize },
    /// 距离指向了还没输出的位置（LZ77 越界）
    DistanceOutOfRange { distance: usize, produced: usize },
    /// 长度/距离的额外位读不到了
    TruncatedExtraBits,
    /// 流没有以"最后一块"的标记结束
    MissingEndOfBlock,
}

impl std::fmt::Display for InflateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InflateError::UnexpectedEof { at_bit } => write!(f, "数据在 bit {at_bit} 处意外结束"),
            InflateError::ReservedBlockType => write!(f, "遇到了保留的块类型（11）"),
            InflateError::StoredLengthMismatch { len, nlen } => {
                write!(f, "存储块的长度字段不自洽：len={len}、~len={nlen}")
            }
            InflateError::BadHuffmanTable { why } => write!(f, "Huffman 码表非法：{why}"),
            InflateError::TooLarge { produced, limit } => write!(
                f,
                "解出的数据 {produced} 字节超过上限 {limit} 字节 —— \
                 这可能是一条构造出来的膨胀流"
            ),
            InflateError::DistanceOutOfRange { distance, produced } => write!(
                f,
                "距离 {distance} 指向了还没输出的位置（目前只输出了 {produced} 字节）"
            ),
            InflateError::TruncatedExtraBits => write!(f, "长度/距离的额外位读不到了"),
            InflateError::MissingEndOfBlock => write!(f, "流没有以最后一块的标记结束"),
        }
    }
}

impl std::error::Error for InflateError {}

/// 按位读入（**LSB first** —— deflate 的位序与直觉相反，这本身是一个坑）。
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            bit: 0,
        }
    }

    /// 当前读到的总位位置（错误信息用）。
    fn bit_pos(&self) -> usize {
        self.pos * 8 + self.bit as usize
    }

    fn read_bit(&mut self) -> Result<u32, InflateError> {
        if self.pos >= self.data.len() {
            return Err(InflateError::UnexpectedEof {
                at_bit: self.bit_pos(),
            });
        }
        let b = self.data[self.pos];
        let v = ((b >> self.bit) & 1) as u32;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Ok(v)
    }

    /// 读 `n` 位（`n <= 16`），**低位在前**。
    fn read_bits(&mut self, n: u32) -> Result<u32, InflateError> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.read_bit()? << i;
        }
        Ok(v)
    }

    /// 对齐到字节边界（存储块用）。
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }
}

/// 一个 Huffman 解码表。
///
/// ## 为什么用"按位走树"而不是查表
///
/// 查表更快，但要处理"码长不满 8 位"的补表逻辑 —— 而那是**最容易出错的
/// 一段代码**，且它的错误表现为"偶发解错"而不是"报错"。
///
/// 本项目要解的是 Minecraft 的 `natives` 与整合包，**不缺这点性能**，
/// 而"按位走树"的每一步都可以被断言。**正确性优先于吞吐，在这里是划算的。**
#[derive(Debug, Clone)]
struct Huffman {
    /// `counts[len]` = 码长为 `len` 的符号数（`len` 从 1 到 15）
    counts: [u16; 16],
    /// 按码长排序的符号表
    symbols: Vec<u16>,
}

impl Huffman {
    /// 从码长表构造（RFC 1951 §3.2.2 的规范构造法）。
    fn from_lengths(lengths: &[u8]) -> Result<Self, InflateError> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            if l > 15 {
                return Err(InflateError::BadHuffmanTable {
                    why: "码长超过 15"
                });
            }
            counts[l as usize] += 1;
        }
        // 码长 0 = 不用这个符号
        counts[0] = 0;

        // 检查"码表不超编"（Kraft 不等式）
        let mut left = 1i32;
        // 从码长 1 起逐级扣减（Kraft 不等式）
        for c in counts.iter().take(16).skip(1) {
            left <<= 1;
            left -= *c as i32;
            if left < 0 {
                return Err(InflateError::BadHuffmanTable {
                    why: "码长分配超过可用空间",
                });
            }
        }

        // 每个码长的起始码
        let mut offsets = [0u16; 16];
        for len in 1..15 {
            offsets[len + 1] = offsets[len] + counts[len];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offsets[l as usize] as usize] = sym as u16;
                offsets[l as usize] += 1;
            }
        }
        Ok(Self { counts, symbols })
    }

    /// 解一个符号。
    fn decode(&self, r: &mut BitReader<'_>) -> Result<u16, InflateError> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..=15usize {
            code |= r.read_bit()? as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                let at = (index + (code - first)) as usize;
                return self
                    .symbols
                    .get(at)
                    .copied()
                    .ok_or(InflateError::BadHuffmanTable {
                        why: "符号索引越界",
                    });
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(InflateError::BadHuffmanTable {
            why: "没有匹配到任何码（码表可能不完整）",
        })
    }
}

/// 长度码表（RFC 1951 §3.2.5）：码 257–285 → 基础长度与额外位。
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// **解一条 raw deflate 流。**
///
/// `limit` 是解出数据的**上限**（防膨胀）。传 `usize::MAX` 表示不设限
/// —— 而**那不该在生产路径上用**（见模块文档第 1 条）。
pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, InflateError> {
    let mut r = BitReader::new(data);
    let mut out: Vec<u8> = Vec::new();
    let mut saw_final = false;

    while !saw_final {
        let bfinal = r.read_bit()?;
        let btype = r.read_bits(2)?;
        saw_final = bfinal == 1;

        match btype {
            0 => inflate_stored(&mut r, &mut out, limit)?,
            1 => {
                let (lit, dist) = fixed_tables();
                inflate_block(&mut r, &mut out, &lit, &dist, limit)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut r)?;
                inflate_block(&mut r, &mut out, &lit, &dist, limit)?;
            }
            _ => return Err(InflateError::ReservedBlockType),
        }

        if out.len() > limit {
            return Err(InflateError::TooLarge {
                produced: out.len(),
                limit,
            });
        }
    }
    if !saw_final {
        return Err(InflateError::MissingEndOfBlock);
    }
    Ok(out)
}

/// 每次调用都重建固定表太浪费，所以缓存。
fn fixed_tables() -> (Huffman, Huffman) {
    use std::sync::OnceLock;
    static LIT: OnceLock<Huffman> = OnceLock::new();
    static DIST: OnceLock<Huffman> = OnceLock::new();
    let lit = LIT.get_or_init(|| {
        let mut l = vec![0u8; 288];
        // 0–143: 8 位；144–255: 9 位；256–279: 7 位；280–287: 8 位
        for (i, v) in l.iter_mut().enumerate() {
            *v = match i {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        Huffman::from_lengths(&l).expect("固定字面量表是规格给定的，必合法")
    });
    let dist = DIST.get_or_init(|| {
        let d = vec![5u8; 30];
        Huffman::from_lengths(&d).expect("固定距离表是规格给定的，必合法")
    });
    (lit.clone(), dist.clone())
}

fn dynamic_tables(r: &mut BitReader<'_>) -> Result<(Huffman, Huffman), InflateError> {
    let hlit = r.read_bits(5)? as usize + 257;
    let hdist = r.read_bits(5)? as usize + 1;
    let hclen = r.read_bits(4)? as usize + 4;

    // 码长表的码长按这个**固定顺序**读（RFC 1951 §3.2.7）
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut cl = vec![0u8; 19];
    for i in 0..hclen {
        cl[ORDER[i]] = r.read_bits(3)? as u8;
    }
    let cl_table = Huffman::from_lengths(&cl)?;

    // 用码长表解出字面量与距离表的码长
    let total = hlit + hdist;
    let mut lengths: Vec<u8> = Vec::with_capacity(total);
    while lengths.len() < total {
        let sym = cl_table.decode(r)?;
        match sym {
            0..=15 => lengths.push(sym as u8),
            16 => {
                // 重复前一个码长 3–6 次
                let prev = *lengths.last().ok_or(InflateError::BadHuffmanTable {
                    why: "码长重复码出现在开头（没有前一个值）",
                })?;
                let n = 3 + r.read_bits(2)? as usize;
                lengths.extend(std::iter::repeat_n(prev, n));
            }
            17 => {
                let n = 3 + r.read_bits(3)? as usize;
                lengths.extend(std::iter::repeat_n(0u8, n));
            }
            18 => {
                let n = 11 + r.read_bits(7)? as usize;
                lengths.extend(std::iter::repeat_n(0u8, n));
            }
            _ => {
                return Err(InflateError::BadHuffmanTable {
                    why: "码长表里出现了 19 以上的符号",
                })
            }
        }
        // **防膨胀**：重复码可以让 length 一次跳很多，但绝不该超过 total
        if lengths.len() > total {
            return Err(InflateError::BadHuffmanTable {
                why: "码长重复超出了声明的总长",
            });
        }
    }
    lengths.truncate(total);

    let lit = Huffman::from_lengths(&lengths[..hlit])?;
    let dist = Huffman::from_lengths(&lengths[hlit..])?;
    Ok((lit, dist))
}

fn inflate_stored(
    r: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), InflateError> {
    r.align();
    if r.pos + 4 > r.data.len() {
        return Err(InflateError::UnexpectedEof {
            at_bit: r.bit_pos(),
        });
    }
    let len = u16::from_le_bytes([r.data[r.pos], r.data[r.pos + 1]]);
    let nlen = u16::from_le_bytes([r.data[r.pos + 2], r.data[r.pos + 3]]);
    r.pos += 4;
    if len != !nlen {
        return Err(InflateError::StoredLengthMismatch { len, nlen });
    }
    let n = len as usize;
    if r.pos + n > r.data.len() {
        return Err(InflateError::UnexpectedEof {
            at_bit: r.bit_pos(),
        });
    }
    if out.len() + n > limit {
        return Err(InflateError::TooLarge {
            produced: out.len() + n,
            limit,
        });
    }
    out.extend_from_slice(&r.data[r.pos..r.pos + n]);
    r.pos += n;
    Ok(())
}

fn inflate_block(
    r: &mut BitReader<'_>,
    out: &mut Vec<u8>,
    lit: &Huffman,
    dist: &Huffman,
    limit: usize,
) -> Result<(), InflateError> {
    loop {
        let sym = lit.decode(r)?;
        match sym {
            0..=255 => {
                out.push(sym as u8);
                if out.len() > limit {
                    return Err(InflateError::TooLarge {
                        produced: out.len(),
                        limit,
                    });
                }
            }
            256 => return Ok(()), // 块结束
            257..=285 => {
                let idx = (sym - 257) as usize;
                let len = LENGTH_BASE[idx] as usize + r.read_bits(LENGTH_EXTRA[idx])? as usize;
                let dsym = dist.decode(r)? as usize;
                if dsym >= DIST_BASE.len() {
                    return Err(InflateError::BadHuffmanTable {
                        why: "距离符号超出表范围",
                    });
                }
                let distance = DIST_BASE[dsym] as usize + r.read_bits(DIST_EXTRA[dsym])? as usize;
                // **LZ77 的 back-reference 只能指已输出的数据。**
                // 这条越界是 deflate 里最经典的一类，而一个"宽容"的实现
                // 会从 out[0] 开始读，产出一份**内容错但长度对**的数据。
                if distance == 0 || distance > out.len() {
                    return Err(InflateError::DistanceOutOfRange {
                        distance,
                        produced: out.len(),
                    });
                }
                if out.len() + len > limit {
                    return Err(InflateError::TooLarge {
                        produced: out.len() + len,
                        limit,
                    });
                }
                let start = out.len() - distance;
                // 逐字节复制（**不能用切片 spread**：distance 可以小于 len，
                // 此时源与目标是重叠的，而那正是 LZ77 的压缩原理）
                for i in 0..len {
                    let b = out[start + i];
                    out.push(b);
                }
            }
            _ => {
                return Err(InflateError::BadHuffmanTable {
                    why: "字面量/长度符号超出范围（286/287 是保留的）",
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个"存储块"的 deflate 流（**最容易手写的一种**）。
    fn stored_stream(chunks: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for (i, c) in chunks.iter().enumerate() {
            let last = i + 1 == chunks.len();
            // 块头：bfinal 1 位 + btype 00 两位（都从字节的最低位数起）
            out.push(if last { 1 } else { 0 });
            let len = c.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(c);
        }
        out
    }

    #[test]
    fn 存储块() {
        let s = stored_stream(&[b"hello world"]);
        assert_eq!(inflate(&s, 1024).unwrap(), b"hello world");
    }

    #[test]
    fn 多个存储块() {
        let s = stored_stream(&[b"one ", b"two ", b"three"]);
        assert_eq!(inflate(&s, 1024).unwrap(), b"one two three");
    }

    #[test]
    fn 空输入的第一块是空的存储块() {
        let s = stored_stream(&[b""]);
        assert_eq!(inflate(&s, 1024).unwrap(), b"");
    }

    #[test]
    fn 存储块长度不自洽被拒绝() {
        // `len` 与 `~len` 必须互补。一个"忽略这个检查"的实现会读错长度，
        // 产出一份**内容错但长度看起来合理**的数据。
        let mut s = stored_stream(&[b"hello"]);
        s[2] = 0xFF; // 破坏 ~len 的低字节
        assert!(matches!(
            inflate(&s, 1024).unwrap_err(),
            InflateError::StoredLengthMismatch { .. }
        ));
    }

    #[test]
    fn 保留块类型被拒绝() {
        // btype = 11 是保留的
        let s = vec![0b0000_0111u8]; // bfinal=1, btype=11
        assert_eq!(
            inflate(&s, 16).unwrap_err(),
            InflateError::ReservedBlockType
        );
    }

    #[test]
    fn 输入提前结束被拒绝() {
        // 说"这是最后一块、btype=00"，但后面没有长度字段
        let s = vec![1u8];
        assert!(matches!(
            inflate(&s, 16).unwrap_err(),
            InflateError::UnexpectedEof { .. }
        ));
    }

    #[test]
    fn 存储块内容不足被拒绝() {
        // 声明 len=100 但只给了 3 字节内容
        let mut s = vec![1u8];
        s.extend_from_slice(&100u16.to_le_bytes());
        s.extend_from_slice(&(!100u16).to_le_bytes());
        s.extend_from_slice(b"abc");
        assert!(matches!(
            inflate(&s, 1024).unwrap_err(),
            InflateError::UnexpectedEof { .. }
        ));
    }

    // ───────────────── 膨胀防护 ─────────────────

    #[test]
    fn 超过上限就拒绝而不是把内存吃光() {
        // ⚠️ **这是"zip 炸弹"的直接防护。**
        // 一条精心构造的流可以解出远超输入的数据。
        let data = vec![0x41u8; 1000];
        let s = stored_stream(&[&data]);
        let e = inflate(&s, 100).unwrap_err();
        match e {
            InflateError::TooLarge { produced, limit } => {
                assert_eq!(limit, 100);
                assert!(produced > limit, "必须报出它想解出多少");
            }
            other => panic!("应当是 TooLarge，实际 {other:?}"),
        }
    }

    #[test]
    fn 上限恰好等于输出长度时通过() {
        let data = vec![7u8; 64];
        let s = stored_stream(&[&data]);
        assert_eq!(inflate(&s, 64).unwrap().len(), 64);
        // 少一字节就拒绝
        assert!(matches!(
            inflate(&s, 63).unwrap_err(),
            InflateError::TooLarge { .. }
        ));
    }

    // ───────────────── Huffman 表构造 ─────────────────

    #[test]
    fn 码长超过十五被拒绝() {
        let l = vec![16u8];
        assert!(matches!(
            Huffman::from_lengths(&l).unwrap_err(),
            InflateError::BadHuffmanTable { .. }
        ));
    }

    #[test]
    fn 超编的码表被拒绝() {
        // 三个码长都是 1 位 —— 那需要 3 个 1 位码，而 1 位只有 2 个
        let l = vec![1u8, 1, 1];
        match Huffman::from_lengths(&l).unwrap_err() {
            InflateError::BadHuffmanTable { why } => assert!(why.contains("超过可用空间"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn 空码表能构造但解不出任何东西() {
        // 这不是"非法"—— 一个距离表可以全是 0 码长（若这条流从不引用距离）。
        // 但那意味着一遇到需要距离的符号就会失败，而那是对的。
        let h = Huffman::from_lengths(&[0u8, 0, 0]).unwrap();
        let data = [0u8; 8];
        let mut r = BitReader::new(&data);
        assert!(h.decode(&mut r).is_err());
    }

    #[test]
    fn 固定表能构造且往返() {
        let (lit, dist) = fixed_tables();
        // 固定表的规模是规格给定的
        assert_eq!(lit.counts[7], 24, "256–279 共 24 个 7 位码");
        assert_eq!(lit.counts[8], 152, "0–143 与 280–287 共 152 个 8 位码");
        assert_eq!(lit.counts[9], 112, "144–255 共 112 个 9 位码");
        assert_eq!(dist.counts[5], 30);
    }

    // ───────────────── LZ77 越界 ─────────────────

    #[test]
    fn 距离指向未输出的位置被拒绝() {
        // 手写一个固定 Huffman 的块：先一个字面量 'A'，然后一个
        // "复制 3 字节、距离 5" —— 而目前只输出了 1 字节。
        //
        // 固定表的 8 位字面量 'A'(65) 的码是 0x30 + 65 = 0x71（8 位，MSB first）
        // 而长度码 257（长度 3，无额外位）的 7 位码 = 0b0000001
        // 距离码 4（距离 5，无额外位）的 5 位码 = 0b00100
        //
        // 位序是 LSB first，所以要反过来写。
        let mut bits: Vec<bool> = Vec::new();
        // ⚠️ **位序是 deflate 最经典的坑，我在写这些测试时踩了一次。**
        //
        // `bits` 是"按发送顺序"的列表，而打包时 `bits[i]` 会落到字节的
        // **第 i%8 位**（LSB first）—— 也就是"发送顺序 = 从低位数起"。
        //
        // 而 **Huffman 码本身的发送顺序是 MSB-first**（最高位先发）。
        // 所以正确做法是：**逐位把 Huffman 码从高位到低位 push 进 `bits`**，
        // 而**不能**把整个码当整数去 `|=`（那会整体反掉）。
        //
        // 第一版就是那么写的，于是三个用手工 deflate 的测试一起失败 ——
        // 而它们失败的方式是"解出来是别的东西"，不是"报错"。
        /// **把块头三位一起写**（bfinal + btype 的两位）。
        ///
        /// ⚠️ 这个辅助函数是**被一次真实错误逼出来的**：
        /// 第一版里我手写那三位，而 `btype` 的两位是 **LSB first**
        /// （`read_bits(2)` 的值 = `bits[1] + 2*bits[2]`），
        /// 于是我把"固定 Huffman"（`01`）写成了 `[false, true]` ——
        /// 那读出来是 `0b10 = 2`（动态 Huffman），
        /// 而失败的表现是"解出来是别的东西"或"码表非法"，**不是"块类型不对"**。
        ///
        /// 把三位一起写之后，**这个错误在结构上就不可能再犯**。
        fn push_block_header(bits: &mut Vec<bool>, bfinal: bool, btype: u32) {
            bits.push(bfinal);
            bits.push(btype & 1 == 1);
            bits.push((btype >> 1) & 1 == 1);
        }
        let push_code = |bits: &mut Vec<bool>, code: u32, len: u32| {
            for i in (0..len).rev() {
                bits.push((code >> i) & 1 == 1);
            }
        };
        push_block_header(&mut bits, true, 1);
        push_code(&mut bits, 0x30 + 65, 8); // 'A'
        push_code(&mut bits, 0b0000001, 7); // 长度码 257 → 长度 3
        push_code(&mut bits, 0b00100, 5); // 距离码 4 → 距离 5
        push_code(&mut bits, 0b0000000, 7); // 块结束 256

        // 打包成字节（LSB first）
        let mut data = vec![0u8; bits.len().div_ceil(8)];
        for (i, b) in bits.iter().enumerate() {
            if *b {
                data[i / 8] |= 1 << (i % 8);
            }
        }
        let e = inflate(&data, 1024).unwrap_err();
        match e {
            InflateError::DistanceOutOfRange { distance, produced } => {
                assert_eq!(distance, 5);
                assert_eq!(produced, 1, "目前只输出了 'A'");
            }
            other => panic!("应当是距离越界，实际 {other:?}"),
        }
    }

    // ───────────────── 真实 deflate（由固定表手工构造）─────────────────

    #[test]
    fn 固定_huffman_的字面量能解出来() {
        // 只用字面量与块结束 —— 这是"手工构造真实 deflate"的最小可信样本。
        let mut bits: Vec<bool> = Vec::new();
        // ⚠️ **位序是 deflate 最经典的坑，我在写这些测试时踩了一次。**
        //
        // `bits` 是"按发送顺序"的列表，而打包时 `bits[i]` 会落到字节的
        // **第 i%8 位**（LSB first）—— 也就是"发送顺序 = 从低位数起"。
        //
        // 而 **Huffman 码本身的发送顺序是 MSB-first**（最高位先发）。
        // 所以正确做法是：**逐位把 Huffman 码从高位到低位 push 进 `bits`**，
        // 而**不能**把整个码当整数去 `|=`（那会整体反掉）。
        //
        // 第一版就是那么写的，于是三个用手工 deflate 的测试一起失败 ——
        // 而它们失败的方式是"解出来是别的东西"，不是"报错"。
        /// **把块头三位一起写**（bfinal + btype 的两位）。
        ///
        /// ⚠️ 这个辅助函数是**被一次真实错误逼出来的**：
        /// 第一版里我手写那三位，而 `btype` 的两位是 **LSB first**
        /// （`read_bits(2)` 的值 = `bits[1] + 2*bits[2]`），
        /// 于是我把"固定 Huffman"（`01`）写成了 `[false, true]` ——
        /// 那读出来是 `0b10 = 2`（动态 Huffman），
        /// 而失败的表现是"解出来是别的东西"或"码表非法"，**不是"块类型不对"**。
        ///
        /// 把三位一起写之后，**这个错误在结构上就不可能再犯**。
        fn push_block_header(bits: &mut Vec<bool>, bfinal: bool, btype: u32) {
            bits.push(bfinal);
            bits.push(btype & 1 == 1);
            bits.push((btype >> 1) & 1 == 1);
        }
        let push_code = |bits: &mut Vec<bool>, code: u32, len: u32| {
            for i in (0..len).rev() {
                bits.push((code >> i) & 1 == 1);
            }
        };
        push_block_header(&mut bits, true, 1);

        for ch in b"hi" {
            let c = *ch as u32;
            if c <= 143 {
                push_code(&mut bits, 0x30 + c, 8);
            } else {
                push_code(&mut bits, 0x190 + (c - 144), 9);
            }
        }
        push_code(&mut bits, 0b0000000, 7); // 256 = 块结束
        let mut data = vec![0u8; bits.len().div_ceil(8)];
        for (i, b) in bits.iter().enumerate() {
            if *b {
                data[i / 8] |= 1 << (i % 8);
            }
        }
        assert_eq!(inflate(&data, 64).unwrap(), b"hi");
    }

    #[test]
    fn 固定_huffman_的重复引用能解出来() {
        // 'A' 然后"复制 3 字节、距离 1" → "AAAA"
        // 这验证 LZ77 的**重叠复制**（源与目标重叠是压缩的原理，不是 bug）
        let mut bits: Vec<bool> = Vec::new();
        // ⚠️ **位序是 deflate 最经典的坑，我在写这些测试时踩了一次。**
        //
        // `bits` 是"按发送顺序"的列表，而打包时 `bits[i]` 会落到字节的
        // **第 i%8 位**（LSB first）—— 也就是"发送顺序 = 从低位数起"。
        //
        // 而 **Huffman 码本身的发送顺序是 MSB-first**（最高位先发）。
        // 所以正确做法是：**逐位把 Huffman 码从高位到低位 push 进 `bits`**，
        // 而**不能**把整个码当整数去 `|=`（那会整体反掉）。
        //
        // 第一版就是那么写的，于是三个用手工 deflate 的测试一起失败 ——
        // 而它们失败的方式是"解出来是别的东西"，不是"报错"。
        /// **把块头三位一起写**（bfinal + btype 的两位）。
        ///
        /// ⚠️ 这个辅助函数是**被一次真实错误逼出来的**：
        /// 第一版里我手写那三位，而 `btype` 的两位是 **LSB first**
        /// （`read_bits(2)` 的值 = `bits[1] + 2*bits[2]`），
        /// 于是我把"固定 Huffman"（`01`）写成了 `[false, true]` ——
        /// 那读出来是 `0b10 = 2`（动态 Huffman），
        /// 而失败的表现是"解出来是别的东西"或"码表非法"，**不是"块类型不对"**。
        ///
        /// 把三位一起写之后，**这个错误在结构上就不可能再犯**。
        fn push_block_header(bits: &mut Vec<bool>, bfinal: bool, btype: u32) {
            bits.push(bfinal);
            bits.push(btype & 1 == 1);
            bits.push((btype >> 1) & 1 == 1);
        }
        let push_code = |bits: &mut Vec<bool>, code: u32, len: u32| {
            for i in (0..len).rev() {
                bits.push((code >> i) & 1 == 1);
            }
        };
        push_block_header(&mut bits, true, 1);

        push_code(&mut bits, 0x30 + 65, 8); // 'A'
        push_code(&mut bits, 0b0000001, 7); // 长度码 257 → 3
        push_code(&mut bits, 0b00000, 5); // 距离码 0 → 距离 1
        push_code(&mut bits, 0b0000000, 7); // 块结束
        let mut data = vec![0u8; bits.len().div_ceil(8)];
        for (i, b) in bits.iter().enumerate() {
            if *b {
                data[i / 8] |= 1 << (i % 8);
            }
        }
        assert_eq!(inflate(&data, 64).unwrap(), b"AAAA");
    }

    // ───────────────── 位读器 ─────────────────

    #[test]
    fn 位读器是_lsb_first() {
        // deflate 的位序与直觉相反 —— 这本身是一个坑，所以单独钉住。
        let data = [0b1011_0100u8];
        let mut r = BitReader::new(&data);
        assert_eq!(r.read_bit().unwrap(), 0);
        assert_eq!(r.read_bit().unwrap(), 0);
        assert_eq!(r.read_bit().unwrap(), 1);
        assert_eq!(r.read_bits(2).unwrap(), 0b10); // 位 3,4 = 0,1 → LSB first = 0b10
        assert_eq!(r.read_bits(3).unwrap(), 0b101); // 位 5,6,7 = 1,0,1 → 0b101
    }

    #[test]
    fn 位读器越界会报出位置() {
        let data = [0u8];
        let mut r = BitReader::new(&data);
        assert!(r.read_bits(8).is_ok());
        match r.read_bit().unwrap_err() {
            InflateError::UnexpectedEof { at_bit } => assert_eq!(at_bit, 8),
            other => panic!("{other:?}"),
        }
    }
}
