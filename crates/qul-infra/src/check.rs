//! # 校验：SHA-1 与"按内容类型注入"的校验器
//!
//! ## 为什么自己写 SHA-1 而不引依赖
//!
//! 三条理由，最后一条是决定性的：
//!
//! 1. **尖刺阶段每多一条依赖就多一条要审的许可**（与 `qul-infra` 用 `reg.exe`
//!    而不是 `winreg` 是同一条纪律）。
//! 2. SHA-1 是**完全确定、规格封闭**的算法（FIPS 180-1，79 行核心逻辑）——
//!    它的正确性可以被官方测试向量钉死，**没有"边界情况靠实现者猜"的空间**。
//! 3. **它要用在整条 Minecraft 下载链路上**：官方元数据给的就是 SHA-1。
//!    把这个算法掌握在自己手里，意味着**校验这件事不依赖任何第三方代码**。
//!
//! ## ⚠️ 一条必须写下来的安全说明
//!
//! **SHA-1 已经不是抗碰撞的了**（2017 年 SHAttered 攻击）。
//! 但**这不构成本项目的问题**，理由是具体的：
//!
//! | 威胁 | 是否存在 |
//! |---|---|
//! | 攻击者伪造一个与官方文件同哈希的恶意文件 | 需要**主动指定碰撞对**，而官方文件的哈希是既定的 |
//! | 攻击者篡改传输中的文件 | 篡改后哈希几乎必然变化 → 被我们拦住 |
//!
//! 也就是说：**我们用的性质是"哈希能发现意外改动"，而不是"哈希能抵抗有意的碰撞构造"。**
//! 后者我们用不到（我们没有"让攻击者选内容再比对哈希"的场景）。
//!
//! **但这条必须写在代码里**，否则将来有人看到"我们自己实现了 SHA-1"
//! 会误以为我们把它当成了抗碰撞的原语。

use std::fmt::Write as _;

/// SHA-1 的初始状态（FIPS 180-1 §5.3.1）。
const H0: [u32; 5] = [
    0x6745_2301,
    0xEFCD_AB89,
    0x98BA_DCFE,
    0x1032_5476,
    0xC3D2_E1F0,
];

/// 流式 SHA-1。**流式而不是"喂一个 `&[u8]`"**，
/// 因为大文件不该被整份读进内存（`assets/` 里有 400+ MB 的文件）。
#[derive(Debug, Clone)]
pub struct Sha1 {
    h: [u32; 5],
    /// 累计已处理的字节数（只取低 64 位，与规格一致）
    len: u64,
    /// 未满 64 字节的尾部缓冲
    buf: [u8; 64],
    buf_len: usize,
}

impl Default for Sha1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha1 {
    pub const fn new() -> Self {
        Self {
            h: H0,
            len: 0,
            buf: [0u8; 64],
            buf_len: 0,
        }
    }

    /// 喂一段数据。
    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        // 先把缓冲填满
        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            let take = need.min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }
        // 整块直通
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        // 剩下的进缓冲
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) =
            (self.h[0], self.h[1], self.h[2], self.h[3], self.h[4]);

        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        self.h[0] = self.h[0].wrapping_add(a);
        self.h[1] = self.h[1].wrapping_add(b);
        self.h[2] = self.h[2].wrapping_add(c);
        self.h[3] = self.h[3].wrapping_add(d);
        self.h[4] = self.h[4].wrapping_add(e);
    }

    /// 收尾并产出 20 字节摘要。
    pub fn finalize(mut self) -> [u8; 20] {
        let bit_len = self.len.wrapping_mul(8);
        // 填充：0x80 然后补零到 56 mod 64，最后 8 字节是大端位长度
        self.update_raw(&[0x80]);
        while self.buf_len != 56 {
            self.update_raw(&[0x00]);
        }
        self.update_raw(&bit_len.to_be_bytes());

        let mut out = [0u8; 20];
        for (i, v) in self.h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
        }
        out
    }

    /// **不走 `len` 计数**的内部喂数据（填充阶段用）。
    ///
    /// 若填充也走 `update`，`len` 会被改动 → 位长度算错 → 摘要错。
    /// 这是一个很容易踩的坑，所以两个入口分开，并在名字上写清。
    fn update_raw(&mut self, data: &[u8]) {
        for b in data {
            self.buf[self.buf_len] = *b;
            self.buf_len += 1;
            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }
    }
}

/// 一次性算一个摘要。
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(data);
    hex(&h.finalize())
}

