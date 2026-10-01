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
//!
//! # 产品维度放在哪一层（**这一节是修正后加的，理由必须留着**）
//!
//! **起因**：给基岩版补能力时连着别扭两次——禁用词表把"产品名"与"内容类型"混在一起、
//! 通用项与产品专属项挤在同一个列表里。查了同类实现（Portal 同时支持 Java 与基岩）
//! 之后看清了根因：**我把"详情层"的东西放进了"实例层"的枚举。**
//!
//! **reference（Portal `MinecraftInstance.cs`）的分层，值得记**：
//!
//! | 层 | 它放什么 | 是否分产品 |
//! |---|---|---|
//! | 实例层 | 目录、名称、图标、备注、收藏、游玩时长、磁盘占用 | **完全通用** |
//! | 详情层 | Java：JVM 参数 / 内存 / 独立实例；基岩：`BedrockInstanceConfig` | **各自一套** |
//!
//! **但它用 `if (Type == Java)` 贯穿全局**：UI 层 **79 处 / 19 个文件**，Core 层 **55 处**。
//! 那是方案 §3.3 明确要消灭的写法，**所以我们不照抄它的写法，只采纳它的分层**。
//!
//! **因此本模块分两层**：
//!
//! - [`CapabilityKey`]：**通用**能力——启动、预检、世界、配置、截图、崩溃分析……
//!   任何产品都成立，**它的成员里不许出现产品专属概念**（由架构测试强制）。
//! - [`InstanceDetail`]：**产品专属**详情——一个实例**恰好持有一种**。
//!   产品专属的能力项（光影 / 行为包 / 皮肤包 / 加载器 / 依赖组件）挂在它上面。
//!
//! **诚实记下这条边界**：**"界面完全不提到产品"做不到 100% 干净。**
//! 因为"哪个产品会出现哪些条目"这份数据**必然要提到产品**——
//! 我们能做的是**把它压缩到 Provider 侧一处**，而不是消灭它。
//! （对照：Portal 是 134 处；我们的目标是**一处**。）

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
    /// 能检测本机已安装的实例（每个产品查法不同：普通路径 / 商店包）
    DetectInstalled,

    // ── 内容管理（**通用**部分）─────────────────────────────
    //
    // ⚠️ **这一组只放"任何产品都成立"的项。**
    // 产品专属的（光影 / 行为包 / 皮肤包 / 加载器 / 依赖组件）**不在这里**，
    // 它们在 [`InstanceDetail`] 上——见本模块文档"产品维度放在哪一层"。
    /// 能管理模组类内容
    Mods,
    /// 能管理资源包
    ResourcePacks,
    /// 能管理世界/存档
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
        Self::DetectInstalled,
        Self::Mods,
        Self::ResourcePacks,
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
            Self::DetectInstalled => "detect_installed",
            Self::Mods => "mods",
            Self::ResourcePacks => "resource_packs",
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
            // 隔离与快照随形态变（同产品内 GDK 与 UWP 差别极大），所以是实例级
            Self::Isolation | Self::Snapshots => CapabilityKind::Instance,
            _ => CapabilityKind::Product,
        }
    }
}

impl InstanceDetail {
    /// **产品专属**能力的完整列表（跨所有产品的并集），**顺序固定**。
    ///
    /// 界面的**第二级左导航** = 通用能力（[`CapabilityKey::ALL`] 里 enabled 的）
    /// **＋** 本产品详情里 enabled 的项。**界面不写任何产品判断。**
    pub const ALL_KEYS: &'static [DetailKey] = &[
        DetailKey::Shaders,
        DetailKey::BehaviorPacks,
        DetailKey::SkinPacks,
        DetailKey::Loaders,
        DetailKey::ProductDependencies,
    ];
}

