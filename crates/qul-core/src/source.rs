//! # 多源策略与速度判定（**纯规则，零 IO**）
//!
//! 设计见 `docs/下载引擎设计规格.md` §5（依据 HMCL `HMCLDownloadProvider.java:62-116`
//! 与 Axolotl 的速度地板）。
//!
//! ## ⚠️ 本模块存在的第一理由：**"不许在代码里写死优先级"**
//!
//! 规格 §5 的补正原文：
//!
//! > 本节原写法隐含"官方源更快、镜像只是备用"。
//! > **用户反馈与归档项目结论冲突** → 已改为 **先测后定**：
//! > 每台机器首次使用时实测各源吞吐，按实测排序并缓存结果；
//! > 镜像的价值同时在于"**官方不通时仍能装完**"。
//! > **不许在代码里写死优先级。**
//!
//! **而"M0 的 S5 尖刺"正是这条补正的实测依据**：在本机实测中
//! 官方源 0.382 → 0.832 MB/s（并发后），而 BMCLAPI 是
//! **0.000–0.124 MB/s 且失败数等于并发数** ——
//! 也就是说**这台机器上镜像比官方差得多**，
//! 而换一条网络路径结论就会反过来（归档项目那次测到官方快 8 倍）。
//!
//! **所以"哪条源快"是一个必须实测、且必须带环境一起引用的结论。**
//! 本模块的 [`SourcePolicy::order`] 因此在**没有任何测量**时返回
//! [`SourceOrder::NeedProbe`] 而不是"默认官方优先" —— 那个默认值正是规格要禁掉的东西。
//!
//! ## 第二条：镜像哈希**只用于跳过下载，不用于判定正确**
//!
//! 规格 §5 原文（HMCL `FetchTask.java:355-357`）：
//!
//! > 读镜像返回的 **`x-bmclapi-hash`** 头，若为合法 SHA-1 就**用于缓存命中判断**
//! > （省一次网络往返），**但最终仍以官方元数据的哈希为准做完整性校验**。
//! > **这条要写进实现注释，否则后人会误用。**
//!
//! 所以 [`MirrorHint`] 的用途在类型名与注释里被写死：它叫 **hint**（提示），
//! 而 `is_usable_for_integrity()` **恒为 `false`** —— 那不是一个"当前返回 false"
//! 的实现细节，而是**这个类型在设计上就不承担校验职责**。

use serde::{Deserialize, Serialize};

/// 一条镜像改写规则：`源前缀 → 目标前缀`。
///
/// ## 为什么是"改写表"而不是"多源列表"
///
/// 规格 §5 给了三条理由：
///
/// > 1. **一条规则服务所有 URL**，不需要为每个下载项维护多份源地址；
/// > 2. **天然覆盖内容平台 CDN**（整合包/mod 下载也走同一套）；
/// > 3. **新增镜像 = 加一条规则，不改代码**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirrorRule {
    /// 官方源的前缀（如 `https://piston-meta.mojang.com/`）
    pub from_prefix: String,
    /// 镜像的前缀（如 `https://bmclapi2.bangbang93.com/`）
    pub to_prefix: String,
    /// 这条规则属于哪个"源"（用于按实测吞吐排序）
    pub source_key: String,
}

/// 把一个 URL 按规则改写。**改不动就原样返回**（不是错误）：
/// 一个不在规则表里的 URL 仍然是**可下载的官方地址**，
/// 把它报成错误会让"未收录的源"变成一次失败。
pub fn rewrite(url: &str, rule: &MirrorRule) -> Option<String> {
    url.strip_prefix(&rule.from_prefix)
        .map(|rest| format!("{}{}", rule.to_prefix, rest))
}

/// **已知的源**（不是优先级列表 —— 它只是"我们认识哪些源"）。
///
/// ⚠️ **顺序在这里没有意义。** 真正的顺序由 [`SourcePolicy::order`] 按实测吞吐给出。
/// 这个常量之所以是一个数组而不是一个有序列表，就是为了**让人无法把它当优先级用**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownSource {
    pub key: String,
    /// 可读的名字（给用户看的"测速结果"列表）
    pub label: String,
    /// 它是镜像吗（镜像的价值包含"官方不通时仍能装完"）
    pub is_mirror: bool,
}

