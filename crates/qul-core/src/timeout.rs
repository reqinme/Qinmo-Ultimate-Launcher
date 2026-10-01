//! # 分级超时、慢站记忆与失败聚合（**纯规则，零 IO**）
//!
//! 依据 `docs/下载引擎设计规格.md` §4（含 NexBox 的"敌意服务器"宽容档）
//! 与 §8 的验收项 6/7/9。
//!
//! ## 本模块要解决的问题
//!
//! 一个**统一的**超时值在任何方向上都错：
//!
//! | 若用 | 后果 |
//! |---|---|
//! | 短（5s） | **已知慢但健康的站**（归档站、镜像冷缓存）会被反复判失败 |
//! | 长（60s） | 一个**真的挂了**的站会让用户等一分钟才知道 |
//!
//! 所以规格分了两档，并且**"它是不是慢站"必须被记住** ——
//! 而记忆这件事本身有一条纪律：**记忆的键是域名，不是 URL**。
//! 同一个站的不同文件共享同一条带宽曲线，而把键做成 URL 会让
//! "每一个文件都要重新学一遍它很慢"。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 停滞超时的两档（规格 §4 的确切取值）。
pub const STALL_NORMAL_MS: u64 = 5_000;
/// **敌意服务器**（已知"慢但会回数据"）的宽容档。
pub const STALL_HOSTILE_MS: u64 = 60_000;

/// 连接/探测阶段的超时（规格 §4：探测 3 次 / 15s）。
pub const PROBE_TIMEOUT_MS: u64 = 15_000;
pub const PROBE_RETRIES: u32 = 3;

/// 最大重试（规格 §4：**5**）。
///
/// ⚠️ **我们此前用的是 4**（`RetryPolicy::default_for_network`），
/// 而规格写的是 5（NexBox `:822`，HMCL 也用 5）。
/// 本模块给出正确的常量，而 `download` 的默认配置改用它 ——
/// 详见 [`TimeoutPolicy::retry`]。
pub const MAX_RETRIES: u32 = 5;
/// 退避基础（规格 §4：**2s**，优于 HMCL 的固定 200ms）。
pub const BACKOFF_BASE_MS: u64 = 2_000;
/// 退避上限。**规格没写**，但没有上限的指数退避会出现"等 8 分钟"这种事。
/// 所以这里取一个保守值，并把"规格没写"记在注释里 ——
/// 免得后人以为它是抄来的。
pub const BACKOFF_CAP_MS: u64 = 30_000;

/// 停滞超时该用哪一档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallTier {
    Normal,
    Hostile,
}

impl StallTier {
    pub const fn timeout_ms(self) -> u64 {
        match self {
            StallTier::Normal => STALL_NORMAL_MS,
            StallTier::Hostile => STALL_HOSTILE_MS,
        }
    }
}

/// 超时与重试策略（规格 §4 的全部取值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutPolicy {
    pub max_retries: u32,
    pub backoff_base_ms: u64,
    pub backoff_cap_ms: u64,
    pub probe_retries: u32,
    pub probe_timeout_ms: u64,
    pub stall: StallTier,
}

impl Default for TimeoutPolicy {
    /// 规格 §4 的默认值。
    fn default() -> Self {
        Self {
            max_retries: MAX_RETRIES,
            backoff_base_ms: BACKOFF_BASE_MS,
            backoff_cap_ms: BACKOFF_CAP_MS,
            probe_retries: PROBE_RETRIES,
            probe_timeout_ms: PROBE_TIMEOUT_MS,
            stall: StallTier::Normal,
        }
    }
}

impl TimeoutPolicy {
    /// 转成 `retry::RetryPolicy`。
    ///
    /// **`max_attempts` = `max_retries` + 1**（含第一次）——
    /// 这一条很容易搞反，而搞反的后果是"少试一次"，
    /// 表现为"有时候网络抖一下就失败了"。
    pub fn retry(self) -> crate::retry::RetryPolicy {
        crate::retry::RetryPolicy {
            max_attempts: self.max_retries.saturating_add(1),
            backoff: crate::retry::Backoff::Exponential {
                first_ms: self.backoff_base_ms,
                factor: 2,
                cap_ms: self.backoff_cap_ms,
            },
        }
    }