/// **产品专属**能力的标识。
///
/// **为什么这些不放在 [`CapabilityKey`] 里**（这是修正后的结构，理由见模块文档）：
/// 它们**不是"任何产品都成立的能力"**，而是**某个产品详情的一部分**。
/// 混进通用枚举会导致：
/// - 通用项与产品专属项挤在同一张表，界面分不清哪些要按产品筛选
/// - 架构测试的禁用词表把"产品名"与"内容类型"混在一起（这个坑踩过）
///
/// **`Loaders` 与 `Shell`（启动通道）的差别**：`Loaders` 是"能装加载器"，
/// 属 Java 详情；基岩的"依赖组件"属基岩详情。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DetailKey {
    /// 能管理光影 —— **Java 详情**
    Shaders,
    /// 能管理行为包 —— **基岩详情**（"能力驱动界面"最典型的一处：
    /// 切到基岩版实例时「光影」消失、「行为包」出现，见方案 §3.3）
    BehaviorPacks,
    /// 能管理皮肤包 —— **基岩详情**
    SkinPacks,
    /// 能安装与管理加载器（Fabric / Forge / NeoForge …）—— **Java 详情**
    Loaders,
    /// 能检测并引导安装依赖组件（Gaming Services / GameInput）—— **基岩详情**
    ProductDependencies,
}

impl DetailKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shaders => "shaders",
            Self::BehaviorPacks => "behavior_packs",
            Self::SkinPacks => "skin_packs",
            Self::Loaders => "loaders",
            Self::ProductDependencies => "product_dependencies",
        }
    }
}

/// 一个实例的**产品专属详情**——**恰好持有一种**。
///
/// **"恰好一种"由类型保证**：不存在"两种都是"，也不存在"两者都不是"。
/// 这是把产品差异**收敛到一处**的落点。
///
/// ## ⚠️ 为什么这里的变体名**不带具体产品名**
///
/// 曾经用过 `Java` / `Bedrock` 这类命名，被架构测试当场拦下
/// （"生产代码/字符串中出现 bedrock"）。**拦住是对的**，理由有两条：
///
/// 1. **与 [`DetailKey`] 的命名原则不一致**：那里是**按能力**命名
///    （中性的 `BehaviorPacks`，不带产品名）。同一层里两套命名原则，
///    等于把"产品名"从后门放了回来。
/// 2. **产品名不是决定能力的那个轴**。真正决定"能不能管内容包"的是
///    **数据在哪**——普通文件系统，还是系统沙箱里。
///    **同一个产品可以有多个形态（见 [`Packaging`]），而同一个形态也可能被多个产品共用。**
///
/// **所以命名按"形态 / 数据布局"，不按品牌。** 品牌名只在
/// **Provider 的适配代码**与**面向用户的 `reason` 文案**里出现。
///
/// ## 为什么还要一个 `Universal` 变体
///
/// 它是给**平台级**能力用的（不绑定任何游戏的能力，比如"磁盘清理"），
/// 以及给测试用的替身。**它不该被用来回避实现产品详情。**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InstanceDetail {
    /// 产品无关的实例（平台级能力 / 测试替身）
    Universal,
    /// **文件系统形态**：实例数据是普通文件，**内容与世界都可直接读写**。
    /// （模组化内容产品普遍是这个形态）
    FileSystem,
    /// **商店沙箱形态**：数据位于 MSIX 沙箱内，**内容包与世界管理都不可用**。
    /// （商店分发的产品普遍是这个形态）
    StoreSandbox,
}

/// **包形态**——同一产品内**能力差别极大**，所以它是详情的一部分。
///
/// 方案 §6.2 的能力矩阵按这两列分列（普通文件 vs MSIX 沙箱）。
///
/// **命名按"数据在哪"，不按品牌**（理由见 [`InstanceDetail`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Packaging {
    /// 普通文件系统：**内容与世界都可管理**
    FileSystem,
    /// MSIX 沙箱：**内容包与世界管理都不可用**
    Sandbox,
}