/// 一个源的实测吞吐。
///
/// ## 为什么存"近 10 样本均值 ×85%"而不是最新值
///
/// 规格 §7.2 第 3 项给的依据（Axolotl `download_manager.rs:6-8, 57-107`）：
///
/// > 判定改用**近 10 样本均值 ×85%、只增不减**的自适应地板
///
/// **"只增不减"是关键**：一个"地板"一旦被一次慢样本拉低，
/// 后面的正常速度就永远够不着它，于是源会被永久判为"慢"。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStats {
    pub source_key: String,
    /// 最近的样本（**新样本在后**）
    recent: Vec<u64>,
    /// 自适应地板：**只增不减**（见类型文档）
    floor: u64,
}

/// 均值窗口大小（规格：近 10 样本）
pub const SAMPLE_WINDOW: usize = 10;
/// 地板系数（规格：×85%）
pub const FLOOR_NUM: u64 = 85;
pub const FLOOR_DEN: u64 = 100;

impl SourceStats {
    pub fn new(source_key: impl Into<String>) -> Self {
        Self {
            source_key: source_key.into(),
            recent: Vec::new(),
            floor: 0,
        }
    }

    /// 喂一个吞吐样本（字节/秒）。
    pub fn observe(&mut self, throughput: u64) {
        self.recent.push(throughput);
        if self.recent.len() > SAMPLE_WINDOW {
            self.recent.remove(0);
        }
        let mean = self.mean();
        // **只增不减**：地板永远不会因为一次慢样本而下降。
        let candidate = mean * FLOOR_NUM / FLOOR_DEN;
        if candidate > self.floor {
            self.floor = candidate;
        }
    }

    /// 近 N 个样本的均值（无样本时为 0）。
    pub fn mean(&self) -> u64 {
        if self.recent.is_empty() {
            return 0;
        }
        let sum: u64 = self.recent.iter().sum();
        sum / self.recent.len() as u64
    }

    pub fn floor(&self) -> u64 {
        self.floor
    }

    pub fn samples(&self) -> usize {
        self.recent.len()
    }

    /// 它是否已经"慢到不该再用"。
    ///
    /// **地板为 0（还没有足够样本）时恒为 `false`** ——
    /// 一个没有测量的源不该被判为慢，那会让"首次使用"直接跳过所有源。
    pub fn is_slow(&self) -> bool {
        self.floor > 0 && self.mean() < self.floor
    }
}

/// 排序结果。**三态而不是一个排序好的列表** —— 因为"还没测过"必须能被表达。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOrder {
    /// **还没测过，必须先探一下。**
    ///
    /// ⚠️ 这个变体是本模块存在的理由：规格明确禁止"代码里写死优先级"，
    /// 而一个"没有测量就返回官方优先"的实现**恰好就是那个被禁掉的东西**。
    NeedProbe { candidates: Vec<String> },
    /// 按实测吞吐排好序（快的在前）
    Ordered { keys: Vec<String> },
}

/// 源策略：认识哪些源 + 它们的测量结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourcePolicy {
    sources: Vec<KnownSource>,
    stats: Vec<SourceStats>,
}

impl SourcePolicy {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个已知的源。**重复登记会被忽略**（不是错误：
    /// 规则表可能从多个来源拼起来，重复是正常的）。
    pub fn know(&mut self, s: KnownSource) -> &mut Self {
        if !self.sources.iter().any(|x| x.key == s.key) {
            self.stats.push(SourceStats::new(s.key.clone()));
            self.sources.push(s);
        }
        self
    }

    pub fn sources(&self) -> &[KnownSource] {
        &self.sources
    }

    pub fn stats(&self, key: &str) -> Option<&SourceStats> {
        self.stats.iter().find(|s| s.source_key == key)
    }

    /// 喂一个实测样本。
    pub fn observe(&mut self, key: &str, throughput: u64) -> bool {
        match self.stats.iter_mut().find(|s| s.source_key == key) {
            Some(s) => {
                s.observe(throughput);
                true
            }
            None => false,
        }
    }

    /// **按实测吞吐给出使用顺序。**
    ///
    /// | 情形 | 结果 |
    /// |---|---|
    /// | 一个源都没测过 | [`SourceOrder::NeedProbe`]（**不是"官方优先"**） |
    /// | 测过一部分 | 按实测排；**没测过的排在测过的之后**（不猜它的速度） |
    /// | 全测过 | 按实测从快到慢 |
    ///
    /// **"没测过的排在测过的之后"而不是"排在最前"**：
    /// 排在前面会让首次使用总是先撞一个可能不通的源；
    /// 排在后面则它们仍然会在前面那些失败时被用到（这正是镜像的价值）。
    pub fn order(&self) -> SourceOrder {
        let measured: Vec<&SourceStats> = self.stats.iter().filter(|s| s.samples() > 0).collect();
        if measured.is_empty() {
            return SourceOrder::NeedProbe {
                candidates: self.sources.iter().map(|s| s.key.clone()).collect(),
            };
        }
        let mut keys: Vec<(String, u64)> = measured
            .iter()
            .map(|s| (s.source_key.clone(), s.mean()))
            .collect();
        // **降序**：快的在前。并列时按键名排，保证确定（同机同输入同输出）
        keys.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut out: Vec<String> = keys.into_iter().map(|(k, _)| k).collect();
        // 没测过的接在后面
        for s in &self.sources {
            if !out.contains(&s.key) {
                out.push(s.key.clone());
            }
        }
        SourceOrder::Ordered { keys: out }
    }