    /// 第 `attempt` 次重试前的等待（毫秒）。
    pub fn delay_ms(self, attempt: u32) -> u64 {
        self.retry().backoff.delay_ms(attempt)
    }

    /// 换成宽容档（用于已知慢站）。
    pub const fn hostile(self) -> Self {
        Self {
            stall: StallTier::Hostile,
            ..self
        }
    }
}

// ───────────────────────── 慢站记忆（规格 §8 验收项 7）─────────────────────────

/// 记忆的有效期（规格 §7.2 第 4 项：**24h**）。
pub const MEMORY_TTL_SECS: u64 = 24 * 60 * 60;

/// 一个域名的记忆。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainMemory {
    /// 这个域名是否"慢但会回数据"
    pub hostile: bool,
    /// 上次记下的**最大可用并发**（规格 §7.2 第 4 项：
    /// "避免对同一站点反复试探最优并发"）
    pub max_concurrency: u32,
    /// 记下的时刻（Unix 秒）
    pub recorded_at: u64,
}

impl DomainMemory {
    /// 是否还有效（24h 内）。
    ///
    /// ⚠️ **过期不是"删掉"而是"当成没有"** ——
    /// 调用方不需要做清理，而"读的时候判断"让过期逻辑只有一个地方。
    pub const fn is_fresh(&self, now: u64) -> bool {
        now.saturating_sub(self.recorded_at) < MEMORY_TTL_SECS
    }
}

/// **按域名**的记忆表。
///
/// ## 为什么键是域名而不是 URL
///
/// 同一个站的不同文件共享同一条带宽曲线。把键做成 URL 会让
/// **每一个文件都要重新学一遍"它很慢"** —— 而"学"的代价是
/// 用户在那个站上的每个文件都先经历一次 5s 超时失败。
///
/// ## 为什么它可以用 `BTreeMap` 而不是更花的结构
///
/// 因为域名数量**是几十量级**（一个启动器会碰到的站就那么些）。
/// 用一个哈希表去优化几十个键是**在没有问题的地方引入复杂度**。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostMemory {
    map: BTreeMap<String, DomainMemory>,
}

impl HostMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// 从一个 URL 取出域名键（**小写**）。
    ///
    /// 失败时返回 `None` 而不是一个空串 —— 一个空串键会让
    /// **所有无法解析的 URL 共享同一条记忆**，而那会把一个慢站的宽容档
    /// 传染给全部站点。
    pub fn host_of(url: &str) -> Option<String> {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let host = rest.split(['/', '?', '#']).next()?;
        // 去掉端口与可能的 userinfo
        let host = host.rsplit('@').next()?;
        let host = host.split(':').next()?;
        if host.is_empty() {
            return None;
        }
        Some(host.to_ascii_lowercase())
    }

    /// 查一个域名的**有效**记忆。过期的会被当成没有。
    pub fn get(&self, host: &str, now: u64) -> Option<&DomainMemory> {
        self.map.get(host).filter(|m| m.is_fresh(now))
    }

    /// 记下"这个域名是慢站"。
    pub fn mark_hostile(&mut self, host: impl Into<String>, now: u64) -> &mut Self {
        let e = self.map.entry(host.into()).or_insert_with(|| DomainMemory {
            hostile: false,
            max_concurrency: 0,
            recorded_at: now,
        });
        e.hostile = true;
        e.recorded_at = now;
        self
    }

    /// 记下"这个域名最多能用几路并发"。
    ///
    /// **只增不减的语义由调用方决定**：这里**覆盖写**，
    /// 因为"最优并发"会随网络环境变化，而一个"只增"的并发上限
    /// 会在用户换到更好的网络后**永远限制他**。
    ///
    /// （对比 [`crate::download`] 的**速度地板**是"只增不减" ——
    /// 那是对的，因为地板是"慢"的下界；而上限是"快"的界。）
    pub fn record_concurrency(&mut self, host: impl Into<String>, n: u32, now: u64) -> &mut Self {
        let e = self.map.entry(host.into()).or_insert_with(|| DomainMemory {
            hostile: false,
            max_concurrency: 0,
            recorded_at: now,
        });
        e.max_concurrency = n;
        e.recorded_at = now;
        self
    }

    /// 给一个 URL 决定该用哪一档超时。
    ///
    /// 这是本类型的**主要用途**，所以它有一个直接的入口 ——
    /// 免得每个调用点各写一遍"取域名 → 查记忆 → 判过期"。
    pub fn tier_for(&self, url: &str, now: u64) -> StallTier {
        match Self::host_of(url).and_then(|h| self.get(&h, now)) {
            Some(m) if m.hostile => StallTier::Hostile,
            _ => StallTier::Normal,
        }
    }

    /// 给一个 URL 取记忆过的并发上限（没有则 `None`）。
    pub fn concurrency_for(&self, url: &str, now: u64) -> Option<u32> {
        let h = Self::host_of(url)?;
        let m = self.get(&h, now)?;
        if m.max_concurrency == 0 {
            None
        } else {
            Some(m.max_concurrency)
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// ───────────────────────── 失败聚合（规格 §8 验收项 9）─────────────────────────

/// 一个源的失败原因。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFailure {
    pub source_key: String,
    pub reason: String,
}

/// **多候选全失败时，抛最后一条并把其余挂为 suppressed。**
///
/// 规格 §4 的依据（HMCL `FetchTask.java:136-142`）：
///
/// > 多候选全失败时，抛最后一条并把其余挂为 suppressed
/// > → **UI 能展示"试了 3 个源分别怎么失败的"**
///
/// **为什么要聚合而不是只报最后一条**：只报最后一条时，
/// 用户看到的是"官方源失败"，而**真正的原因可能在前两个源里**
/// （例如"官方不通"与"镜像返回了错误的哈希"是两件完全不同的事）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregatedFailure {
    /// 最后一条失败（**它就是"主因"**）
    pub last: SourceFailure,
    /// 其余（suppressed）。**顺序保持尝试顺序**。
    pub suppressed: Vec<SourceFailure>,
}

