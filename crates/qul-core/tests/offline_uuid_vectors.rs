//! # 离线 UUID 的**跨实现交叉验证**（M2）
//!
//! ## 为什么必须有这个文件
//!
//! 离线 UUID 的推导规则**不是我们发明的** —— 它必须与服务器推导出来的一致，
//! 否则同一个名字在本地与服务器上会是**两个人**（物品栏、权限、白名单全对不上）。
//!
//! 所以它是一个**跨启动器的兼容契约**，而这一点本项目自己的调研早就记下了：
//!
//! > `docs/XMCL-源码调研.md:186` —— 「离线 UUID 的生成规则（`OfflinePlayer:<name>`
//! > 的 MD5 变体）……**跨启动器兼容契约**」
//! >
//! > `docs/XMCL-源码调研.md:309` —— 「**值得独立成模块 + 测试**」
//!
//! ## 三份独立实现的互证
//!
//! | 实现 | 位置 |
//! |---|---|
//! | HMCL | `repos/HMCL/.../OfflineAccountFactory.java:86` |
//! | Prism | `repos/PrismLauncher/.../MinecraftAccount.cpp:282`（含字节级 version/variant 操作） |
//! | XMCL | `repos/XMCL/packages/user-offline-uuid/`（`index.ts` + `index.browser.ts` 两份实现） |
//!
//! 而向量是**在 Node 里用 XMCL 的实现算出来的**，且算了两遍
//! （Node crypto 版 vs 手写 MD5 版，两者必须一致）。
//! 见 `spikes/m2-offline-uuid/xmcl-cross-check.cjs`。
//!
//! ## ⚠️ 而它们都是 MD5（v3），不是 SHA-1（v5）
//!
//! `UUID.nameUUIDFromBytes` 这个名字听起来像 SHA-1 —— **我据此以为错过一次**。
//! Prism 那份再实现把这个判据钉死了：
//!
//! ```text
//!   digest = MD5("OfflinePlayer:" + name)
//!   digest[6] = (digest[6] & 0x0f) | 0x30   // 版本 3
//!   digest[8] = (digest[8] & 0x3f) | 0x80   // IETF variant
//! ```
//!
//! 下面有一条测试专门断言"**不是** SHA-1" —— 若实现被改成 SHA-1，它会红。

use qul_core::offline::{derive_uuid, OFFLINE_NAMESPACE_PREFIX};
use std::path::PathBuf;

#[derive(Debug, serde::Deserialize)]
struct Vector {
    name: String,
    uuid: String,
}

fn vectors() -> Vec<Vector> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("offline_uuid_vectors.json");
    let text =
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()));
    let raw: Vec<serde_json::Value> =
        serde_json::from_str(&text).expect("向量文件应当是合法 JSON 数组");
    // 跳过第一个元素（它是 `_readme`，不是向量）
    raw.into_iter()
        .filter(|v| v.get("name").is_some())
        .map(|v| serde_json::from_value(v).expect("向量项该有 name 与 uuid"))
        .collect()
}

#[test]
fn vectors_file_is_present_and_complete() {
    // 一个空的向量文件会让下面那条测试**永远通过**。
    let v = vectors();
    assert!(
        v.len() >= 6,
        "向量太少（{}）—— 一个空的或过小的向量集是一条无效的验证",
        v.len()
    );
    for x in &v {
        assert_eq!(x.uuid.len(), 36, "{} 的 UUID 形态不对：{}", x.name, x.uuid);
        assert_eq!(
            x.uuid.chars().filter(|c| *c == '-').count(),
            4,
            "{} 的 UUID 该有 4 个连字符：{}",
            x.name,
            x.uuid
        );
    }
}

#[test]
fn our_derivation_matches_three_independent_implementations() {
    // ⚠️ **这是本文件的全部价值。**
    //
    // 它比对的是"我们算出来的"与"外部实现算出来的"。
    // 若这条通过，那"同一个名字在服务器上也是同一个人"这件事就有了证据 ——
    // 而不是靠我们读了一遍文档。
    let v = vectors();
    for x in &v {
        assert_eq!(
            derive_uuid(&x.name),
            x.uuid,
            "**{} 的 UUID 与外部实现不一致。**\n  \
             我们算出：{}\n  外部实现：{}\n  \
             这意味着那个玩家在服务器上会是另一个人。",
            x.name,
            derive_uuid(&x.name),
            x.uuid
        );
    }
}