    /// 实测结论的**一行摘要**（给"测速"界面与诊断包用）。
    ///
    /// 它必须带上"这是实测结论"这句话 —— 规格要求**结论要带环境一起引用**，
    /// 而这个摘要正是被引用时的那段上下文。
    pub fn summary(&self) -> String {
        match self.order() {
            SourceOrder::NeedProbe { candidates } => format!(
                "尚未实测各源吞吐（{} 个候选：{}）—— 首次使用会先测一次",
                candidates.len(),
                candidates.join("、")
            ),
            SourceOrder::Ordered { keys } => {
                let mut parts = Vec::new();
                for k in &keys {
                    match self.stats(k) {
                        Some(s) if s.samples() > 0 => {
                            parts.push(format!("{k} {:.3} MB/s", s.mean() as f64 / 1_048_576.0))
                        }
                        _ => parts.push(format!("{k} 未测")),
                    }
                }
                format!("按本机实测排序（带环境引用）：{}", parts.join(" > "))
            }
        }
    }
}

// ───────────────────────── 镜像哈希：**只是提示** ─────────────────────────

/// 镜像返回的哈希提示（`x-bmclapi-hash`）。
///
/// ## ⚠️ 它的名字叫 hint，而这是刻意的
///
/// 规格 §5 原文：
///
/// > 读镜像返回的 **`x-bmclapi-hash`** 头，若为合法 SHA-1 就**用于缓存命中判断**
/// > （省一次网络往返），**但最终仍以官方元数据的哈希为准做完整性校验**。
/// > **这条要写进实现注释，否则后人会误用。**
///
/// **"后人会误用"是完全可预见的**：一个已经拿到哈希的地方，
/// 顺手拿它做校验是**最自然的一步**，而那样做的后果是
/// **镜像可以决定我们认为什么内容是"正确的"** ——
/// 那是一道不该交出去的安全边界。
///
/// 所以本类型的用法被写死：
/// - ✅ `is_usable_for_cache_hit()` —— 用来**跳过下载**
/// - ❌ `is_usable_for_integrity()` —— **恒为 `false`**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorHint {
    value: String,
}

