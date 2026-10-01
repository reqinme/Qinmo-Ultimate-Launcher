//! 能力描述符 —— **跨内核 / Provider / 界面三层的契约**。
//!
//! 见方案 §3.3。三条规则：
//!
//! 1. **能力是"声明"，不是"判断"**：Provider 算好结果交上来，界面照着渲染。
//!    **不许在界面里做运行时判断来决定显示什么**（那必然腐化成
//!    `if product == "java"`）。
//! 2. **产品级 + 实例级两层**：产品级说"这个产品支不支持"，实例级说"这一个实例
//!    支不支持"（同一产品不同形态/版本/加载器，能力可以不同）。
//! 3. **`reason` 是必填项**：`enabled == false` 必须给出人话原因。
//!    **没有原因的禁用态一律视为缺陷。**
//!
//! 第 3 条无法靠"记得写"来保证，所以它由[类型系统](Capability)守住：
//! **构造禁用态的唯一途径是 `Capability::disabled(原因)`，而它拒绝空原因。**

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 一个能力的**稳定标识**。
///
/// **纪律**：能力 key **集中在这一个枚举里**，不许散落字符串
/// （方案 §3.3 变更流程第 1 步："枚举/常量集中在一处"）。
/// 这样加能力时编译期就能发现所有遗漏点。
///
/// `#[non_exhaustive]` 是有意的：**新增 key 允许发生，匹配它的下游必须处理默认分支**
/// ——这正是"加能力不用改主程序别的地方"的机制保证。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CapabilityKey {
    // ── 入口与生命周期 ────────────────────────────────────────
    /// 能创建新实例
    CreateInstance,
    /// 能启动
    Launch,
    /// 启动前能做预检（依赖/内存/运行时）
    Preflight,

    // ── 内容管理（产品相关区，界面按 enabled 才渲染）──────────
    /// 能管理模组
    Mods,
    /// 能管理资源包/行为包
    ResourcePacks,
    /// 能管理光影
    Shaders,
    /// 能管理存档
    Worlds,
    /// 能管理配置
    Configs,
    /// 能截图/管理截图
    Screenshots,

    // ── 稳定与诊断 ────────────────────────────────────────────
    /// 能捕获崩溃并归因
    CrashAnalysis,
    /// 能把日志分级过滤
    LogFiltering,
    /// 支持实例快照与回滚
    Snapshots,

    // ── 隔离与安全 ────────────────────────────────────────────
    /// 能提供实例隔离（进程/文件/账户）
    ///
    /// **注意**：这是"如实声明"的能力，不是"必须为真"的能力。
    /// 基岩版的 UWP 形态做不到时，**如实报 `false` 并给原因**，
    /// 而不是假装支持（方案 §3.4）。
    Isolation,
    /// 能离线游玩
    OfflinePlay,
}

impl CapabilityKey {
    /// 全部 key，**顺序固定**（供界面遍历、供测试断言"无遗漏"）。
    ///
    /// 顺序即界面里的显示顺序——所以它是**产品决策**，不是实现细节。
    pub const ALL: &'static [CapabilityKey] = &[
        Self::CreateInstance,
        Self::Launch,
        Self::Preflight,
        Self::Mods,
        Self::ResourcePacks,
        Self::Shaders,
        Self::Worlds,
        Self::Configs,
        Self::Screenshots,
        Self::CrashAnalysis,
        Self::LogFiltering,
        Self::Snapshots,
        Self::Isolation,
        Self::OfflinePlay,
    ];

    /// 稳定性标识（落进 `profile.json` 与前后端消息里）。
    ///
    /// **它就是契约本身**——改名等于破坏兼容，所以要与枚举名一起评审。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CreateInstance => "create_instance",
            Self::Launch => "launch",
            Self::Preflight => "preflight",
            Self::Mods => "mods",
            Self::ResourcePacks => "resource_packs",
            Self::Shaders => "shaders",
            Self::Worlds => "worlds",
            Self::Configs => "configs",
            Self::Screenshots => "screenshots",
            Self::CrashAnalysis => "crash_analysis",
            Self::LogFiltering => "log_filtering",
            Self::Snapshots => "snapshots",
            Self::Isolation => "isolation",
            Self::OfflinePlay => "offline_play",
        }
    }

    /// **产品级**还是**实例级**（方案 §3.3 变更流程第 2 步）。
    ///
    /// - **产品级**：整个产品要么支持要么不支持，与具体实例无关。
    /// - **实例级**：同一产品下不同实例可以不同（形态 / 版本 / 加载器 /
    ///   账号类型 / 系统环境）。
    ///
    /// 实例级的能力**必须**在实例描述符里被求值；产品级只作为默认值继承。
    pub const fn kind(self) -> CapabilityKind {
        match self {
            // 隔离能力随形态变（GDK vs UWP），所以是实例级
            Self::Isolation | Self::Snapshots => CapabilityKind::Instance,
            _ => CapabilityKind::Product,
        }
    }
}