impl AggregatedFailure {
    /// 从按尝试顺序排列的失败列表构造。
    ///
    /// **空列表返回 `None`** —— 没有失败就不该有聚合结果，
    /// 而返回一个"空的聚合"会让调用方误以为"试过了但都失败"。
    pub fn from_attempts(attempts: Vec<SourceFailure>) -> Option<Self> {
        let mut it = attempts;
        let last = it.pop()?;
        Some(Self {
            last,
            suppressed: it,
        })
    }

    /// 尝试过的源数。
    pub fn tried(&self) -> usize {
        1 + self.suppressed.len()
    }

    /// 一行摘要（**界面要能一次展示全部原因**）。
    pub fn summary(&self) -> String {
        let mut parts = vec![format!("{}：{}", self.last.source_key, self.last.reason)];
        for s in &self.suppressed {
            parts.push(format!("{}：{}", s.source_key, s.reason));
        }
        format!("试了 {} 个源 —— {}", self.tried(), parts.join("；"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────── 规格取值 ─────────────────

    #[test]
    fn 规格取值的常量对得上() {
        // 这些数字全部来自规格 §4 的表格。**把它们钉住**，
        // 因为一个被改小的超时会让"慢站"变成"失败"，而那种改动
        // 在代码审查里看起来完全无害。
        assert_eq!(MAX_RETRIES, 5);
        assert_eq!(BACKOFF_BASE_MS, 2000);
        assert_eq!(STALL_NORMAL_MS, 5000);
        assert_eq!(STALL_HOSTILE_MS, 60_000);
        assert_eq!(PROBE_TIMEOUT_MS, 15_000);
        assert_eq!(PROBE_RETRIES, 3);
        assert_eq!(MEMORY_TTL_SECS, 86_400);
    }

    #[test]
    fn max_attempts_含第一次() {
        // **很容易搞反**，而搞反的后果是"少试一次"，
        // 表现为"有时候网络抖一下就失败了"。
        let p = TimeoutPolicy::default();
        assert_eq!(p.retry().max_attempts, MAX_RETRIES + 1);
        assert_eq!(p.retry().max_attempts, 6);
    }

    #[test]
    fn 退避从两秒开始且翻倍且有上限() {
        let p = TimeoutPolicy::default();
        assert_eq!(p.delay_ms(0), 0, "第一次尝试立刻做");
        assert_eq!(p.delay_ms(1), 2000);
        assert_eq!(p.delay_ms(2), 4000);
        assert_eq!(p.delay_ms(3), 8000);
        assert_eq!(p.delay_ms(4), 16_000);
        assert_eq!(p.delay_ms(5), 30_000, "封顶");
        // 规格没写上限；这条注释与断言一起说明它是我们加的
        assert_eq!(p.delay_ms(50), BACKOFF_CAP_MS);
    }

    #[test]
    fn 最坏情况下总等待是可接受的() {
        // 5 次重试的总等待不该是"分钟级" —— 否则用户会以为卡死。
        // （对比：没有上限时 2+4+8+16+32 = 62 秒，而封顶后是 2+4+8+16+30 = 60 秒。
        // 这条断言的意义是**把这个数字写在测试里**，让改动它的人看到后果。）
        let p = TimeoutPolicy::default();
        let total: u64 = (1..=p.max_retries).map(|a| p.delay_ms(a)).sum();
        assert_eq!(total, 60_000, "总等待应当是 60 秒");
        assert!(total <= 90_000, "不该超过一分半");
    }

    #[test]
    fn 宽容档把停滞超时放宽十二倍() {
        let p = TimeoutPolicy::default();
        assert_eq!(p.stall, StallTier::Normal);
        assert_eq!(p.hostile().stall.timeout_ms(), 60_000);
        assert_eq!(StallTier::Normal.timeout_ms(), 5_000);
        assert_eq!(
            StallTier::Hostile.timeout_ms() / StallTier::Normal.timeout_ms(),
            12
        );
    }

    // ───────────────── 慢站记忆（验收项 7）─────────────────

    #[test]
    fn 域名提取() {
        assert_eq!(
            HostMemory::host_of("https://piston-meta.mojang.com/mc/game/x.json?v=1").as_deref(),
            Some("piston-meta.mojang.com")
        );
        assert_eq!(
            HostMemory::host_of("http://bmclapi2.bangbang93.com:8080/a").as_deref(),
            Some("bmclapi2.bangbang93.com"),
            "端口要去掉"
        );
        assert_eq!(
            HostMemory::host_of("https://User@Example.COM/x").as_deref(),
            Some("example.com"),
            "userinfo 要去掉且域名小写"
        );
        // 无法解析的返回 None —— **绝不能返回空串**，
        // 否则所有无法解析的 URL 会共享同一条记忆、
        // 于是一个慢站的宽容档会传染给全部站点。
        for bad in ["", "ftp://x/y", "not a url", "https:///x"] {
            assert_eq!(HostMemory::host_of(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn 慢站记忆按域名生效而不是按_url() {
        // 同一个站的不同文件共享同一条带宽曲线。
        // 把键做成 URL 会让**每一个文件都要重新学一遍"它很慢"**。
        let mut m = HostMemory::new();
        m.mark_hostile("slow.example.com", 1000);
        assert_eq!(
            m.tier_for("https://slow.example.com/a.jar", 1001),
            StallTier::Hostile
        );
        assert_eq!(
            m.tier_for("https://slow.example.com/deep/path/b.jar", 1001),
            StallTier::Hostile,
            "另一个文件也该享受宽容档"
        );
        // 别的站不受影响
        assert_eq!(
            m.tier_for("https://fast.example.com/a.jar", 1001),
            StallTier::Normal
        );
    }

    #[test]
    fn 记忆会在二十四小时后失效() {
        let mut m = HostMemory::new();
        m.mark_hostile("slow.example.com", 1000);
        assert_eq!(
            m.tier_for("https://slow.example.com/x", 1000),
            StallTier::Hostile
        );
        // 刚好 24h：`now - recorded < TTL` 为假 → 过期
        let just_expired = 1000 + MEMORY_TTL_SECS;
        assert_eq!(
            m.tier_for("https://slow.example.com/x", just_expired),
            StallTier::Normal,
            "24h 后应当失效（否则一个网络环境变化后的临时结论会被永久沿用）"
        );
        // 而记忆**没被删掉**（读的时候判断过期，只有一个地方管这件事）
        assert_eq!(m.len(), 1);
        // 重新标记后又有效
        m.mark_hostile("slow.example.com", just_expired);
        assert_eq!(
            m.tier_for("https://slow.example.com/x", just_expired + 1),
            StallTier::Hostile
        );
    }

    #[test]
    fn 记下的并发上限可以读回来且会过期() {
        let mut m = HostMemory::new();
        assert_eq!(m.concurrency_for("https://a.example.com/x", 0), None);
        m.record_concurrency("a.example.com", 8, 100);
        assert_eq!(m.concurrency_for("https://a.example.com/x", 101), Some(8));
        assert_eq!(
            m.concurrency_for("https://a.example.com/x", 100 + MEMORY_TTL_SECS),
            None,
            "过期后不该再限制用户"
        );
    }

    #[test]
    fn 并发上限是覆盖而不是只增() {
        // 对比 `download` 的**速度地板**（只增不减，那是对的 ——
        // 地板是"慢"的下界）。
        // 而并发上限是"快"的界：一个"只增"的上限会在用户换到更好的网络后
        // **永远限制他**。
        let mut m = HostMemory::new();
        m.record_concurrency("a.example.com", 4, 100);
        assert_eq!(m.concurrency_for("https://a.example.com/x", 100), Some(4));
        m.record_concurrency("a.example.com", 16, 200);
        assert_eq!(
            m.concurrency_for("https://a.example.com/x", 200),
            Some(16),
            "更好的网络应当能提高上限"
        );
    }

    #[test]
    fn 标记慢站不会清掉已记的并发上限() {
        // 两件事是独立的：一个站可以"慢"且"需要低并发"，
        // 也可以"慢"但"其实能扛高并发"。
        let mut m = HostMemory::new();
        m.record_concurrency("a.example.com", 3, 100);
        m.mark_hostile("a.example.com", 150);
        assert_eq!(m.concurrency_for("https://a.example.com/x", 150), Some(3));
        assert_eq!(
            m.tier_for("https://a.example.com/x", 150),
            StallTier::Hostile
        );
    }

    #[test]
    fn 记忆表可以序列化_因为它要跨会话存活() {
        // 一个"只活在内存里"的记忆是没用的：
        // 用户每次启动都要重新学一遍"那个站很慢"。
        let mut m = HostMemory::new();
        m.mark_hostile("slow.example.com", 1000);
        m.record_concurrency("slow.example.com", 2, 1000);
        let j = serde_json::to_string(&m).unwrap();
        let back: HostMemory = serde_json::from_str(&j).unwrap();
        assert_eq!(back, m);
    }

    // ───────────────── 失败聚合（验收项 9）─────────────────

    #[test]
    fn 聚合保留全部原因而不是只留最后一条() {
        // 规格：**UI 能展示"试了 3 个源分别怎么失败的"**。
        // 只报最后一条时，用户看到"官方源失败"，
        // 而真正的原因可能在前两个里 ——
        // 例如"官方不通"与"镜像返回了错误的哈希"是完全不同的事。
        let f = AggregatedFailure::from_attempts(vec![
            SourceFailure {
                source_key: "mirror".into(),
                reason: "哈希不匹配".into(),
            },
            SourceFailure {
                source_key: "official".into(),
                reason: "连接超时".into(),
            },
            SourceFailure {
                source_key: "other".into(),
                reason: "HTTP 503".into(),
            },
        ])
        .unwrap();

        assert_eq!(f.tried(), 3);
        assert_eq!(f.last.source_key, "other", "最后一条就是主因");
        assert_eq!(f.suppressed.len(), 2);
        // **顺序保持尝试顺序**
        assert_eq!(f.suppressed[0].source_key, "mirror");
        assert_eq!(f.suppressed[1].source_key, "official");

        let s = f.summary();
        for k in ["mirror", "official", "other"] {
            assert!(s.contains(k), "摘要里必须有 {k}：{s}");
        }
        assert!(s.contains("3 个源"), "{s}");
    }

    #[test]
    fn 空失败列表不产生聚合结果() {
        // 返回一个"空的聚合"会让调用方误以为"试过了但都失败"。
        assert!(AggregatedFailure::from_attempts(vec![]).is_none());
    }

    #[test]
    fn 单源失败也能聚合() {
        let f = AggregatedFailure::from_attempts(vec![SourceFailure {
            source_key: "only".into(),
            reason: "超时".into(),
        }])
        .unwrap();
        assert_eq!(f.tried(), 1);
        assert!(f.suppressed.is_empty());
    }
}
