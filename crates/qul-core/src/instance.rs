//! # 实例的**通用模型**（M1 最后一项 · **纯规则，零 IO**）
//!
//! ## 这个模型要回答的问题
//!
//! > **实例是什么？** —— 而答案必须能同时容纳 Java 版、基岩版，
//! > 以及**以后与 MC 无关的产品**。
//!
//! 那意味着它**不能是"一个 `.minecraft` 目录的包装"**。否则：
//!
//! | 藏在那个假设里的东西 | 它会在什么情形下崩 |
//! |---|---|
//! | 实例一定有 `versions/` | 基岩版没有 |
//! | 实例一定有 `.jar` | 基岩版没有 |
//! | 实例一定有"启动器配置文件" | 别的产品形态不同 |
//! | 实例一定用"游戏目录 = 根目录" | UWP 形态的目录由系统管 |
//!
//! 所以本模块的分解是：
//!
//! ```text
//!     Instance                    ← 身份 + 目录 + 状态（**与产品无关**）
//!       ├─ id        : InstanceId       稳定标识
//!       ├─ key       : String           目录名（**也是用户的重命名对象**）
//!       ├─ name      : String           显示名
//!       ├─ dir       : RelPath          相对实例根
//!       ├─ product   : ProductRef       **哪个产品的哪个变体**
//!       └─ state     : InstanceState    生命周期
//!     ProductRef { product_key, version, variant }
//!       └─ product_key → 去 ProductRegistry 查它有什么能力
//! ```
//!
//! **产品专属的维度（光影 / 行为包 / 皮肤包 / 加载器）不在本模块** ——
//! 它们在 [`crate::caps::InstanceDetail`] 上。本模块只放"任何产品都成立"的东西。
//!
//! ## 🔴 两个被刻意否定的设计
//!
//! ### ① ID 不是随机生成的，它是 `key` 的**函数**
//!
//! 引 `uuid` 生成一个随机 ID 很容易，而它有一个后果：
//! **"这个 ID 还对得上那个目录吗"变成一个无法回答的问题。**
//!
//! 本模块的做法是 `id = fnv1a64(key)` —— 于是：
//!
//! - [ `Instance::check_integrity()` ] 能发现 **"ID 与 key 不符"** 这个状态；
//! - "两个实例的 ID 撞了" 在数学上可忽略（64 位），**且可被测试穷举同组实例**；
//! - 目录名与 ID **互为校验**，而不是两套并行的事实。
//!
//! ### ② **`key` 与 `name` 是两个东西，而只有 `name` 可改**
//!
//! 一个把"显示名"直接当目录名的实现在用户重命名时会面对
//! **"要么改目录（可能撞车/可能失败），要么名实不符"**。
//! 本模块的选择是：**`key` 一经分配不再改变**（它是身份），
//! `name` 可随便改（它是标签）。
//!
//! 这与 ADR-0013 那条纪律同源：**"身份"与"标签"混淆之后，
//! 每一次改名都在冒丢失身份的风险。**

use crate::layout::RelPath;
use serde::{Deserialize, Serialize};

/// **稳定的实例标识。**
///
/// 形态：`inst-` + 16 位小写十六进制（`fnv1a64(key)`）。
///
/// **它不是随机数** —— 见模块文档 ①。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(String);

impl InstanceId {
    /// 从 `key` 算出来（**确定性**：同一个 key 永远得到同一个 ID）。
    pub fn from_key(key: &str) -> Self {
        Self(format!("inst-{:016x}", fnv1a64(key.as_bytes())))
    }