impl InstanceDetail {
    /// 详情里各能力的结论表。
    ///
    /// **`Universal` 返回空表**——它没有任何产品专属能力，
    /// 而"未声明 ≠ 禁用"（见 [`Capabilities::contains_detail`]）。
    pub fn details(&self) -> Capabilities {
        let mut c = Capabilities::new();
        match self {
            Self::Universal => {}
            Self::FileSystem => {
                // 文件系统形态：光影与加载器是这类产品的典型能力
                c.set_detail(DetailKey::Shaders, Capability::enabled());
                c.set_detail(DetailKey::Loaders, Capability::enabled());
                // 而"行为包 / 皮肤包 / 依赖组件"不属于这类——
                // **如实声明不可用并给原因**，而不是省略
                c.set_detail(
                    DetailKey::BehaviorPacks,
                    Capability::disabled("该类产品的内容以模组形式存在，没有行为包").unwrap(),
                );
                c.set_detail(
                    DetailKey::SkinPacks,
                    Capability::disabled("该类产品没有皮肤包").unwrap(),
                );
                c.set_detail(
                    DetailKey::ProductDependencies,
                    Capability::disabled("该类产品没有此概念").unwrap(),
                );
            }
            Self::StoreSandbox => {
                // 商店沙箱形态：行为包 / 皮肤包 / 依赖组件是它的典型能力
                c.set_detail(DetailKey::BehaviorPacks, Capability::enabled());
                c.set_detail(DetailKey::SkinPacks, Capability::enabled());
                c.set_detail(DetailKey::ProductDependencies, Capability::enabled());
                c.set_detail(
                    DetailKey::Shaders,
                    Capability::disabled("该类产品不支持光影").unwrap(),
                );
                c.set_detail(
                    DetailKey::Loaders,
                    Capability::disabled("该类产品没有加载器").unwrap(),
                );
            }
        }
        c
    }

    /// 详情 + **包形态**：形态能把同一类产品内部再分出一档能力。
    ///
    /// **这就是"同一产品内也能不同"的表达方式**（方案 §6.2 的 GDK / UWP 两列）——
    /// 它**不需要**为形态单开一个"产品"，只要形态是详情的一部分。
    pub fn with_packaging(base: &Self, packaging: Packaging) -> Capabilities {
        let mut c = base.details();
        if *base == Self::StoreSandbox && packaging == Packaging::Sandbox {
            // 沙箱形态：连内容包也不能管了
            for k in [DetailKey::BehaviorPacks, DetailKey::SkinPacks] {
                c.set_detail(
                    k,
                    Capability::disabled("位于系统沙箱内，无法管理内容包").unwrap(),
                );
            }
        }
        c
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

/// 一组能力结论。
///
/// **两张表分开存**（这是修正后的结构）：
/// - `universal`：**任何产品都成立**的通用能力（[`CapabilityKey`]）
/// - `detail`：**产品专属**的能力（[`DetailKey`]，来自 [`InstanceDetail::details`]）
///
/// **为什么分开而不合成一张**：混在一张表里，界面就分不清"哪些项要按产品筛选、
/// 哪些是通用的"——**而这正是修正前那版结构别扭的根源**。
/// 分开之后，"通用项 vs 产品专属项"由**类型**区分，不靠约定。
///
/// 内部用 `BTreeMap` 而非 `HashMap`：**输出稳定**（同样的输入给同样的
/// JSON 字节序），这让我们能对前后端契约做快照测试。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
/// 一份**可展示的能力概览**（`Capabilities::overview` 的返回值）。
///
/// 刻意做成**已排序的普通数据**，而不是让界面自己去遍历两张表：
/// 界面的职责是画出来，而"哪些可用、为什么不可用"是内核的判断（见 `overview` 的注释）。
pub struct Overview {
    /// 启用的能力条数
    pub enabled_count: usize,
    /// 禁用的能力条数
    pub disabled_count: usize,
    /// 启用的能力（稳定字符串，已排序）
    pub enabled: Vec<String>,
    /// 禁用的能力及其原因（已按 key 排序）
    pub disabled: Vec<DisabledItem>,
}

/// 一项被禁用的能力，**必须带原因**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisabledItem {
    /// 能力或详情的稳定字符串
    pub key: String,
    /// 为什么不可用（内核保证非空；`validate()` 会检查）
    pub reason: String,
}

/// **两张表分开存**：通用能力与产品详情。
///
/// **为什么分开而不合成一张**：混在一张表里，界面就分不清"哪些项要按产品筛选、
/// 哪些是通用的"——**而这正是修正前那版结构别扭的根源**。
/// 分开之后，"通用项 vs 产品专属项"由**类型**区分，不靠约定。
///
/// 内部用 `BTreeMap` 而非 `HashMap`：**输出稳定**（同样的输入给同样的
/// JSON 字节序），这让我们能对前后端契约做快照测试。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    universal: BTreeMap<CapabilityKey, Capability>,
    detail: BTreeMap<DetailKey, Capability>,
}

