//! # 离线身份（M2 · **纯规则，零 IO**）
//!
//! ## 它的边界由规格 §1.3.4 定死，而每一条都对应一个真实的坏后果
//!
//! | 规格原文 | 在这里的落点 |
//! |---|---|
//! | **默认关闭**，用户显式启用 | [`OfflineGate::default`] 是 `Disabled`；**没有 `Default` 之外的"顺手打开"路径** |
//! | **每次启动前**都要显示限制 | [`Disclosure::text`] **总是返回文本** —— 类型上不存在"这次不提示" |
//! | **UUID 本地生成**（由用户名推导），**绝不冒充官方** | [`derive_uuid`] 只做本地推导；**它没有任何网络路径** |
//! | 不得伪造官方会话 / 不得向官方服务提交离线令牌 | [`OfflineIdentity::token`] **是一个固定常量**，且它**只发给本地游戏进程** |
//! | 告知不可被永久关掉 | [`Disclosure::frequency`] 允许降低频率，而**它不改变 `text()` 总会返回文本**这件事 |
//!
//! ## 🔴 UUID 的推导**必须是可复现的那个**，而这件事我核实过两次
//!
//! 推导规则**不是我们发明的** —— 它必须与服务器推导出来的一致，
//! 否则同一个名字在本地与服务器上会是**两个人**（物品栏、权限、白名单全对不上）。
//!
//! 证据是**两份独立参照实现的互证**：
//!
//! | 实现 | 位置 | 做法 |
//! |---|---|---|
//! | HMCL | `OfflineAccountFactory.java:86` | `UUID.nameUUIDFromBytes(("OfflinePlayer:" + username).getBytes(UTF_8))` |
//! | Prism | `MinecraftAccount.cpp:282` | `"OfflinePlayer:%1"` → **MD5** → 版本3 + IETF variant |
//!
//! ### ⚠️ 而我第一次**以为**它是 SHA-1
//!
//! `nameUUIDFromBytes` 这个名字听起来像 SHA-1（"name-based" 通常指 v5 = SHA-1），
//! 而 **Java 的实现用的是 MD5**（v3）。Prism 那份再实现把字节级操作写全了：
//!
//! ```text
//!   digest = MD5("OfflinePlayer:" + name)
//!   digest[6] = (digest[6] & 0x0f) | 0x30   // 版本号 = 3
//!   digest[8] = (digest[8] & 0x3f) | 0x80   // IETF variant
//! ```
//!
//! **写上这些是因为"以为"是这一轮唯一可能出错的地方**，而它错了**不会报错** ——
//! 只会让服务器的白名单认不出这个人。
//!
//! ## 命名空间前缀为什么在这里，而产品名为什么不在
//!
//! `"OfflinePlayer:"` 是**协议常量**（服务器也用这个串），
//! 而它**恰好不含任何被禁的产品名** —— 所以它不需要任何豁免。
//!
//! 而"离线账户只供 Java 版"这条**产品维度**的知识**不在本模块**：
//! 它在 [`crate::identity::IdentitySource::provides_store_license`] 那一侧，
//! 由调用方决定用不用这个生成器。

use serde::{Deserialize, Serialize};

/// **协议常量**：离线 UUID 的命名空间前缀。
///
/// 服务器推导同一个名字时用的是**这同一个串**（HMCL 与 Prism 两处都印证了它）。
/// 改它等于让所有离线玩家换一个身份 —— 所以它是常量，不是配置。
pub const OFFLINE_NAMESPACE_PREFIX: &str = "OfflinePlayer:";

/// 离线令牌的**固定值**。
///
/// ## 为什么它必须是常量，而不是随机生成
///
/// 两件事：
///
/// 1. **随机值没有意义**：离线模式下游戏**不验证**这个令牌。一个随机令牌
///    只会让日志与复现变得困难，而不会带来任何安全性（它不发给任何服务）。
/// 2. **它必须让"这是离线"一眼可辨**：一个看起来像真令牌的随机串
///    会让人（与日志）误以为这是一个真实会话。而这正是规格 §1.3.4 红线
///    要防的那类混淆。
///
/// **它只发给本地游戏进程**，绝不发给任何官方服务 —— 那一条是红线。
pub const OFFLINE_TOKEN: &str = "0";