/// 20 字节 → 40 位小写十六进制。
pub fn hex(digest: &[u8; 20]) -> String {
    let mut s = String::with_capacity(40);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// 一个"看起来像 SHA-1"的字符串（40 位十六进制）。
pub fn looks_like_sha1(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ───────────────────────── 校验器 ─────────────────────────

/// **按期望哈希校验一个文件**（不变量 I3：校验函数按内容类型注入）。
///
/// 它**流式**读文件：一份 400 MB 的 `assets` 文件不该被整份读进内存。
#[derive(Debug)]
pub struct Sha1Verifier {
    expected: String,
}

impl Sha1Verifier {
    /// 构造。**期望值必须是 40 位十六进制** —— 否则直接返回 `None`。
    ///
    /// 为什么不让它接受任意字符串：一个拼错的期望哈希
    /// （比如漏了一位的）会让**每一份正确文件都被判为不匹配** ——
    /// 而那表现为"下载永远失败"，极难归因。宁可在这里就拒绝。
    pub fn new(expected: &str) -> Option<Self> {
        let e = expected.trim().to_ascii_lowercase();
        if !looks_like_sha1(&e) {
            return None;
        }
        Some(Self { expected: e })
    }

    pub fn expected(&self) -> &str {
        &self.expected
    }
}

impl super::download::Verifier for Sha1Verifier {
    fn verify(&self, path: &std::path::Path) -> Result<(), String> {
        let actual = sha1_file(path)?;
        if actual == self.expected {
            Ok(())
        } else {
            Err(format!("期望 {}，实际 {actual}", self.expected))
        }
    }
    fn name(&self) -> &'static str {
        "SHA-1"
    }
}

/// 流式算一个文件的 SHA-1。
pub fn sha1_file(path: &std::path::Path) -> Result<String, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| e.kind().to_string())?;
    let mut h = Sha1::new();
    // 256 KB 的缓冲：足够大以掩盖系统调用，又小到不占内存
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.kind().to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// 一个**多文件**校验器：校验一个"路径 → 期望哈希"的清单。
///
/// 它存在的理由：Minecraft 的一次安装在**几百个文件**上做校验，
/// 而"逐个构造 `Sha1Verifier`"会让调用方写一个循环 ——
/// **而循环里的哈希拼错一个就会静默漏过一次校验**。
/// 这个类型让"清单"成为一个东西，于是它可以被整体审查。
#[derive(Debug)]
pub struct ManifestVerifier {
    entries: Vec<(String, String)>,
}