/// 能力的作用域（见 [`CapabilityKey::kind`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    /// 产品级：整个产品一个答案
    Product,
    /// 实例级：每个实例可以不同
    Instance,
}

/// 构造禁用态时**原因为空**。
///
/// 这个错误类型存在的唯一目的是：让"没有原因的禁用态"**在编译期之后、
/// 运行期之前就撞墙**，而不是等到界面上出现一个没有解释的灰按钮。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("禁用态必须给出非空原因（方案 §3.3 规则 3）")]
pub struct ReasonError;

/// 一个能力在某一层（产品 / 实例）的**结论**。
///
/// 字段私有是**刻意的**：外部只能经 [`Capability::enabled`] 或
/// [`Capability::disabled`] 构造，因此**不可能造出"禁用但没原因"的值**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CapabilityWire")]
pub struct Capability {
    enabled: bool,
    /// `enabled == false` 时必为非空；`enabled == true` 时为 `None`。
    reason: Option<String>,
}

impl Capability {
    /// 可用。
    pub const fn enabled() -> Self {
        Self {
            enabled: true,
            reason: None,
        }
    }

    /// 不可用，**必须给人话原因**。
    ///
    /// 原因是**面向用户**的，不是给开发者看的错误码：
    /// 好例子 → `"基岩版不支持光影"`；坏例子 → `"unsupported"` / `""`。
    pub fn disabled(reason: impl Into<String>) -> Result<Self, ReasonError> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(ReasonError);
        }
        Ok(Self {
            enabled: false,
            reason: Some(reason),
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 禁用原因。`enabled` 为真时恒为 `None`。
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

/// 反序列化的中转形状。
///
/// **它存在的理由是：JSON 也必须遵守那条规则。**
/// 若直接 derive 反序列化，配置文件或前端就能塞进
/// `{"enabled": false}`（无原因）或 `{"enabled": false, "reason": "  "}`，
/// 绕过类型系统。这里把校验补回到边界上。
#[derive(Deserialize)]
struct CapabilityWire {
    enabled: bool,
    #[serde(default)]
    reason: Option<String>,
}

impl TryFrom<CapabilityWire> for Capability {
    type Error = ReasonError;

    fn try_from(w: CapabilityWire) -> Result<Self, Self::Error> {
        if w.enabled {
            Ok(Self::enabled())
        } else {
            match w.reason {
                Some(r) => Self::disabled(r),
                None => Err(ReasonError),
            }
        }
    }
}

/// 一组能力结论（产品级或实例级各持有一个）。
///
/// 内部用 `BTreeMap` 而非 `HashMap`：**输出稳定**（同样的输入给同样的
/// JSON 字节序），这让我们能对前后端契约做快照测试。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capabilities(BTreeMap<CapabilityKey, Capability>);

impl Capabilities {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// 设定一个能力；**禁用态的原因由 [`Capability`] 保证非空**。
    pub fn set(&mut self, key: CapabilityKey, cap: Capability) -> &mut Self {
        self.0.insert(key, cap);
        self
    }

    pub fn get(&self, key: CapabilityKey) -> Option<&Capability> {
        self.0.get(&key)
    }

    /// 该层是否声明了这个能力。
    ///
    /// **未声明 ≠ 禁用**：未声明表示"这一层没意见"（由上层决定），
    /// 禁用表示"这一层明确说不行，并给了原因"。
    pub fn contains(&self, key: CapabilityKey) -> bool {
        self.0.contains_key(&key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (CapabilityKey, &Capability)> {
        self.0.iter().map(|(k, v)| (*k, v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 叠加：`overrides` 里声明了的 key 覆盖当前值，其余保留。
    ///
    /// 这是实现"产品级给默认、实例级覆盖"的那一步——
    /// **子层只声明差异，不重复整张表**。
    pub fn merged_with(&self, overrides: &Capabilities) -> Capabilities {
        let mut out = self.clone();
        for (k, v) in overrides.iter() {
            out.0.insert(k, v.clone());
        }
        out
    }

    /// 一致性自检：**任何禁用态必须有非空原因**。
    ///
    /// 正常路径下这个检查永远不会失败（类型系统已守住）。它存在是为了
    /// 覆盖**反序列化与其他语言的输入**这两条绕行路径——
    /// 前者已由 [`CapabilityWire`] 拦截，本方法用于测试与运行时断言。
    pub fn validate(&self) -> Result<(), (CapabilityKey, ReasonError)> {
        for (k, v) in self.iter() {
            if !v.is_enabled() && v.reason().map_or(true, |r| r.trim().is_empty()) {
                return Err((k, ReasonError));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 禁用态必须有原因() {
        assert_eq!(Capability::disabled(""), Err(ReasonError));
        assert_eq!(Capability::disabled("   "), Err(ReasonError));
        assert!(Capability::disabled("基岩版不支持光影").is_ok());
    }

    #[test]
    fn 可用态不带原因() {
        let c = Capability::enabled();
        assert!(c.is_enabled());
        assert_eq!(c.reason(), None);
    }

    #[test]
    fn json_也绕不过规则() {
        // 缺原因 → 反序列化失败
        assert!(serde_json::from_str::<Capability>(r#"{"enabled":false}"#).is_err());
        // 原因全是空白 → 失败
        assert!(serde_json::from_str::<Capability>(r#"{"enabled":false,"reason":"  "}"#).is_err());
        // 合法禁用态 → 成功，且原因被保住
        let c: Capability =
            serde_json::from_str(r#"{"enabled":false,"reason":"基岩版不支持光影"}"#).unwrap();
        assert_eq!(c.reason(), Some("基岩版不支持光影"));
    }

    #[test]
    fn 序列化后仍能反序列化回来() {
        let mut caps = Capabilities::new();
        caps.set(CapabilityKey::Mods, Capability::enabled());
        caps.set(
            CapabilityKey::Shaders,
            Capability::disabled("基岩版不支持光影").unwrap(),
        );
        let json = serde_json::to_string(&caps).unwrap();
        let back: Capabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(caps, back);
    }

    #[test]
    fn 叠加时子层只覆盖声明的键() {
        let mut product = Capabilities::new();
        product.set(CapabilityKey::Mods, Capability::enabled());
        product.set(CapabilityKey::Isolation, Capability::enabled());

        let mut instance = Capabilities::new();
        instance.set(
            CapabilityKey::Isolation,
            Capability::disabled("UWP 形态无法隔离").unwrap(),
        );

        let merged = product.merged_with(&instance);
        assert!(merged.get(CapabilityKey::Mods).unwrap().is_enabled());
        assert!(!merged.get(CapabilityKey::Isolation).unwrap().is_enabled());
        assert_eq!(merged.len(), 2, "叠加不应新增未声明的键");
    }

    #[test]
    fn 未声明与禁用是两件事() {
        let mut caps = Capabilities::new();
        caps.set(
            CapabilityKey::Shaders,
            Capability::disabled("基岩版不支持光影").unwrap(),
        );
        assert!(caps.contains(CapabilityKey::Shaders));
        assert!(!caps.contains(CapabilityKey::Mods), "未声明 ≠ 禁用");
    }
}