#[test]
fn it_is_md5_v3_and_not_sha1_v5() {
    // ⚠️ **这条测试的存在理由是我自己以为错过一次。**
    //
    // `nameUUIDFromBytes` 听起来像 SHA-1（v5），而 Java 用的是 **MD5**（v3）。
    // 判据来自 Prism 那份把字节级操作写全的再实现。
    //
    // 所以这里断言：我们算出来的版本位是 **3**（不是 5）。
    // 若哪天有人"优化"成 SHA-1，这条会红 —— 而**没有它，那个错误不会报错**，
    // 只会让服务器的白名单认不出这个人。
    for x in &vectors() {
        let u = derive_uuid(&x.name);
        let version_nibble = &u[14..15]; // 第 3 组的首位
        assert_eq!(
            version_nibble, "3",
            "**必须是版本 3（MD5）** —— 得到 {version_nibble} 说明实现用了别的哈希：{u}"
        );
    }
}

#[test]
fn determinism_is_the_whole_point() {
    // 同一个名字**永远**得到同一个 UUID。这不是"顺便的性质"，而是协议要求。
    for x in &vectors() {
        for _ in 0..3 {
            assert_eq!(derive_uuid(&x.name), x.uuid);
        }
    }
}

#[test]
fn the_namespace_prefix_is_a_load_bearing_contract_term() {
    // 前缀**不是装饰**：它变了，UUID 就完全变，而那个 UUID 在服务器上
    // 不对应任何人。所以"前缀写对"是这条契约的一半。
    //
    // 判据刻意**不经过** `derive_uuid`（那是自证），而是：
    // **另一个独立实现**（本文件底部的 `md5_hex`）按协议手工算一遍。
    // 两个独立的实现对同一个输入得到同一个结果，才是证据。
    let name = "Steve";
    let expected = vectors()
        .into_iter()
        .find(|v| v.name == name)
        .expect("向量里该有 Steve")
        .uuid;

    let input = format!("{OFFLINE_NAMESPACE_PREFIX}{name}");
    assert_eq!(
        input, "OfflinePlayer:Steve",
        "前缀与名字的拼接形态**必须**是这个 —— 它是契约的原文"
    );

    let manual = uuid_from_md5_hex(&md5_hex(&input));
    assert_eq!(
        manual, expected,
        "手工按协议算出的值该与外部向量一致（这一条把**前缀**也验进去了）"
    );
    assert_eq!(derive_uuid(name), expected);

    // 而前缀变了结果一定变 —— 这条把"改前缀会换身份"写成断言
    let other = format!("OfflinePlayer{name}");
    assert_ne!(
        md5_hex(&input),
        md5_hex(&other),
        "前缀少一个冒号，摘要就不同 —— 这就是它为什么是契约的一半"
    );
}

/// 把一份 32 位十六进制摘要按协议变成 UUID 串（版本3 + IETF variant）。
fn uuid_from_md5_hex(hex: &str) -> String {
    let mut bytes: Vec<u8> = hex
        .as_bytes()
        .chunks(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// 独立的 MD5（只为"手工算一遍"服务）。
///
/// ⚠️ 它**不是**被验证的实现 —— 它是**另一个**实现。
fn md5_hex(input: &str) -> String {
    let mut h: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let mut msg = input.as_bytes().to_vec();
    let bits = (input.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_le_bytes());
    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, s) in m.iter_mut().enumerate() {
            *s = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let tmp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(K[i])
                    .wrapping_add(m[g])
                    .rotate_left(S[i]),
            );
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
    }
    let mut out = String::new();
    for w in h {
        for b in w.to_le_bytes() {
            out.push_str(&format!("{b:02x}"));
        }
    }
    out
}

#[test]
fn the_independent_md5_agrees_with_the_known_vectors() {
    // ⚠️ 上面那个"独立 MD5"若自己就是错的，那它作为对照毫无价值 ——
    // 而它的错误会让 `the_namespace_prefix_is_a_load_bearing_contract_term`
    // **恰好在两边都错的时候通过**。
    //
    // 所以先钉住它自己。RFC 1321 附录 A.5 的两条。
    assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(
        md5_hex("OfflinePlayer:Steve").len(),
        32,
        "长度该是 32 个十六进制字符"
    );
}