impl ManifestVerifier {
    /// 从 `(相对路径, 期望哈希)` 列表构造。**非法哈希会被拒绝**（返回 `Err`）。
    pub fn new(entries: Vec<(String, String)>) -> Result<Self, String> {
        let mut ok = Vec::with_capacity(entries.len());
        for (p, h) in entries {
            let hh = h.trim().to_ascii_lowercase();
            if !looks_like_sha1(&hh) {
                return Err(format!("{p} 的期望哈希不是 40 位十六进制：{h}"));
            }
            ok.push((p, hh));
        }
        Ok(Self { entries: ok })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 校验一个文件是否属于本清单、且哈希匹配。
    pub fn verify_one(&self, relative: &str, path: &std::path::Path) -> Result<(), String> {
        let Some((_, want)) = self.entries.iter().find(|(p, _)| p == relative) else {
            return Err(format!("清单里没有 {relative}"));
        };
        let got = sha1_file(path)?;
        if &got == want {
            Ok(())
        } else {
            Err(format!("{relative}：期望 {want}，实际 {got}"))
        }
    }

    /// 查一个相对路径的期望哈希。
    pub fn expected_of(&self, relative: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(p, _)| p == relative)
            .map(|(_, h)| h.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────── FIPS 180-1 与广泛使用的官方测试向量 ─────────────────

    #[test]
    fn 官方测试向量() {
        // 这些是 SHA-1 最广为引用的向量（含 FIPS 180-1 的 abc 与
        // 后续被广泛引用的长消息向量）。
        let cases: [(&str, &str); 5] = [
            ("", "da39a3ee5e6b4b0d3255bfef95601890afd80709"),
            ("abc", "a9993e364706816aba3e25717850c26c9cd0d89d"),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "84983e441c3bd26ebaae4aa1f95129e5e54670f1",
            ),
            (
                "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
                "a49b2446a02c645bf419f995b67091253a04a259",
            ),
            ("The quick brown fox jumps over the lazy dog", "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"),
        ];
        for (input, want) in cases {
            assert_eq!(sha1_hex(input.as_bytes()), want, "输入 {input:?}");
        }
    }

    #[test]
    fn 一百万个_a() {
        // FIPS 180-1 的第三个向量：一百万个 'a'
        let mut h = Sha1::new();
        let chunk = vec![b'a'; 1000];
        for _ in 0..1000 {
            h.update(&chunk);
        }
        assert_eq!(
            hex(&h.finalize()),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    #[test]
    fn 流式与一次性结果一致() {
        // 流式的分块边界必须不影响结果 —— 这是"流式实现"最容易错的地方：
        // 缓冲拼接、填充、位长度三处都依赖"已处理多少字节"这个计数。
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let one = sha1_hex(&data);
        for chunk in [1usize, 3, 63, 64, 65, 127, 128, 1000] {
            let mut h = Sha1::new();
            for c in data.chunks(chunk) {
                h.update(c);
            }
            assert_eq!(hex(&h.finalize()), one, "分块大小 {chunk}");
        }
    }

    #[test]
    fn 正好跨越填充边界() {
        // 55/56/57 与 63/64/65 是填充边界上的经典陷阱：
        // 55 字节时需要额外一个块，56 字节时需要两个块（因为要放 8 字节长度）。
        for n in [54usize, 55, 56, 57, 63, 64, 65, 119, 120, 121] {
            let data = vec![0x61u8; n];
            let one = sha1_hex(&data);
            let mut h = Sha1::new();
            for c in data.chunks(7) {
                h.update(c);
            }
            assert_eq!(hex(&h.finalize()), one, "长度 {n}");
        }
    }

    #[test]
    fn 填充不会改动位长度() {
        // 第一版把填充也走 `update` —— 于是 `len` 被填充字节改动，
        // 位长度算错而摘要全错。两个入口分开就是为了这个。
        // 这里用"空输入"来暴露它：空输入的位长度必须是 0。
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    // ───────────────── 校验器 ─────────────────

    #[test]
    fn 只接受合法的四十位十六进制() {
        assert!(Sha1Verifier::new("da39a3ee5e6b4b0d3255bfef95601890afd80709").is_some());
        // 大写也接受并规范化
        assert_eq!(
            Sha1Verifier::new("DA39A3EE5E6B4B0D3255BFEF95601890AFD80709")
                .unwrap()
                .expected(),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        // 非法的一律拒绝 —— 一个拼错的期望哈希会让**每一份正确文件都被判为不匹配**
        for bad in [
            "",
            "abc",
            "da39a3ee5e6b4b0d3255bfef95601890afd8070",
            "g".repeat(40).as_str(),
        ] {
            assert!(Sha1Verifier::new(bad).is_none(), "应当拒绝 {bad:?}");
        }
    }

    #[test]
    fn 文件摘要与内存摘要一致() {
        let d = std::env::temp_dir().join(format!("qul-sha1-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("x.bin");
        let content = b"hello world";
        std::fs::write(&p, content).unwrap();
        assert_eq!(sha1_file(&p).unwrap(), sha1_hex(content));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 校验器对不匹配的文件报出两个哈希() {
        use crate::download::Verifier;
        let d = std::env::temp_dir().join(format!("qul-sha1b-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("y.bin");
        std::fs::write(&p, b"actual").unwrap();
        let v = Sha1Verifier::new("da39a3ee5e6b4b0d3255bfef95601890afd80709").unwrap();
        let e = v.verify(&p).unwrap_err();
        assert!(e.contains("期望"), "{e}");
        assert!(e.contains("实际"), "必须报出实际值（否则无法定位）：{e}");
        assert_eq!(v.name(), "SHA-1");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 清单校验器拒绝非法哈希并在构造时就报错() {
        // 让"清单里有一个拼错的哈希"变成**构造期的错误**，
        // 而不是某一次下载失败时的谜题。
        let e = ManifestVerifier::new(vec![("a.jar".into(), "nope".into())]).unwrap_err();
        assert!(e.contains("a.jar"), "{e}");

        let m = ManifestVerifier::new(vec![
            (
                "a.jar".into(),
                "da39a3ee5e6b4b0d3255bfef95601890afd80709".into(),
            ),
            (
                "b.jar".into(),
                "a9993e364706816aba3e25717850c26c9cd0d89d".into(),
            ),
        ])
        .unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(
            m.expected_of("b.jar"),
            Some("a9993e364706816aba3e25717850c26c9cd0d89d")
        );
        assert_eq!(m.expected_of("nope"), None);
    }

    #[test]
    fn 清单校验器按清单里的哈希校验() {
        let d = std::env::temp_dir().join(format!("qul-sha1c-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let p = d.join("z.bin");
        std::fs::write(&p, b"abc").unwrap();
        let m = ManifestVerifier::new(vec![(
            "z.bin".into(),
            "a9993e364706816aba3e25717850c26c9cd0d89d".into(),
        )])
        .unwrap();
        assert!(m.verify_one("z.bin", &p).is_ok());
        // 不在清单里的文件会被拒绝 —— 而不是"没找到就跳过"
        assert!(m.verify_one("nope.bin", &p).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 看起来像_sha1_的判断() {
        assert!(looks_like_sha1("da39a3ee5e6b4b0d3255bfef95601890afd80709"));
        assert!(looks_like_sha1("DA39A3EE5E6B4B0D3255BFEF95601890AFD80709"));
        assert!(!looks_like_sha1(""));
        assert!(!looks_like_sha1("da39a3ee5e6b4b0d3255bfef95601890afd8070"));
        assert!(!looks_like_sha1(&"z".repeat(40)));
    }
}