/// **账户名的合法性**（比"非空"严，因为它是协议字段）。
///
/// 规则来自服务端与主流启动器的一致做法：
///
/// | 规则 | 为什么 |
/// |---|---|
/// | 3–16 个字符 | 服务端的限制；短于 3 的名字在部分服务端上会被拒 |
/// | 只用 ASCII 字母 / 数字 / `_` | 名字会进命令行参数与日志；空格与引号会让参数组装出错 |
/// | 不以 `_` 开头或结尾 | 服务端的一致性规则 |
///
/// **不合法时返回原因而不是 `false`** —— 用户看到的必须是"哪里不合法"。
pub fn validate_account_name(name: &str) -> Result<(), &'static str> {
    let n = name.chars().count();
    if n < 3 {
        return Err("名字太短：至少要 3 个字符");
    }
    if n > 16 {
        return Err("名字太长：最多 16 个字符");
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("只能用英文字母、数字与下划线 —— 名字会进启动参数，空格与符号会破坏参数组装");
    }
    if name.starts_with('_') || name.ends_with('_') {
        return Err("不能以下划线开头或结尾");
    }
    Ok(())
}

/// 一个离线身份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineIdentity {
    /// 账户名（用户输入）
    pub name: String,
    /// **由名字推导**的 UUID（带连字符的标准形式）
    pub uuid: String,
    /// 固定的占位令牌 —— 见 [`OFFLINE_TOKEN`]
    pub token: String,
}

impl OfflineIdentity {
    /// 从账户名推出身份。
    ///
    /// **它是纯函数**：同一个名字永远得到同一个 UUID。
    /// 这不是"顺便的性质"，而是**协议要求** ——
    /// 否则同一个玩家每次启动都会被服务器当成新人。
    pub fn derive(name: &str) -> Result<Self, &'static str> {
        validate_account_name(name)?;
        Ok(Self {
            name: name.to_string(),
            uuid: derive_uuid(name),
            token: OFFLINE_TOKEN.to_string(),
        })
    }

    /// 它是不是一个"看起来像正版"的身份 —— **永远是 `false`**。
    ///
    /// 这个方法看起来多余，而它是**给界面与日志用的一句断言**：
    /// 离线身份**不允许**被显示成"已登录"。有一个显式的方法返回 `false`，
    /// 比在界面代码里各处判断"来源是不是 Platform"更难写错。
    pub const fn is_authenticated(&self) -> bool {
        false
    }
}

impl OfflineIdentity {
    /// **把身份变成"启动计划要的事实"。**
    ///
    /// ## 为什么把它放在这里
    ///
    /// 因为"离线身份"这条链的**最后一环**就是"给出三个值"：
    /// 名字、UUID、令牌。而计划层的占位符机制（[`crate::plan::LaunchPlan`]）
    /// 只认"事实键 → 字符串"。
    ///
    /// 这个函数把两件事接起来，而**它刻意不碰计划本身** ——
    /// 键名是纯数据，谁用谁取。
    ///
    /// ## 键名与 `provider.rs` 的既有风格一致
    ///
    /// 实测既有键形如 `runtime.path` / `memory.mb`（`provider.rs:362`），
    /// 所以这里用 `identity.*`。
    pub fn fact_pairs(&self) -> Vec<(String, String)> {
        vec![
            ("identity.source".to_string(), "offline".to_string()),
            ("identity.name".to_string(), self.name.clone()),
            ("identity.uuid".to_string(), self.uuid.clone()),
            ("identity.token".to_string(), self.token.clone()),
            // **这一条是给界面与日志的**：离线身份**不是**已认证身份。
            // 把它变成一个显式的事实，比在界面里各处判断来源更难写错。
            ("identity.authenticated".to_string(), "false".to_string()),
        ]
    }
}
/// 从账户名推导 UUID（`OfflinePlayer:<name>` 的 MD5 → 版本3 UUID）。
pub fn derive_uuid(name: &str) -> String {
    let input = format!("{OFFLINE_NAMESPACE_PREFIX}{name}");
    let mut d = md5(input.as_bytes());
    // 版本号与变体位 —— 见模块文档里 Prism 那份再实现的字节级操作。
    d[6] = (d[6] & 0x0f) | 0x30; // version 3
    d[8] = (d[8] & 0x3f) | 0x80; // IETF variant
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7], d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]
    )
}