    /// 从一个已有的字符串还原（用于反序列化后的校验）。
    pub fn parse(s: &str) -> Option<Self> {
        let hex = s.strip_prefix("inst-")?;
        if hex.len() != 16 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// FNV-1a 64 位。
///
/// **为什么不用 `DefaultHasher`**：它的输出**不保证跨版本稳定** ——
/// 而实例 ID 会被写进文件、在升级后还要能对上。
/// 一个"换个 Rust 版本就换了 ID"的实现会让所有实例变成"身份不明"。
///
/// **为什么不用 `uuid`**：见模块文档 ① —— 随机 ID 无法校验"还对得上吗"。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// **这个实例属于哪个产品的哪个变体。**
///
/// `product_key` 是去 [`crate::provider::ProductRegistry`] 查能力的钥匙，
/// 而**能力是产品维度的唯一真相**（方案 §3.4：做不到就如实报 `false` 并给原因）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductRef {
    /// 产品标识（`"java"` / `"bedrock"` / 未来的别的）
    pub product_key: String,
    /// 版本标识（**对 Java 版就是 manifest 里的 `id`**）
    ///
    /// ⚠️ **不许硬编码"最新版"** —— 官方最新 release 是 `26.3` 而不是 `1.21.x`，
    /// 所以"最新"必须**从 manifest 来**，永远不是一个常量。
    pub version: String,
    /// 变体标识（同一个版本的不同形态：`vanilla` / `forge` / `fabric` / …）
    ///
    /// **空串表示"该产品没有变体概念"**，而不是"未知"。
    pub variant: String,
}

impl ProductRef {
    pub fn new(
        product_key: impl Into<String>,
        version: impl Into<String>,
        variant: impl Into<String>,
    ) -> Self {
        Self {
            product_key: product_key.into(),
            version: version.into(),
            variant: variant.into(),
        }
    }

    /// 有没有变体概念。
    pub fn has_variant(&self) -> bool {
        !self.variant.is_empty()
    }

    /// 给人看的一行（`java 26.3/vanilla`）。
    pub fn display(&self) -> String {
        if self.has_variant() {
            format!("{} {}/{}", self.product_key, self.version, self.variant)
        } else {
            format!("{} {}", self.product_key, self.version)
        }
    }
}

/// 实例的生命周期状态。
///
/// 它与 [`crate::crash::MarkerDecision`] 的关系是：
/// **崩溃标记决定"要不要进 `Manual`"**，而不是另有第四个状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceState {
    /// 全新、可用
    Ready,
    /// **需要人工处理**（迁移失败 / 反复崩溃）—— 不会自动启动
    Manual,
    /// 正在运行（**运行态是易失的，不写进磁盘**）
    Running,
    /// 用户归档了它（不出现在默认列表里，但数据还在）
    Archived,
}

impl InstanceState {
    pub const ALL: &'static [InstanceState] =
        &[Self::Ready, Self::Manual, Self::Running, Self::Archived];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Manual => "manual",
            Self::Running => "running",
            Self::Archived => "archived",
        }
    }

    /// **它是不是一个会被写进磁盘的持久状态。**
    ///
    /// `Running` 不是 —— 它由进程与锁推出，而不是由文件说。
    /// 一个把 `Running` 写进磁盘的实现会在**上次异常退出后**
    /// 让界面显示"正在运行"，而那个状态**永远无法自愈**。
    pub const fn is_persistent(self) -> bool {
        !matches!(self, Self::Running)
    }

    /// 能不能从这个状态发起启动。
    pub const fn can_launch(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// 一个实例（**通用模型：任何产品都成立的部分**）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instance {
    /// 稳定标识（`key` 的函数）
    pub id: InstanceId,
    /// **目录名** —— 同时是稳定的"键"
    ///
    /// 它在实例根之下、必须是一个合法的 [`RelPath`] 单段，
    /// 且**一经分配不再改变**（用户改的是 `name`）。
    pub key: String,
    /// 显示名（**用户可以随便改**）
    pub name: String,
    /// 相对实例根的目录
    pub dir: RelPath,
    /// 哪个产品的哪个变体
    pub product: ProductRef,
    /// 生命周期状态
    pub state: InstanceState,
    /// 数据格式版本（用于 [`crate::migrate`]）
    pub data_version: u32,
}