impl Capabilities {
    pub fn new() -> Self {
        Self {
            universal: BTreeMap::new(),
            detail: BTreeMap::new(),
        }
    }

    // ── 通用能力（任何产品都成立）──────────────────────────────

    /// 设定一个**通用**能力；禁用态的原因由 [`Capability`] 保证非空。
    pub fn set(&mut self, key: CapabilityKey, cap: Capability) -> &mut Self {
        self.universal.insert(key, cap);
        self
    }

    pub fn get(&self, key: CapabilityKey) -> Option<&Capability> {
        self.universal.get(&key)
    }

    /// 该层是否声明了这个**通用**能力。
    ///
    /// **未声明 ≠ 禁用**：未声明表示"这一层没意见"（由上层决定），
    /// 禁用表示"这一层明确说不行，并给了原因"。
    pub fn contains(&self, key: CapabilityKey) -> bool {
        self.universal.contains_key(&key)
    }

    /// **通用**能力的遍历器（界面渲染第一段用这个）。
    pub fn iter(&self) -> impl Iterator<Item = (CapabilityKey, &Capability)> {
        self.universal.iter().map(|(k, v)| (*k, v))
    }

    // ── 产品专属详情（由 [`InstanceDetail`] 提供）──────────────

    /// 设定一个**产品专属**能力。
    pub fn set_detail(&mut self, key: DetailKey, cap: Capability) -> &mut Self {
        self.detail.insert(key, cap);
        self
    }

    pub fn get_detail(&self, key: DetailKey) -> Option<&Capability> {
        self.detail.get(&key)
    }

    pub fn contains_detail(&self, key: DetailKey) -> bool {
        self.detail.contains_key(&key)
    }

    /// **产品专属**能力的遍历器（界面渲染第二段用这个）。
    ///
    /// **它为什么与 `iter()` 分开**：两者混在一张表里，
    /// 界面就分不清"哪些项要按产品筛选、哪些是通用的"——
    /// 而这正是修正前那版结构别扭的根源。
    pub fn iter_detail(&self) -> impl Iterator<Item = (DetailKey, &Capability)> {
        self.detail.iter().map(|(k, v)| (*k, v))
    }

    /// 把一份**详情**合并进来（`InstanceDetail::details()` 的产物）。
    pub fn with_detail(&self, detail: &Capabilities) -> Capabilities {
        let mut out = self.clone();
        for (k, v) in detail.iter_detail() {
            out.detail.insert(k, v.clone());
        }
        out
    }

    // ── 通用 ──────────────────────────────────────────────────

    pub fn len(&self) -> usize {
        self.universal.len() + self.detail.len()
    }

    pub fn is_empty(&self) -> bool {
        self.universal.is_empty() && self.detail.is_empty()
    }

    /// 叠加：`overrides` 里声明了的 key 覆盖当前值，其余保留。
    ///
    /// 这是实现"产品级给默认、实例级覆盖"的那一步——
    /// **子层只声明差异，不重复整张表**。
    pub fn merged_with(&self, overrides: &Capabilities) -> Capabilities {
        let mut out = self.clone();
        for (k, v) in overrides.iter() {
            out.universal.insert(k, v.clone());
        }
        for (k, v) in overrides.iter_detail() {
            out.detail.insert(k, v.clone());
        }
        out
    }