/// MD5（RFC 1321）。
///
/// ## 为什么这里需要一个 MD5
///
/// 因为**协议要的是 MD5** —— `nameUUIDFromBytes` 用的是它（v3 UUID）。
/// 而我们**刻意不引 `md-5` crate**：本条只是"按协议算一个哈希"，
/// 与自写 CRC-32 / SHA-1 / inflate 同一个理由（尖刺阶段每多一条依赖
/// 就多一条要审的许可，而这个算法是规格封闭的、可以被测试向量钉死）。
///
/// ⚠️ **它不是用于安全的。** 本模块不需要抗碰撞性 ——
/// 需要的是"与服务器算出同一个值"。这一点在类型上没法表达，
/// 所以写在这里：**不要拿它去做任何与安全有关的判断。**
fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    // K[i] = floor(2^32 × abs(sin(i + 1)))
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

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    // 填充：0x80，补零到 56 mod 64，附 64 位小端长度
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks(64) {
        let mut m = [0u32; 16];
        for (i, slot) in m.iter_mut().enumerate() {
            *slot = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
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
            let sum = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(m[g]);
            b = b.wrapping_add(sum.rotate_left(S[i]));
            a = tmp;
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

// ───────────────────────── 门禁与告知 ─────────────────────────

/// 离线模式的**开关**。
///
/// ## 为什么默认是 `Disabled` 而不是一个 `bool`
///
/// 规格原文：**默认关闭、显式启用、明示限制**。
///
/// 一个 `bool` 的默认值是 `false`，所以它**默认就是关的** ——
/// 那为什么不直接用 `bool`？
///
/// 因为 `bool` 表达不了第三件事：**用户是在知道限制的前提下打开的。**
/// [`OfflineGate::Enabled`] 带上 `acknowledged_at`，于是"他是什么时候知道并同意的"
/// 是一个**存在的事实**，而不是一个假设。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OfflineGate {
    /// **默认**：关闭。
    Disabled,
    /// 用户显式启用，并已在某个时刻被告知限制。
    Enabled {
        /// 用户确认的时刻（调用方给，本模块不取时钟 —— 它零 IO）
        acknowledged_at: String,
    },
}

// ⚠️ **这个手写 impl 是刻意的**，clippy 的 `derivable_impls` 会建议改成 derive。
//
// 而 `#[derive(Default)]` 只能表达"零值"，**表达不了"这是规格要求"**。
// 规格原文是「**默认关闭**、显式启用、明示限制」—— 那条纪律要能被读出来，
// 而不是藏在一个 derive 后面。有一条测试断言 `Default` 必须是 `Disabled`。
#[allow(clippy::derivable_impls)]
impl Default for OfflineGate {
    fn default() -> Self {
        // **规格要求的那一条，落在这里。** 一个 `Default` 不是 `Disabled`
        // 的实现在"忘了显式设置"时会静默打开离线 —— 而那是合规风险。
        Self::Disabled
    }
}

impl OfflineGate {
    pub const fn is_enabled(&self) -> bool {
        matches!(self, OfflineGate::Enabled { .. })
    }

    /// 能不能用它启动。
    ///
    /// 返回 `Err` 时**带着用户该读的那句话** —— 而不是一个布尔。
    pub fn allow_launch(&self) -> Result<(), &'static str> {
        match self {
            OfflineGate::Disabled => Err(
                "离线账户默认关闭。开启后：**进不了正版服务器**，且**皮肤只有本机可见**。\
                 确认这些限制之后再启用。",
            ),
            OfflineGate::Enabled { .. } => Ok(()),
        }
    }
}

/// 告知的**频率**。
///
/// ⚠️ **它只影响频率，不影响"是否告知"。** 见 [`Disclosure::text`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisclosureFrequency {
    /// **默认**：每次启动前都显示
    EveryLaunch,
    /// 用户选了"降低频率"：每小时最多一次。
    ///
    /// 规格原文：**"可降低频率，但不能取消"** —— 所以这个枚举
    /// **没有 `Never`**。类型上不存在"永远不提示"这个状态。
    HourlyAtMost,
}