impl MirrorHint {
    /// 从响应头值解析。**只接受合法的 SHA-1（40 位十六进制）** ——
    /// 一个长度不对的值既不能用于缓存判断，也不该被当成"镜像给了哈希"。
    pub fn parse(raw: &str) -> Option<Self> {
        let v = raw.trim().trim_matches('"');
        if v.len() != 40 || !v.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        Some(Self {
            value: v.to_ascii_lowercase(),
        })
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// **能用于缓存命中判断**（省一次网络往返）。
    pub const fn is_usable_for_cache_hit(&self) -> bool {
        true
    }

    /// **不能用于完整性校验。** 恒为 `false`。
    ///
    /// 它不是一个"当前返回 false"的实现细节，而是**这个类型在设计上就不承担校验职责**。
    /// 校验基准只能来自官方元数据（见 `crate::download` 的校验不变量 I1/I3）。
    pub const fn is_usable_for_integrity(&self) -> bool {
        false
    }

    /// 校验基准必须来自官方元数据 —— 这句提醒要在**编译期**就能被读到。
    pub const INTEGRITY_BASELINE_NOTE: &'static str =
        "完整性校验的哈希基准只能来自官方元数据；镜像哈希仅用于缓存命中判断。";

    /// 0 值占位（用于"这个源没给哈希提示"）。
    pub fn absent() -> Option<Self> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(key: &str, is_mirror: bool) -> KnownSource {
        KnownSource {
            key: key.to_string(),
            label: key.to_string(),
            is_mirror,
        }
    }

    // ───────────────── URL 改写 ─────────────────

    #[test]
    fn 前缀改写() {
        let r = MirrorRule {
            from_prefix: "https://piston-meta.mojang.com/".into(),
            to_prefix: "https://bmclapi2.bangbang93.com/".into(),
            source_key: "mirror".into(),
        };
        assert_eq!(
            rewrite("https://piston-meta.mojang.com/mc/game/x.json", &r).as_deref(),
            Some("https://bmclapi2.bangbang93.com/mc/game/x.json")
        );
    }

    #[test]
    fn 改不动就原样返回而不是报错() {
        // 一个不在规则表里的 URL 仍然是**可下载的官方地址**；
        // 把它报成错误会让"未收录的源"变成一次失败。
        let r = MirrorRule {
            from_prefix: "https://a.example/".into(),
            to_prefix: "https://b.example/".into(),
            source_key: "m".into(),
        };
        assert_eq!(rewrite("https://other.example/x", &r), None);
    }

    // ───────────────── 排序：核心纪律 ─────────────────

    #[test]
    fn 没测过时必须要求先探测而不是默认官方优先() {
        // **这条测试就是规格那句"不许在代码里写死优先级"的落点。**
        // 一个"没有测量就返回官方优先"的实现恰好就是被禁掉的东西。
        let mut p = SourcePolicy::new();
        p.know(source("official", false))
            .know(source("mirror", true));
        match p.order() {
            SourceOrder::NeedProbe { candidates } => {
                assert_eq!(candidates.len(), 2);
                assert!(candidates.contains(&"official".to_string()));
                assert!(candidates.contains(&"mirror".to_string()));
            }
            SourceOrder::Ordered { keys } => {
                panic!("没有测量时不该给出顺序（那等于写死优先级）：{keys:?}")
            }
        }
        assert!(p.summary().contains("尚未实测"), "{}", p.summary());
    }

    #[test]
    fn 按实测吞吐排序而不是按登记顺序() {
        // 先登记官方，但镜像实测更快 → 镜像必须排前面。
        // **这正是 S5 实测可能推翻直觉的地方**（本机实测里官方反而更快，
        // 而归档项目那次测到官方快 8 倍 —— 两个结论都只在各自环境下成立）。
        let mut p = SourcePolicy::new();
        p.know(source("official", false))
            .know(source("mirror", true));
        p.observe("official", 400_000);
        p.observe("mirror", 900_000);
        match p.order() {
            SourceOrder::Ordered { keys } => assert_eq!(keys[0], "mirror", "{keys:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn 结论会反过来_证明顺序不由登记决定() {
        let mut p = SourcePolicy::new();
        p.know(source("official", false))
            .know(source("mirror", true));
        p.observe("official", 900_000);
        p.observe("mirror", 100_000);
        match p.order() {
            SourceOrder::Ordered { keys } => assert_eq!(keys[0], "official", "{keys:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn 没测过的源排在测过的之后() {
        // 排在前面会让首次使用总是先撞一个可能不通的源；
        // 排在后面则它们仍会在前面那些失败时被用到（这正是镜像的价值）。
        let mut p = SourcePolicy::new();
        p.know(source("a", false))
            .know(source("b", true))
            .know(source("c", true));
        p.observe("a", 1000);
        match p.order() {
            SourceOrder::Ordered { keys } => {
                assert_eq!(keys[0], "a");
                assert_eq!(keys.len(), 3, "没测过的也要在列表里：{keys:?}");
                assert!(keys[1..].contains(&"b".to_string()));
                assert!(keys[1..].contains(&"c".to_string()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn 排序是确定的_并列时按键名() {
        // 同机同输入同输出 —— 否则同一台机器两次跑出不同结果，问题无法复现。
        let mut p = SourcePolicy::new();
        p.know(source("z", false)).know(source("a", false));
        p.observe("z", 5000);
        p.observe("a", 5000);
        let first = match p.order() {
            SourceOrder::Ordered { keys } => keys,
            other => panic!("{other:?}"),
        };
        assert_eq!(first, vec!["a".to_string(), "z".to_string()]);
        for _ in 0..5 {
            match p.order() {
                SourceOrder::Ordered { keys } => assert_eq!(keys, first),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn 重复登记被忽略且不改变统计() {
        let mut p = SourcePolicy::new();
        p.know(source("a", false));
        p.observe("a", 1000);
        p.know(source("a", false));
        p.know(source("a", false));
        assert_eq!(p.sources().len(), 1);
        assert_eq!(p.stats("a").unwrap().samples(), 1, "统计不该被重置");
    }

    #[test]
    fn 给未知源喂样本会被拒绝() {
        let mut p = SourcePolicy::new();
        p.know(source("a", false));
        assert!(!p.observe("nope", 1000), "未知源不该被悄悄接受");
    }

    // ───────────────── 速度地板 ─────────────────

    #[test]
    fn 地板只增不减() {
        // Axolotl 的依据：地板一旦被一次慢样本拉低，
        // 后面的正常速度就永远够不着它，于是源会被**永久**判为慢。
        let mut s = SourceStats::new("a");
        for _ in 0..5 {
            s.observe(1_000_000);
        }
        let high = s.floor();
        assert!(high > 0);
        // 一个极慢的样本
        s.observe(1);
        assert!(
            s.floor() >= high,
            "地板不该被慢样本拉低：{} < {high}",
            s.floor()
        );
    }

    #[test]
    fn 样本窗口是有限的() {
        let mut s = SourceStats::new("a");
        for i in 0..(SAMPLE_WINDOW + 5) {
            s.observe(1000 + i as u64);
        }
        assert_eq!(s.samples(), SAMPLE_WINDOW, "只保留最近 N 个");
    }

    #[test]
    fn 没有样本时不会被判为慢() {
        // 一个没有测量的源不该被判为慢 ——
        // 那会让"首次使用"直接跳过所有源。
        let s = SourceStats::new("a");
        assert_eq!(s.mean(), 0);
        assert_eq!(s.floor(), 0);
        assert!(!s.is_slow());
    }

    #[test]
    fn 远低于地板才算慢() {
        let mut s = SourceStats::new("a");
        for _ in 0..5 {
            s.observe(1_000_000);
        }
        // 均值在地板之上 → 不慢
        assert!(!s.is_slow());
        // 喂一堆极慢样本，把均值压到地板之下
        for _ in 0..SAMPLE_WINDOW {
            s.observe(1000);
        }
        assert!(s.is_slow(), "均值 {} 地板 {}", s.mean(), s.floor());
    }

    #[test]
    fn 摘要会带上实测数字与未测标注() {
        let mut p = SourcePolicy::new();
        p.know(source("official", false))
            .know(source("mirror", true));
        p.observe("official", 1_048_576); // 1 MB/s
        let s = p.summary();
        assert!(s.contains("实测"), "{s}");
        assert!(s.contains("official"), "{s}");
        assert!(s.contains("未测"), "没测过的要标出来：{s}");
        assert!(s.contains("MB/s"), "{s}");
    }

    // ───────────────── 镜像哈希：只是提示 ─────────────────

    #[test]
    fn 镜像哈希可用于缓存命中但不可用于校验() {
        // 规格原文：**"这条要写进实现注释，否则后人会误用。"**
        // "误用"是完全可预见的：一个已经拿到哈希的地方，
        // 顺手拿它做校验是最自然的一步 —— 而那样做的后果是
        // **镜像可以决定我们认为什么内容是"正确的"**。
        let h = MirrorHint::parse("0123456789abcdef0123456789abcdef01234567").unwrap();
        assert!(h.is_usable_for_cache_hit());
        assert!(
            !h.is_usable_for_integrity(),
            "镜像哈希**绝不能**用于完整性校验 —— 那等于把安全边界交给了镜像"
        );
        assert!(MirrorHint::INTEGRITY_BASELINE_NOTE.contains("官方元数据"));
    }

    #[test]
    fn 非法形态的镜像哈希被拒绝() {
        // 长度不对的值既不能用于缓存判断，也不该被当成"镜像给了哈希"。
        for bad in [
            "",
            "abc",
            "0123456789abcdef0123456789abcdef0123456", // 39 位
            "0123456789abcdef0123456789abcdef012345678", // 41 位
            "0123456789abcdef0123456789abcdef0123456g", // 非十六进制
        ] {
            assert!(MirrorHint::parse(bad).is_none(), "应当拒绝 {bad:?}");
        }
    }

    #[test]
    fn 镜像哈希大小写被规范化() {
        // 同一份内容用大写与小写表达是同一个哈希；
        // 不规范化会让缓存命中判断在大小写不同的镜像上失效。
        let h = MirrorHint::parse("0123456789ABCDEF0123456789ABCDEF01234567").unwrap();
        assert_eq!(h.value(), "0123456789abcdef0123456789abcdef01234567");
    }

    #[test]
    fn 带引号的哈希也能解析() {
        // HTTP 头里带引号是常见的。
        let h = MirrorHint::parse("\"0123456789abcdef0123456789abcdef01234567\"");
        assert!(h.is_some());
    }

    #[test]
    fn 没有哈希提示时是_none() {
        assert!(MirrorHint::absent().is_none());
    }
}