    /// 把两张表压成**一份可展示的概览**（启用的有哪些、禁用的各因为什么）。
    ///
    /// **为什么这个函数在内核里而不是在界面里**：它是**规则**，不是排版。
    /// "哪些能力可用、不可用的原因是什么"这个判断与窗口、与渲染无关，
    /// 所以它属于 `qul-core`；界面只负责把返回的结构画出来。
    ///
    /// **也是 M0 · S4 那条跨层链路的"计算结果"**：
    /// 界面按钮 → Tauri 命令 → `qul-app` → 这里 → 返回给前端。
    /// 选它当链路样板，是因为它**纯计算、无副作用**——
    /// 链路测试要验的是"调用路径通不通"，不是"副作用对不对"。
    ///
    /// 输出**已排序**：`BTreeMap` 的迭代顺序本就稳定，但这里再显式按
    /// 稳定字符串排序，让概览与"键的声明顺序"无关（否则重排枚举会改动输出）。
    pub fn overview(&self) -> Overview {
        let mut enabled: Vec<String> = Vec::new();
        let mut disabled: Vec<DisabledItem> = Vec::new();

        for (k, v) in self.iter() {
            if v.is_enabled() {
                enabled.push(k.as_str().to_string());
            } else {
                disabled.push(DisabledItem {
                    key: k.as_str().to_string(),
                    reason: v.reason().unwrap_or("").to_string(),
                });
            }
        }
        for (k, v) in self.iter_detail() {
            if v.is_enabled() {
                enabled.push(k.as_str().to_string());
            } else {
                disabled.push(DisabledItem {
                    key: k.as_str().to_string(),
                    reason: v.reason().unwrap_or("").to_string(),
                });
            }
        }

        enabled.sort();
        disabled.sort_by(|a, b| a.key.cmp(&b.key));

        Overview {
            enabled_count: enabled.len(),
            disabled_count: disabled.len(),
            enabled,
            disabled,
        }
    }