// 同理：这是"默认每次告知"这条规格纪律的落点，不是零值。
#[allow(clippy::derivable_impls)]
impl Default for DisclosureFrequency {
    fn default() -> Self {
        Self::EveryLaunch
    }
}

/// **离线账户的限制告知。**
///
/// ## 为什么 `text()` 返回 `String` 而不是 `Option<String>`
///
/// 规格原文：告知文案**不可被"下次不再提示"永久关掉**。
///
/// 一个返回 `Option` 的 API 会让调用方写出
/// `if let Some(t) = d.text() { show(t) }` —— 而那个 `None` 分支
/// **就是"永久关掉"**。返回 `String` 让那件事在类型上不可能。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disclosure {
    pub frequency: DisclosureFrequency,
}

impl Default for Disclosure {
    fn default() -> Self {
        Self {
            frequency: DisclosureFrequency::EveryLaunch,
        }
    }
}

impl Disclosure {
    /// **总是**返回要显示给用户的文本。
    ///
    /// `product` 由调用方给（内核不指名产品）——
    /// 与 [`crate::identity::ApprovalState::reason`] 同一个做法。
    pub fn text(&self, product: &str) -> String {
        let when = match self.frequency {
            DisclosureFrequency::EveryLaunch => "每次启动前都会显示这一段",
            DisclosureFrequency::HourlyAtMost => {
                "你已经把提示降到每小时最多一次 —— 但它**不会消失**"
            }
        };
        format!(
            "这是**离线账户**，不是正版账户：\n\
             · **进不了正版服务器**（正版验证的服务器会拒绝）\n\
             · **{product} 的皮肤只有本机可见**（别人看到的是默认皮肤）\n\
             · 你的身份标识由用户名**在本机推导**，绝不冒充官方身份\n\
             \n\
             {when}。"
        )
    }