/// 构造/校验实例时的错误。**每一条都可被穷举。**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceError {
    /// `key` 是空的
    EmptyKey,
    /// `key` 含路径分隔符、`..`、`.`、或 Windows 非法字符
    BadKey { key: String, why: &'static str },
    /// `name` 是空的（或只有空白）
    EmptyName,
    /// `dir` 与 `key` 不一致
    ///
    /// **这条是有意的**：`dir` 应当是 `instances/<key>`，
    /// 而"两处独立地记着同一件事"就是"它们会漂开"的前提。
    DirKeyMismatch { dir: String, key: String },
    /// `product_key` 为空
    EmptyProduct,
    /// `version` 为空
    EmptyVersion,
    /// **ID 与 `key` 不符**（陈旧或被篡改的记录）
    IdKeyMismatch { id: String, expected: String },
}

impl std::fmt::Display for InstanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstanceError::EmptyKey => write!(f, "实例 key 不能为空"),
            InstanceError::BadKey { key, why } => write!(f, "实例 key {key:?} 不合法：{why}"),
            InstanceError::EmptyName => write!(f, "实例显示名不能为空"),
            InstanceError::DirKeyMismatch { dir, key } => write!(
                f,
                "实例目录 {dir:?} 与它的 key {key:?} 不一致 —— \
                 两处独立记着同一件事，就是它们会漂开的前提"
            ),
            InstanceError::EmptyProduct => write!(f, "product_key 不能为空"),
            InstanceError::EmptyVersion => write!(
                f,
                "version 不能为空 —— 「最新版」必须从 manifest 来，不许是一个常量"
            ),
            InstanceError::IdKeyMismatch { id, expected } => write!(
                f,
                "实例 ID {id} 与它 key 算出来的 {expected} 不符 —— \
                 这条记录是陈旧的，或被人改过"
            ),
        }
    }
}

impl std::error::Error for InstanceError {}

/// **`key` 的合法性。**
///
/// 规则（刻意严格 —— 它会变成一个真实的目录名）：
/// - 非空、长度 ≤ 64
/// - 不含 `/` `\`（那是路径分隔符）
/// - 不是 `.` 或 `..`
/// - 不含 Windows 非法字符 `< > : " | ? *`
/// - 不含控制字符
/// - **不以空格或点结尾**（Windows 会默默去掉它们 → 名实不符）
pub fn is_valid_key(key: &str) -> bool {
    if key.is_empty() || key.len() > 64 {
        return false;
    }
    if key == "." || key == ".." {
        return false;
    }
    if key.ends_with(' ') || key.ends_with('.') {
        return false;
    }
    if key.contains('/') || key.contains('\\') {
        return false;
    }
    if key
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control())
    {
        return false;
    }
    // Windows 保留设备名 —— **目录名叫 `NUL` 会创建失败或指向设备**
    const RESERVED: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let stem = key.split('.').next().unwrap_or(key).to_ascii_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return false;
    }
    true
}

/// 实例相对实例根所在的那个目录层。
///
/// **它集中在一个函数里**，因为 `dir` 与 `key` 必须一致 ——
/// 而"两处独立地拼同一个路径"就是它们会漂开的前提。
pub fn dir_for_key(key: &str) -> RelPath {
    RelPath::new(format!("instances/{key}")).expect("已校验过的 key 拼出的路径必然合法")
}

impl Instance {
    /// **新建**一个实例（ID 由 key 算出）。
    pub fn create(
        key: impl Into<String>,
        name: impl Into<String>,
        product: ProductRef,
        data_version: u32,
    ) -> Result<Self, InstanceError> {
        let key = key.into();
        let name = name.into();
        if key.is_empty() {
            return Err(InstanceError::EmptyKey);
        }
        if !is_valid_key(&key) {
            return Err(InstanceError::BadKey {
                key: key.clone(),
                why: bad_key_reason(&key),
            });
        }
        if name.trim().is_empty() {
            return Err(InstanceError::EmptyName);
        }
        if product.product_key.is_empty() {
            return Err(InstanceError::EmptyProduct);
        }
        if product.version.is_empty() {
            return Err(InstanceError::EmptyVersion);
        }
        Ok(Self {
            id: InstanceId::from_key(&key),
            dir: dir_for_key(&key),
            key,
            name,
            product,
            state: InstanceState::Ready,
            data_version,
        })
    }