    /// 一致性自检：**任何禁用态必须有非空原因**（两张表都查）。
    ///
    /// 正常路径下这个检查永远不会失败（类型系统已守住）。它存在是为了
    /// 覆盖**反序列化与其他语言的输入**这两条绕行路径——
    /// 前者已由 [`CapabilityWire`] 拦截，本方法用于测试与运行时断言。
    pub fn validate(&self) -> Result<(), ReasonError> {
        for v in self.universal.values().chain(self.detail.values()) {
            // `is_none_or` 需要 Rust 1.82+；工作区 MSRV 已是 1.89（见根 `Cargo.toml`）。
            if !v.is_enabled() && v.reason().is_none_or(|r| r.trim().is_empty()) {
                return Err(ReasonError);
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
        caps.set_detail(
            DetailKey::Shaders,
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
        caps.set_detail(
            DetailKey::Shaders,
            Capability::disabled("基岩版不支持光影").unwrap(),
        );
        assert!(caps.contains_detail(DetailKey::Shaders));
        assert!(!caps.contains(CapabilityKey::Mods), "未声明 ≠ 禁用");
    }

    /// **这一组测试守的是"能力驱动界面"那件事本身。**
    ///
    /// 方案 §3.3 的原话是：切到基岩版实例时，「光影」自动消失、「行为包」自动出现。
    /// 下面几个测试就是**把这句话写成断言**。
    #[test]
    fn 通用能力表里不许出现产品专属概念() {
        // **这是结构修正的核心断言。**
        // 产品专属项（光影 / 行为包 / 皮肤包 / 加载器 / 依赖组件）
        // **必须**只出现在 `DetailKey` 里，不许回到 `CapabilityKey`。
        //
        // 为什么：混进通用表之后，界面就分不清"哪些项要按产品筛选"，
        // 而架构测试的禁用词表也会把"产品名"与"内容类型"混在一起
        // —— 这个坑在本次修正前真实踩过两次。
        let universal: Vec<&str> = CapabilityKey::ALL.iter().map(|k| k.as_str()).collect();
        for product_only in ["shaders", "behavior_packs", "skin_packs", "loaders"] {
            assert!(
                !universal.contains(&product_only),
                "`{}` 是产品专属项，不该出现在通用能力表里：{:?}",
                product_only,
                universal
            );
        }
        // 而它必须出现在详情表里
        let details: Vec<&str> = InstanceDetail::ALL_KEYS
            .iter()
            .map(|k| k.as_str())
            .collect();
        for product_only in [
            "shaders",
            "behavior_packs",
            "skin_packs",
            "loaders",
            "product_dependencies",
        ] {
            assert!(
                details.contains(&product_only),
                "`{}` 应在详情表里：{:?}",
                product_only,
                details
            );
        }
    }

    #[test]
    fn 实例详情恰好一种且各自给出差异() {
        let render_detail = |c: &Capabilities| -> Vec<&'static str> {
            InstanceDetail::ALL_KEYS
                .iter()
                .filter(|k| c.get_detail(**k).is_some_and(|v| v.is_enabled()))
                .map(|k| k.as_str())
                .collect()
        };

        // 文件系统形态：有光影与加载器，没有行为包 / 皮肤包 / 依赖组件
        let filesystem = InstanceDetail::FileSystem.details();
        let j = render_detail(&filesystem);
        assert!(j.contains(&"shaders"), "文件系统形态应有光影：{:?}", j);
        assert!(j.contains(&"loaders"), "文件系统形态应有加载器：{:?}", j);
        assert!(
            !j.contains(&"behavior_packs"),
            "文件系统形态不该有行为包：{:?}",
            j
        );

        // 商店沙箱形态（普通文件部分）：有行为包 / 皮肤包 / 依赖组件，没有光影与加载器
        let store = InstanceDetail::StoreSandbox.details();
        let s = render_detail(&store);
        assert!(s.contains(&"behavior_packs"), "该类产品应有行为包：{:?}", s);
        assert!(s.contains(&"skin_packs"), "该类产品应有皮肤包：{:?}", s);
        assert!(s.contains(&"product_dependencies"), "该类产品应有依赖组件");
        assert!(!s.contains(&"shaders"), "该类产品不该有光影：{:?}", s);
        assert!(!s.contains(&"loaders"), "该类产品不该有加载器：{:?}", s);

        // **同一个详情 + 沙箱形态**：连内容包也不能管了（方案 §6.2 的能力矩阵）
        let sandboxed =
            InstanceDetail::with_packaging(&InstanceDetail::StoreSandbox, Packaging::Sandbox);
        let sd = render_detail(&sandboxed);
        assert!(
            !sd.contains(&"behavior_packs"),
            "沙箱内不该能管内容包：{:?}",
            sd
        );
        assert!(
            !sd.contains(&"skin_packs"),
            "沙箱内不该能管皮肤包：{:?}",
            sd
        );
        // 但"依赖组件"在沙箱形态下仍然可用（运行时组件一样要装）
        assert!(
            sd.contains(&"product_dependencies"),
            "沙箱形态仍需要依赖组件：{:?}",
            sd
        );

        // **这条断言是本节的重点**：
        // "同一详情内、形态不同"造成的差异，与"详情本身不同"造成的差异，
        // **由同一个机制表达**——不需要为形态另开一个"产品"。
        assert_ne!(s, sd, "普通形态与沙箱形态的详情结论必须不同");
    }

    #[test]
    fn 界面渲染第一段与第二段互不干扰() {
        // 通用表 + 某产品详情 = 该实例的完整左栏
        let mut universal = Capabilities::new();
        for k in [
            CapabilityKey::Launch,
            CapabilityKey::ResourcePacks,
            CapabilityKey::Worlds,
            CapabilityKey::Configs,
            CapabilityKey::Screenshots,
        ] {
            universal.set(k, Capability::enabled());
        }

        let filesystem = universal.with_detail(&InstanceDetail::FileSystem.details());
        let sandbox = universal.with_detail(&InstanceDetail::StoreSandbox.details());

        let render = |c: &Capabilities| -> Vec<&'static str> {
            let mut v: Vec<&'static str> = c
                .iter()
                .filter(|(_, cap)| cap.is_enabled())
                .map(|(k, _)| k.as_str())
                .collect();
            v.extend(
                c.iter_detail()
                    .filter(|(_, cap)| cap.is_enabled())
                    .map(|(k, _)| k.as_str()),
            );
            v
        };

        let j = render(&filesystem);
        let b = render(&sandbox);

        // 通用项两边都在（**界面不需要为它们写任何产品判断**）
        for common in ["launch", "resource_packs", "worlds"] {
            assert!(j.contains(&common), "文件系统形态应含通用项 {}", common);
            assert!(b.contains(&common), "沙箱形态应含通用项 {}", common);
        }
        // 详情项各自不同
        assert!(j.contains(&"shaders") && !j.contains(&"behavior_packs"));
        assert!(b.contains(&"behavior_packs") && !b.contains(&"shaders"));

        // 两张表不互相污染（**这是"分开存"换来的保证**）
        assert!(
            filesystem.get_detail(DetailKey::Shaders).is_some(),
            "详情项应在详情表里"
        );
        assert!(
            filesystem.get(CapabilityKey::Launch).is_some(),
            "通用项应在通用表里"
        );
    }
}