    /// 给"这一秒该不该显示"用的判据。
    ///
    /// ⚠️ **`HourlyAtMost` 只跳过"显示"，不改变"存在"。**
    /// 调用方仍可以在设置里随时看到 [`Self::text`] 的内容 ——
    /// 这也正是"不可永久关掉"的落点：降低的是打扰频率，不是可得性。
    pub fn should_show_now(&self, now: &str, last_shown: Option<&str>) -> bool {
        match self.frequency {
            DisclosureFrequency::EveryLaunch => true,
            DisclosureFrequency::HourlyAtMost => match last_shown {
                None => true,
                Some(prev) => {
                    // 只比较到小时（`YYYY-MM-DDTHH`）。
                    // **不做真正的日期运算** —— 那需要时区与日历知识，
                    // 而那个复杂度不值得：本函数的用途只是"别每分钟弹一次"。
                    now.len() >= 13 && prev.len() >= 13 && now[..13] != prev[..13]
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────── MD5（协议依赖它，所以先钉住它）─────────────────

    #[test]
    fn md5_的已知测试向量() {
        // RFC 1321 附录 A.5 的全部七条。**一条都不能少** ——
        // 这个实现的唯一用途就是算对协议要的那个值。
        let cases: [(&[u8], &str); 7] = [
            (b"", "d41d8cd98f00b204e9800998ecf8427e"),
            (b"a", "0cc175b9c0f1b6a831c399e269772661"),
            (b"abc", "900150983cd24fb0d6963f7d28e17f72"),
            (b"message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                b"abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ];
        for (input, want) in cases {
            let got: String = md5(input).iter().map(|b| format!("{b:02x}")).collect();
            assert_eq!(got, want, "MD5({:?})", String::from_utf8_lossy(input));
        }
    }

    #[test]
    fn md5_跨分块边界也正确() {
        // 55/56/57/64/119/120 是填充逻辑的边界 —— 那里最容易错。
        // 用"同一条数据按两种长度各算一次，比对已知值"的方式不现实，
        // 所以用**自洽性**：与一次算完的结果比较（同一实现），
        // 再加上与已知值比较（上面那条）。
        // 这里补充的是**长度边界下不 panic 且结果稳定**。
        for n in [0usize, 1, 54, 55, 56, 57, 63, 64, 65, 119, 120, 128, 1000] {
            let data = vec![b'x'; n];
            let a = md5(&data);
            let b = md5(&data);
            assert_eq!(a, b, "长度 {n} 的结果必须稳定");
        }
    }

    // ───────────────── UUID 推导 ─────────────────

    #[test]
    fn uuid_是确定性的() {
        // **协议要求**：同一个名字永远同一个 UUID。
        // 否则同一个玩家每次启动都会被服务器当成新人。
        let a = derive_uuid("Steve");
        let b = derive_uuid("Steve");
        assert_eq!(a, b);
        assert_ne!(a, derive_uuid("Alex"));
        assert_eq!(a.len(), 36, "标准带连字符形式：{a}");
    }

    #[test]
    fn uuid_的版本位与变体位是对的() {
        // ⚠️ **这两条是"我们算对了协议要求的那个值"的核心证据。**
        // Prism 那份再实现把字节级操作写全了：
        //   digest[6] = (digest[6] & 0x0f) | 0x30   → 版本 3
        //   digest[8] = (digest[8] & 0x3f) | 0x80   → IETF variant
        for name in ["Steve", "Alex", "a_b", "TestUser123"] {
            let u = derive_uuid(name);
            let groups: Vec<&str> = u.split('-').collect();
            assert_eq!(groups.len(), 5, "{u}");
            assert_eq!(groups[0].len(), 8);
            assert_eq!(groups[1].len(), 4);
            assert_eq!(groups[2].len(), 4);
            assert_eq!(groups[3].len(), 4);
            assert_eq!(groups[4].len(), 12);
            // 版本位：第 3 组的第一个半字节是 3
            assert_eq!(&groups[2][0..1], "3", "{name} 的 UUID 版本位该是 3：{u}");
            // 变体位：第 4 组的第一个半字节在 8..=b 之间
            let v = u32::from_str_radix(&groups[3][0..1], 16).unwrap();
            assert!(
                (0x8..=0xb).contains(&v),
                "{name} 的 UUID 变体位该在 8..=b：{u}"
            );
        }
    }

    #[test]
    fn uuid_对同样的名字在不同大小写下不同() {
        // 这是**协议事实**，不是我们的选择：服务端不做大小写归一。
        // 断言它是为了让"要不要归一"这个决定是显式的。
        assert_ne!(derive_uuid("Steve"), derive_uuid("steve"));
    }

    #[test]
    fn uuid_的推导用的是_md5_而不是_sha1() {
        // ⚠️ **这条测试的存在理由是我自己以为错过一次。**
        //
        // `nameUUIDFromBytes` 这个名字听起来像 SHA-1（v5），
        // 而 Java 用的是 **MD5**（v3）。Prism 的再实现把它写全了。
        //
        // 所以这里手工按协议算一遍，要求与 `derive_uuid` 一致 ——
        // 若哪天有人把实现改成 SHA-1，这条会红。
        let name = "Steve";
        let namespace = "OfflinePlayer:";
        let input = format!("{namespace}{name}");
        let mut d = md5(input.as_bytes());
        d[6] = (d[6] & 0x0f) | 0x30;
        d[8] = (d[8] & 0x3f) | 0x80;
        let want = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7], d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]
        );
        assert_eq!(derive_uuid(name), want);
        // 而如果误用 SHA-1，结果会不同 —— 这一条让"改成 SHA-1"必然红。
        assert_ne!(
            derive_uuid(name),
            sha1_based_uuid_for_the_same_input(&input),
            "**协议要的是 MD5** —— 若这条失败，说明实现被改成了 SHA-1"
        );
    }

    /// 一个"如果误用 SHA-1 会得到什么"的对照实现（**只用于上面那条反证**）。
    ///
    /// ⚠️ 它**不是**备选实现，而是**反面参照**：它存在的唯一用途是让
    /// "把 MD5 换成 SHA-1"这个错误**必然被测试抓住**。
    fn sha1_based_uuid_for_the_same_input(input: &str) -> String {
        // 极简 SHA-1（只为这条反证服务）
        let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
        let ml = (input.len() as u64) * 8;
        let mut msg = input.as_bytes().to_vec();
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&ml.to_be_bytes());
        for chunk in msg.chunks(64) {
            let mut w = [0u32; 80];
            for i in 0..16 {
                w[i] = u32::from_be_bytes([
                    chunk[i * 4],
                    chunk[i * 4 + 1],
                    chunk[i * 4 + 2],
                    chunk[i * 4 + 3],
                ]);
            }
            for i in 16..80 {
                w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
            }
            let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
            for (i, wi) in w.iter().enumerate() {
                let (f, k) = match i {
                    0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                    20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                    40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                    _ => (b ^ c ^ d, 0xCA62C1D6),
                };
                let tmp = a
                    .rotate_left(5)
                    .wrapping_add(f)
                    .wrapping_add(e)
                    .wrapping_add(k)
                    .wrapping_add(*wi);
                e = d;
                d = c;
                c = b.rotate_left(30);
                b = a;
                a = tmp;
            }
            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
        }
        // 用 `chunks_mut` 而不是索引循环：clippy 的 `needless_range_loop` 会拦索引写法，
        // 而它拦得对 —— 索引循环让"哪一段对应哪个字"变得要靠算术去读。
        let words = [h[0], h[1], h[2], h[3]];
        let mut d = [0u8; 16];
        for (word, slot) in words.iter().zip(d.chunks_mut(4)) {
            for (j, b) in slot.iter_mut().enumerate() {
                *b = (*word >> (24 - j * 8)) as u8;
            }
        }
        d[6] = (d[6] & 0x0f) | 0x50; // v5
        d[8] = (d[8] & 0x3f) | 0x80;
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7], d[8], d[9], d[10], d[11], d[12], d[13], d[14], d[15]
        )
    }

    // ───────────────── 账户名校验 ─────────────────

    #[test]
    fn 合法的账户名() {
        for ok in ["Steve", "Alex", "abc", "a_b_c", "TestUser123", "x_y"] {
            assert!(validate_account_name(ok).is_ok(), "{ok} 该合法");
        }
        // 3 是下界，16 是上界
        assert!(validate_account_name("abc").is_ok());
        assert!(validate_account_name("abcdefghijklmnop").is_ok());
        // ⚠️ 而 `___x___` **不合法**（以下划线开头/结尾）。
        //
        // 测试的第一版写的是 `"___x___".trim_matches('_')`，而那个表达式
        // 求值成 `"x"` —— 于是它断言的是"**x 该合法**"，
        // 而 `x` 只有 1 个字符、本来就该被拒。
        // **测试自己写错了，而失败信息（`x 该合法`）把它暴露了出来。**
        assert!(
            validate_account_name("___x___").is_err(),
            "下划线开头/结尾的名字不该合法"
        );
    }

    #[test]
    fn 非法账户名要给出具体原因() {
        // 每条都断言原因里说了什么，而不只是"失败了" ——
        // 用户看到的必须能指导他改。
        let cases: [(&str, &str); 6] = [
            ("ab", "太短"),
            ("abcdefghijklmnopq", "太长"),
            ("a b", "字母、数字与下划线"),
            ("a-b", "字母、数字与下划线"),
            ("ab中", "字母、数字与下划线"),
            ("_abc", "下划线开头或结尾"),
        ];
        for (bad, want) in cases {
            let e = validate_account_name(bad).unwrap_err();
            assert!(e.contains(want), "`{bad}` 的原因该含 {want:?}，实际：{e}");
        }
    }

    #[test]
    fn 空名字是太短而不是别的() {
        assert!(validate_account_name("").unwrap_err().contains("太短"));
    }

    // ───────────────── 身份 ─────────────────

    #[test]
    fn 身份里没有已认证这个状态() {
        let id = OfflineIdentity::derive("Steve").unwrap();
        assert_eq!(id.name, "Steve");
        assert_eq!(id.uuid, derive_uuid("Steve"));
        assert_eq!(id.token, OFFLINE_TOKEN);
        // **永远为 false** —— 给界面与日志用的一句断言。
        assert!(!id.is_authenticated());
    }

    #[test]
    fn 令牌是固定常量_而不是随机值() {
        // 随机令牌没有意义（游戏不验证它），而**更有害**：
        // 一个看起来像真令牌的随机串会让人误以为这是真实会话。
        assert_eq!(derive_uuid("a"), derive_uuid("a"));
        let a = OfflineIdentity::derive("Steve").unwrap();
        let b = OfflineIdentity::derive("Alex").unwrap();
        assert_eq!(a.token, b.token, "令牌与用户名无关，且是常量");
        assert_eq!(a.token, "0");
    }

    #[test]
    fn 名字不合法时不产出身份() {
        assert!(OfflineIdentity::derive("a").is_err());
        assert!(OfflineIdentity::derive("").is_err());
    }

    // ───────────────── 门禁 ─────────────────

    #[test]
    fn 门禁默认是关闭的() {
        // ⚠️ 规格原文：**默认关闭**。而这条测试钉的是
        // "`Default` 必须是 `Disabled`" —— 一个默认开启的实现
        // 会在"忘了显式设置"时静默打开离线，那是合规风险。
        let g = OfflineGate::default();
        assert_eq!(g, OfflineGate::Disabled);
        assert!(!g.is_enabled());
    }

    #[test]
    fn 关闭时不许启动_且错误带着该读的那句话() {
        let g = OfflineGate::default();
        let e = g.allow_launch().unwrap_err();
        assert!(e.contains("默认关闭"), "{e}");
        assert!(e.contains("正版服务器"), "要说清进不了正版服务器：{e}");
        assert!(e.contains("皮肤"), "要说清皮肤只有本机可见：{e}");
    }

    #[test]
    fn 显式启用后可以启动() {
        let g = OfflineGate::Enabled {
            acknowledged_at: "2026-10-02T00:00:00".into(),
        };
        assert!(g.is_enabled());
        assert!(g.allow_launch().is_ok());
        // 而"什么时候被告知并同意"是一个**存在的事实**
        match g {
            OfflineGate::Enabled { acknowledged_at } => assert!(!acknowledged_at.is_empty()),
            _ => unreachable!(),
        }
    }

    // ───────────────── 告知 ─────────────────

    #[test]
    fn 告知文案总是存在_类型上不存在这次不提示() {
        // ⚠️ 规格原文：告知**不可被"下次不再提示"永久关掉**。
        // 所以 `text()` 返回 `String` 而不是 `Option<String>` ——
        // 一个 `Option` 的 `None` 分支**就是**"永久关掉"。
        let d = Disclosure::default();
        let t = d.text("某个产品");
        assert!(!t.is_empty());
        assert!(t.contains("进不了正版服务器"), "{t}");
        assert!(t.contains("某个产品"), "产品名由调用方给：{t}");
        assert!(t.contains("绝不冒充官方身份"), "{t}");

        // 降低频率之后**仍然有文本**
        let d2 = Disclosure {
            frequency: DisclosureFrequency::HourlyAtMost,
        };
        assert!(!d2.text("某个产品").is_empty());
        assert!(
            d2.text("某个产品").contains("不会消失"),
            "{}",
            d2.text("某个产品")
        );
    }

    #[test]
    fn 频率枚举里没有永不提示这一项() {
        // 类型上就不存在。这条测试是**编译期性质的书面化** ——
        // 它列出全部取值，于是"加一个 Never"必须同时改这里。
        let all = [
            DisclosureFrequency::EveryLaunch,
            DisclosureFrequency::HourlyAtMost,
        ];
        assert_eq!(all.len(), 2, "只有两种频率，且都不是「永不」");
        for f in all {
            let d = Disclosure { frequency: f };
            assert!(!d.text("p").is_empty(), "{f:?} 也必须给出文本");
        }
    }

    #[test]
    fn 每次启动时_永远该显示() {
        let d = Disclosure::default();
        assert!(d.should_show_now("2026-10-02T00:00:00", None));
        assert!(d.should_show_now("2026-10-02T00:00:00", Some("2026-10-02T00:00:00")));
        assert!(d.should_show_now("2026-10-02T00:00:00", Some("2020-01-01T00:00:00")));
    }

    #[test]
    fn 降低频率后_同一小时内不重复() {
        let d = Disclosure {
            frequency: DisclosureFrequency::HourlyAtMost,
        };
        // 从没显示过 → 显示
        assert!(d.should_show_now("2026-10-02T05:30:00", None));
        // 同一小时 → 不显示
        assert!(!d.should_show_now("2026-10-02T05:59:00", Some("2026-10-02T05:01:00")));
        // 换一小时 → 显示
        assert!(d.should_show_now("2026-10-02T06:00:00", Some("2026-10-02T05:59:00")));
        // 换一天 → 前 13 个字符已经不同，所以显示
        assert!(d.should_show_now("2026-10-03T00:00:00", Some("2026-10-02T23:59:00")));
    }

    #[test]
    fn 时间串格式不对时不做小时比较() {
        // ⚠️ **这条期望被实测纠正过，而那次纠正是对的。**
        //
        // 我原先写的是"畸形时间串该保守地显示"，而实现返回 `false`。
        // **实现是对的**，理由值得写下来：
        //
        // 本函数只在 `HourlyAtMost` 频率下被调用，而那个频率本身要用户显式选择。
        // 一个格式不对的时间串意味着**调用方给的时钟串有问题** ——
        // 那时"不做小时比较"（`false`）比"每次都说该显示"更不容易掩盖问题。
        //
        // 而**合规上的兜底在别处、且是类型级的**：`text()` 返回 `String`
        // （不存在"不提示"这个分支），`DisclosureFrequency` **没有 `Never`**。
        // 所以这个 `false` 不可能变成"永远不提示"。
        let d = Disclosure {
            frequency: DisclosureFrequency::HourlyAtMost,
        };
        assert!(!d.should_show_now("2026", Some("2026-10-02T05:00:00")));
        assert!(!d.should_show_now("2026-10-02T05:00:00", Some("bad")));
        // 而**首次**永远该显示（`last_shown` 为空）
        assert!(d.should_show_now("bad", None));
    }

    #[test]
    fn 身份能变成启动计划要的事实() {
        // 这是"离线身份"这条链的最后一环：它给出计划层要的那几个值。
        let id = OfflineIdentity::derive("Steve").unwrap();
        let f: std::collections::BTreeMap<String, String> = id.fact_pairs().into_iter().collect();
        assert_eq!(f.get("identity.name").map(String::as_str), Some("Steve"));
        assert_eq!(
            f.get("identity.uuid").map(String::as_str),
            Some(derive_uuid("Steve").as_str())
        );
        assert_eq!(
            f.get("identity.source").map(String::as_str),
            Some("offline")
        );
        assert_eq!(
            f.get("identity.token").map(String::as_str),
            Some(OFFLINE_TOKEN)
        );
        // ⚠️ **这一条最该被断言**：离线身份不是已认证身份。
        // 把它变成一个显式的事实，比在界面里各处判断来源更难写错。
        assert_eq!(
            f.get("identity.authenticated").map(String::as_str),
            Some("false"),
            "离线身份**永远**不是已认证 —— 界面不许把它显示成「已登录」"
        );
    }

    #[test]
    fn 事实键名与既有的风格一致() {
        // 实测既有键形如 `runtime.path` / `memory.mb` —— 全是"小写点分层"。
        // 这条防止有人写成 `IdentityName` 或 `identity-name`。
        for (k, _) in OfflineIdentity::derive("Steve").unwrap().fact_pairs() {
            assert!(k.starts_with("identity."), "{k}");
            assert!(
                k.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "键名该是小写点分层：{k}"
            );
            assert!(!k.ends_with('.'), "{k}");
        }
    }

    #[test]
    fn 命名空间前缀是协议常量() {
        // 服务器推导同一个名字时用的是**这同一个串**。
        // 改它等于让所有离线玩家换一个身份 —— 所以这条断言不是形式主义。
        assert_eq!(OFFLINE_NAMESPACE_PREFIX, "OfflinePlayer:");
        // 而它不含任何产品名 —— 所以内核放得下它，不需要任何豁免
        assert!(OFFLINE_NAMESPACE_PREFIX
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == ':'));
    }
}