    /// **校验一条读回来的记录。**
    ///
    /// 它是"落盘 → 读回"这条往返的对称面：能写的都该能读，
    /// 而**读回来的东西必须自洽** —— 尤其是 ID 与 key 的关系。
    pub fn validate(&self) -> Vec<InstanceError> {
        let mut errs = Vec::new();
        if self.key.is_empty() {
            errs.push(InstanceError::EmptyKey);
        } else if !is_valid_key(&self.key) {
            errs.push(InstanceError::BadKey {
                key: self.key.clone(),
                why: bad_key_reason(&self.key),
            });
        }
        if self.name.trim().is_empty() {
            errs.push(InstanceError::EmptyName);
        }
        if self.product.product_key.is_empty() {
            errs.push(InstanceError::EmptyProduct);
        }
        if self.product.version.is_empty() {
            errs.push(InstanceError::EmptyVersion);
        }
        // **ID 与 key 的关系**：这是本模块最独特的一条校验。
        let expected = InstanceId::from_key(&self.key);
        if self.id != expected {
            errs.push(InstanceError::IdKeyMismatch {
                id: self.id.as_str().to_string(),
                expected: expected.as_str().to_string(),
            });
        }
        // dir 必须与 key 一致
        let want = dir_for_key(&self.key);
        if self.dir != want {
            errs.push(InstanceError::DirKeyMismatch {
                dir: self.dir.as_str().to_string(),
                key: self.key.clone(),
            });
        }
        errs
    }

    /// 一次说清"这条记录自洽吗"。
    pub fn check_integrity(&self) -> Result<(), Vec<InstanceError>> {
        let e = self.validate();
        if e.is_empty() {
            Ok(())
        } else {
            Err(e)
        }
    }

    /// **改显示名。** 这是唯一一个"改了不会破坏身份"的字段。
    pub fn rename(&mut self, new_name: impl Into<String>) -> Result<(), InstanceError> {
        let n = new_name.into();
        if n.trim().is_empty() {
            return Err(InstanceError::EmptyName);
        }
        self.name = n;
        // **key 与 id 都不动** —— 那正是"身份"与"标签"分开的意义。
        Ok(())
    }

    /// 换产品变体（**实例身份不变** —— 同一个目录里换了加载器还是同一个实例）。
    pub fn rebind_product(&mut self, product: ProductRef) -> Result<(), InstanceError> {
        if product.product_key.is_empty() {
            return Err(InstanceError::EmptyProduct);
        }
        if product.version.is_empty() {
            return Err(InstanceError::EmptyVersion);
        }
        self.product = product;
        Ok(())
    }

    /// 能不能启动。
    pub fn can_launch(&self) -> bool {
        self.state.can_launch()
    }

    /// 按稳定顺序排一批实例（**界面与测试都靠它得到确定的顺序**）。
    ///
    /// 顺序：`name` → `id`（并列时打破）。
    ///
    /// ## ⚠️ 这个顺序是**确定的，但不是"语言学上正确的"**
    ///
    /// 它用 `str::cmp` —— 也就是**按 UTF-8 字节序**。而那个顺序：
    ///
    /// | 名字 | 字节序下的位置 | 中文使用者期待的 |
    /// |---|---|---|
    /// | `Alpha` / `Beta` | A 在前 ✓ | 一致 |
    /// | **`甲` / `乙`** | **`乙` 在前**（`乙` = U+4E59 < `甲` = U+7532） | **`甲` 在前**（按拼音 jiǎ < yǐ） |
    ///
    /// **所以中文名的排序结果是"反的"。** 而这条注释留着，是因为
    /// 我**在写测试时正是被它绊了一下**：我以为 `甲乙` 该按那个顺序排出来。
    ///
    /// 正确的做法（一期不做）是**按 `locale` 取一个 collator**。
    /// 一期不做它的理由是：**它需要一个按语言而定的排序表**，
    /// 而那是几百 KB 的数据 —— 而"实例列表的排序方式"目前不是产品决策。
    ///
    /// **现在这样就够用的地方**：它是**确定的**（同一批实例永远同一个顺序），
    /// 而"界面每次刷新顺序都在变"才是真正要防的那件事。
    ///
    /// 另有一条测试把这个限制**钉成一个可断言的事实**（`排序不承诺语言学正确`），
    /// 于是它将来不会变成一个"我以为它是对的"的静默假设。
    pub fn sort_stable(v: &mut [Instance]) {
        v.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    }
}

fn bad_key_reason(key: &str) -> &'static str {
    if key.is_empty() {
        return "空";
    }
    if key.len() > 64 {
        return "过长（>64）";
    }
    if key == "." || key == ".." {
        return "是 . 或 ..";
    }
    if key.ends_with(' ') || key.ends_with('.') {
        return "以空格或点结尾（Windows 会默默去掉它们）";
    }
    if key.contains('/') || key.contains('\\') {
        return "含路径分隔符";
    }
    if key
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return "含 Windows 非法字符";
    }
    if key.chars().any(|c| c.is_control()) {
        return "含控制字符";
    }
    "是 Windows 保留设备名"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pv() -> ProductRef {
        ProductRef::new("java", "26.3", "vanilla")
    }

    fn mk(key: &str) -> Instance {
        Instance::create(key, "测试实例", pv(), 3).unwrap()
    }

    // ───────────────── ID 是 key 的函数 ─────────────────

    #[test]
    fn id_是确定性的() {
        // **这是本模块最重要的性质。**
        // 同一个 key 必须永远得到同一个 ID —— 否则"读回来的记录还对得上吗"
        // 就变成一个无法回答的问题。
        let a = InstanceId::from_key("abc");
        let b = InstanceId::from_key("abc");
        assert_eq!(a, b);
        assert!(a.as_str().starts_with("inst-"));
        assert_eq!(a.as_str().len(), 5 + 16);
        // 不同的 key 得到不同的 ID
        assert_ne!(InstanceId::from_key("abc"), InstanceId::from_key("abd"));
    }

    #[test]
    fn fnv_的已知值() {
        // FNV-1a 64 的标准测试向量 —— 钉住它，防止有人"顺手换个哈希"。
        // 换了哈希会让**所有已有实例的 ID 全部失配**。
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn id_的解析会拒绝畸形值() {
        assert!(InstanceId::parse("inst-0123456789abcdef").is_some());
        for bad in [
            "",
            "inst-",
            "inst-0123456789abcde",   // 15 位
            "inst-0123456789abcdef0", // 17 位
            "inst-0123456789abcdeZ",  // 非十六进制
            "0123456789abcdef",       // 缺前缀
            "INST-0123456789abcdef",  // 前缀大小写
        ] {
            assert!(InstanceId::parse(bad).is_none(), "{bad} 不该被接受");
        }
    }

    // ───────────────── key 的合法性 ─────────────────

    #[test]
    fn 合法_key() {
        for ok in ["abc", "my-instance", "实例", "1", "a.b.c", "x1"] {
            assert!(is_valid_key(ok), "{ok} 该合法");
        }
    }

    #[test]
    fn 非法_key_被拒绝且原因具体() {
        // 每一条都对应一个**真实的坏后果**，而不只是"看起来不干净"。
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a<b",
            "a>b",
            "a:b",
            "a\"b",
            "a|b",
            "a?b",
            "a*b",
            "trailing ",
            "trailing.",
            "NUL",
            "con.txt",
            "COM1",
            &"x".repeat(65),
        ] {
            assert!(!is_valid_key(bad), "{bad:?} 不该合法");
            if !bad.is_empty() {
                let why = bad_key_reason(bad);
                assert!(!why.is_empty(), "{bad:?} 该给出原因");
            }
        }
        // 控制字符
        assert!(!is_valid_key("a\u{0007}b"));
    }

    #[test]
    fn 保留设备名会被识别() {
        // 目录名叫 `NUL` 会创建失败或指向设备。
        for bad in ["NUL", "nul", "Con", "PRN", "aux", "COM9", "LPT1"] {
            assert!(!is_valid_key(bad), "{bad} 是保留设备名");
        }
        // 但相近的正常名字不该被误判
        for ok in ["console", "null", "com0", "com10", "aux2"] {
            assert!(is_valid_key(ok), "{ok} 不该被误判为保留名");
        }
    }

    // ───────────────── 创建 ─────────────────

    #[test]
    fn 创建实例并得到自洽的记录() {
        let i = mk("alpha");
        assert_eq!(i.key, "alpha");
        assert_eq!(i.dir.as_str(), "instances/alpha");
        assert_eq!(i.id, InstanceId::from_key("alpha"));
        assert_eq!(i.state, InstanceState::Ready);
        assert!(i.check_integrity().is_ok(), "{:?}", i.validate());
    }

    #[test]
    fn 创建时的错误是具体的() {
        assert_eq!(
            Instance::create("", "n", pv(), 1).unwrap_err(),
            InstanceError::EmptyKey
        );
        assert!(matches!(
            Instance::create("a/b", "n", pv(), 1).unwrap_err(),
            InstanceError::BadKey { .. }
        ));
        assert_eq!(
            Instance::create("ok", "   ", pv(), 1).unwrap_err(),
            InstanceError::EmptyName
        );
        assert_eq!(
            Instance::create("ok", "n", ProductRef::new("", "1", ""), 1).unwrap_err(),
            InstanceError::EmptyProduct
        );
        assert_eq!(
            Instance::create("ok", "n", ProductRef::new("java", "", ""), 1).unwrap_err(),
            InstanceError::EmptyVersion
        );
    }

    // ───────────────── 校验能发现"陈旧记录" ─────────────────

    #[test]
    fn id_与_key_不符会被发现() {
        // ⚠️ **这条校验是本模块最独特的一条**，也是"ID 必须是 key 的函数"
        // 这个设计选择的直接回报：一个随机 ID 的方案**做不到这件事**。
        let mut i = mk("alpha");
        i.id = InstanceId::from_key("beta"); // 模拟"记录被改过/陈旧"
        let errs = i.validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, InstanceError::IdKeyMismatch { .. })),
            "该发现 ID 与 key 不符：{errs:?}"
        );
        let msg = errs
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("；");
        assert!(msg.contains("陈旧"), "错误信息要说清这意味着什么：{msg}");
    }

    #[test]
    fn dir_与_key_不符会被发现() {
        let mut i = mk("alpha");
        i.dir = RelPath::new("instances/other").unwrap();
        let errs = i.validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, InstanceError::DirKeyMismatch { .. })),
            "{errs:?}"
        );
    }

    #[test]
    fn 校验一次报全部问题() {
        // 只报第一个会把一次能说完的事变成 N 轮。
        let mut i = mk("alpha");
        i.id = InstanceId::from_key("zzz");
        i.dir = RelPath::new("instances/yyy").unwrap();
        i.name = "  ".to_string();
        let errs = i.validate();
        assert!(errs.len() >= 3, "该一次报全部：{errs:?}");
    }

    // ───────────────── 改名不动身份 ─────────────────

    #[test]
    fn 改名不动_key_与_id() {
        // ⚠️ **这是"身份"与"标签"分开的直接回报。**
        // 一个把显示名直接当目录名的实现，在用户重命名时会面对
        // "要么改目录（可能撞车/可能失败），要么名实不符"。
        let mut i = mk("alpha");
        let id_before = i.id.clone();
        let key_before = i.key.clone();
        let dir_before = i.dir.clone();

        i.rename("全新的名字").unwrap();

        assert_eq!(i.name, "全新的名字");
        assert_eq!(i.id, id_before, "**ID 不该变**");
        assert_eq!(i.key, key_before, "**key 不该变**");
        assert_eq!(i.dir, dir_before, "**目录不该变**");
        assert!(i.check_integrity().is_ok());
    }

    #[test]
    fn 改名成空白被拒绝() {
        let mut i = mk("alpha");
        assert_eq!(i.rename("  ").unwrap_err(), InstanceError::EmptyName);
        assert_eq!(i.name, "测试实例", "失败时不该改动");
    }

    #[test]
    fn 换产品变体不动身份() {
        // 同一个目录里换了加载器（vanilla → fabric）**还是同一个实例**。
        let mut i = mk("alpha");
        let id_before = i.id.clone();
        i.rebind_product(ProductRef::new("java", "26.3", "fabric"))
            .unwrap();
        assert_eq!(i.id, id_before);
        assert_eq!(i.product.variant, "fabric");
        assert!(i.check_integrity().is_ok());
        // 变空版本要失败，且不改动
        assert!(i
            .rebind_product(ProductRef::new("java", "", "fabric"))
            .is_err());
        assert_eq!(i.product.version, "26.3");
    }

    // ───────────────── ProductRef ─────────────────

    #[test]
    fn product_ref_的展示与变体语义() {
        let a = ProductRef::new("java", "26.3", "vanilla");
        assert!(a.has_variant());
        assert_eq!(a.display(), "java 26.3/vanilla");
        // **空串表示"该产品没有变体概念"，不是"未知"**
        let b = ProductRef::new("bedrock", "1.21.90", "");
        assert!(!b.has_variant());
        assert_eq!(b.display(), "bedrock 1.21.90");
    }

    // ───────────────── 状态 ─────────────────

    #[test]
    fn 运行态不是持久状态() {
        // ⚠️ 一个把 `Running` 写进磁盘的实现会在**上次异常退出后**
        // 让界面显示"正在运行"，而那个状态**永远无法自愈**。
        assert!(!InstanceState::Running.is_persistent());
        for s in [
            InstanceState::Ready,
            InstanceState::Manual,
            InstanceState::Archived,
        ] {
            assert!(s.is_persistent(), "{s:?} 该是持久状态");
        }
    }

    #[test]
    fn 只有_ready_能启动() {
        assert!(InstanceState::Ready.can_launch());
        for s in [
            InstanceState::Manual,
            InstanceState::Running,
            InstanceState::Archived,
        ] {
            assert!(!s.can_launch(), "{s:?} 不该能启动");
        }
    }

    #[test]
    fn 状态的_all_与_key_无遗漏且互不相同() {
        // 这条防的是"加了新状态却忘了登记"——而 `ALL` 是界面遍历的依据。
        assert_eq!(InstanceState::ALL.len(), 4);
        let mut keys: Vec<&str> = InstanceState::ALL.iter().map(|s| s.key()).collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n, "key 有重复");
        for s in InstanceState::ALL {
            assert!(s.key().is_ascii(), "{s:?} 的 key 该是 ASCII");
        }
    }

    // ───────────────── 序列化往返 ─────────────────

    #[test]
    fn json_往返且形态稳定() {
        // 实例记录会被写进磁盘，所以**它的 JSON 形态是契约**。
        let i = mk("alpha");
        let json = serde_json::to_string(&i).unwrap();
        let back: Instance = serde_json::from_str(&json).unwrap();
        assert_eq!(back, i);
        // `id` 是 transparent 的 —— 直接是字符串，不是 `{"0": "..."}`
        assert!(json.contains("\"id\":\"inst-"), "{json}");
        assert!(
            json.contains("\"state\":\"ready\""),
            "状态该是 snake_case：{json}"
        );
        assert!(json.contains("\"dir\":\"instances/alpha\""), "{json}");
    }

    #[test]
    fn 读回的畸形记录会被_validate_抓住() {
        // 手工造一条"ID 与 key 不匹配"的 JSON —— 那是**现实中会出现**的形态：
        // 目录被手工改名、或记录被别的工具改过。
        let json = r#"{
            "id": "inst-0000000000000000",
            "key": "alpha",
            "name": "x",
            "dir": "instances/alpha",
            "product": {"product_key":"java","version":"26.3","variant":""},
            "state": "ready",
            "data_version": 3
        }"#;
        let i: Instance = serde_json::from_str(json).unwrap();
        let errs = i.validate();
        assert!(
            errs.iter()
                .any(|e| matches!(e, InstanceError::IdKeyMismatch { .. })),
            "读回的记录必须被校验：{errs:?}"
        );
    }

    // ───────────────── 排序 ─────────────────

    #[test]
    fn 排序是确定的() {
        // 用 ASCII 名验证"顺序本身" —— 而中文名的行为单独由下一条测试钉住。
        let mut v = vec![
            Instance::create("c", "Zeta", pv(), 1).unwrap(),
            Instance::create("a", "Alpha", pv(), 1).unwrap(),
            Instance::create("b", "Alpha", pv(), 1).unwrap(), // 同名，靠 id 打破
        ];
        Instance::sort_stable(&mut v);
        let names: Vec<&str> = v.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Alpha", "Alpha", "Zeta"]);
        // 同名的两个按 id 升序 —— **确定性**（否则界面每次刷新顺序都变）
        assert!(v[0].id < v[1].id, "同名时该按 id 打破并列");
        // 再排一次结果不变
        let before: Vec<String> = v.iter().map(|i| i.id.as_str().to_string()).collect();
        Instance::sort_stable(&mut v);
        let after: Vec<String> = v.iter().map(|i| i.id.as_str().to_string()).collect();
        assert_eq!(before, after, "排序必须幂等");
    }

    #[test]
    fn 排序不承诺语言学正确() {
        // ⚠️ **这条测试把上面那条文档里的限制钉成可断言的事实。**
        //
        // 我写前一版测试时以为 `甲` 会排在 `乙` 前面（按拼音 jiǎ < yǐ），
        // 而实际不会 —— 因为 `str::cmp` 是**按 UTF-8 字节序**，
        // 而 `乙` = U+4E59 < `甲` = U+7532。
        //
        // 所以这条测试断言的是**当前的真实行为**，而不是"我期望的行为"。
        // 它的用处：将来若有人加了 collator，这条测试会红，
        // 而**那次红正是"排序语义变了"的一次显式确认**。
        let mut v = vec![
            Instance::create("a", "甲", pv(), 1).unwrap(),
            Instance::create("b", "乙", pv(), 1).unwrap(),
        ];
        Instance::sort_stable(&mut v);
        let names: Vec<&str> = v.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["乙", "甲"],
            "当前是 UTF-8 字节序：乙 < 甲。这**不是**拼音序 —— \
             若这条红了，说明排序语义变了，而那需要一次显式确认"
        );
        // 但它仍然是**确定的**
        let mut again = v.clone();
        Instance::sort_stable(&mut again);
        assert_eq!(again, v, "无论用什么序，都必须是确定的");
    }

    // ───────────────── 通用性：它能容纳别的产品 ─────────────────

    #[test]
    fn 同一个模型能容纳基岩版形态() {
        // ⚠️ **这条测试是"通用"这个词的实质检验。**
        //
        // 一个"Java 版包装"的模型会在这里崩：基岩版没有 versions/、
        // 没有 .jar、没有变体概念。
        let bedrock = ProductRef::new("bedrock", "1.21.90", "");
        let i = Instance::create("bedrock-main", "基岩版", bedrock, 3).unwrap();
        assert!(i.check_integrity().is_ok());
        assert_eq!(i.product.display(), "bedrock 1.21.90");
        assert!(!i.product.has_variant());
        // 模型里**没有任何字段提到 java / jar / versions**
        let json = serde_json::to_string(&i).unwrap().to_lowercase();
        for forbidden in ["jar", "versions/", "asset", "libraries"] {
            assert!(
                !json.contains(forbidden),
                "通用实例模型里不该出现产品专属的词 {forbidden:?}：{json}"
            );
        }
    }

    #[test]
    fn 同一个模型能容纳与_mc_无关的产品() {
        // 用户原话：「后面可能也会添加一些与MC无关的功能」。
        // 一个把"实例"绑死在 MC 上的模型会在那时需要重写。
        let other = ProductRef::new("some-other-product", "2026.1", "");
        let i = Instance::create("other-1", "别的东西", other, 3).unwrap();
        assert!(i.check_integrity().is_ok());
        assert!(i.can_launch());
    }
}
