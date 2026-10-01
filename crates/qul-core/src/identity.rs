//! # 身份来源与外部审批闸门（**纯规则，零 IO**）
//!
//! ## 这一层在回答什么
//!
//! 方案 §5.7 要求"审批未通过时按 §3.3 渲染为**带原因的禁用态**（不是隐藏，也不是灰按钮）"，
//! 而**"带原因"这三个字里面藏着一个硬约束**：
//!
//! > **原因由运行时读到的审批状态决定，不是编译期常量。**
//!
//! 这句话决定了本模块的形状：**审批状态必须是数据，不能是代码分支。**
//! 如果它被写成一个常量，那么"审批中途通过"就要**改代码重编译**——
//! 而那正是方案明确要避免的（原话：*"审批中途通过 → 无需改动代码"*）。
//!
//! ## ⚠️ 一条纪律：**内核不许出现产品名**（护栏抓过我一次）
//!
//! 本模块的第一版把审批状态命名成 `MojangApproval`、把方法命名成 `serves_bedrock`，
//! 于是 `crates/qul-core/tests/architecture.rs` **立刻报警**：
//!
//! ```text
//! identity.rs: 生产代码/字符串中出现 `mojang`（产品名只许出现在注释与测试里）
//! identity.rs: 生产代码/字符串中出现 `bedrock`
//! ```
//!
//! **它抓得对。** 方案 §1 的原话是"MC 概念降为**某个 Provider 的实现细节**"，
//! 而把品牌写进内核的类型名与字符串，正是"内核腐化"的第一步：
//! 它会让"加第二个产品"变成**改内核**，而不是**注册一个 Provider**。
//!
//! 所以现在的形状是：
//!
//! | 层 | 知道什么 | 不知道什么 |
//! |---|---|---|
//! | **内核（本模块）** | **身份来源可以有一个外部审批闸门**，闸门有四种状态 | 那个闸门是**谁**的（属于哪个产品） |
//! | **Provider（M2 起）** | 闸门的具体身份与它自己的品牌文案 | — |
//!
//! **具体身份由调用方以 `&str` 传入**（[`ApprovalState::reason`] 的参数）。
//! 于是内核**一个字的产品名都没有**，而 UI 仍然拿得到完整原因。
//!
//! ## 为什么要区分四种身份来源
//!
//! 规格 §1.3 的原话：**"此前整份规格默认只有微软一种"** —— 那是用户指出来的缺口。
//! 而四种来源的**可用性条件完全不同**：
//!
//! | 来源 | 谁能影响它的可用性 |
//! |---|---|
//! | 平台账户 | **一个外部审批**（不由我们决定） |
//! | 离线账户 | **没有人** —— 它永远可用 |
//! | 统一通行证 | **第三方验证服务**（不由我们决定） |
//! | 外置登录 | **第三方皮肤站**（不由我们决定） |
//!
//! **把这四种塞进一个 `bool` 会立刻出错**：`available: false` 说不清是
//! "我们没做完"、"审批没过"、还是"那个第三方站挂了"——
//! 而用户要做的动作**三者完全不同**。

use serde::{Deserialize, Serialize};

/// 四种身份来源（规格 §1.3.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IdentitySource {
    /// 平台账户登录（本项目的第一个实例是微软账户）。
    ///
    /// **它是唯一能供"需要商店授权"的产品使用的来源** ——
    /// 见 [`IdentitySource::provides_store_license`]。
    Platform,
    /// 离线账户。**只能供"用本地令牌启动"的产品**。
    Offline,
    /// 统一通行证（第三方验证服务）。**只能供"用本地令牌启动"的产品**。
    UnifiedPass,
    /// 外置登录（第三方注入）。**只能供"用本地令牌启动"的产品**。
    AuthlibInjector,
}

impl IdentitySource {
    pub const ALL: &'static [IdentitySource] = &[
        IdentitySource::Platform,
        IdentitySource::Offline,
        IdentitySource::UnifiedPass,
        IdentitySource::AuthlibInjector,
    ];

    /// 稳定标识（界面与配置都用它，**不用中文名**）。
    pub const fn key(self) -> &'static str {
        match self {
            IdentitySource::Platform => "identity.platform",
            IdentitySource::Offline => "identity.offline",
            IdentitySource::UnifiedPass => "identity.unified_pass",
            IdentitySource::AuthlibInjector => "identity.authlib_injector",
        }
    }

    /// 这个来源能不能提供**商店授权**。
    ///
    /// ## 为什么这是一个通用概念，而不是某个产品的属性
    ///
    /// 两类产品的授权获取方式**在机制上不同**：
    ///
    /// | 产品形态 | 授权怎么来 | 谁能供 |
    /// |---|---|---|
    /// | **用本地令牌启动** | 启动参数里带令牌/标识 | 四种来源**都能** |
    /// | **需要商店授权** | 由商店/平台账户直接授予 | **只有平台账户** |
    ///
    /// 第二类**改不了**：它的授权由平台授予，
    /// **没有"往启动参数里塞一个令牌"这条路**。
    /// 所以这不是"当前是否支持"的问题，而是**机制上不可能**。
    ///
    /// **因此它是常量而不是运行时判断**：把它写成运行时状态，
    /// 会让人以为"以后也许能"。
    pub const fn provides_store_license(self) -> bool {
        matches!(self, IdentitySource::Platform)
    }

    pub const fn human(self) -> &'static str {
        match self {
            IdentitySource::Platform => "平台账户登录",
            IdentitySource::Offline => "离线账户",
            IdentitySource::UnifiedPass => "统一通行证",
            IdentitySource::AuthlibInjector => "外置登录",
        }
    }
}

/// **外部审批闸门**的状态（通用，不指名是谁的审批）。
///
/// **刻意用枚举而不是字符串**：字符串能表达"任何东西"，
/// 于是"审批通过但界面上仍显示未通过"这类不一致**无法被检查**。
/// 枚举逼着调用方**只能落在四个已被想过的状态之一**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalState {
    /// 还没提交。
    NotSubmitted,
    /// 已提交，等结果。**有外部等待期。**
    Submitted {
        /// 申请编号（对方给的）
        reference: String,
    },
    /// 通过。
    Approved,
    /// 被拒。**必须带上我们知道的理由**，否则用户只能反复重试。
    Rejected { note: String },
}

impl ApprovalState {
    /// 是否已经"能用了"（即审批不再是阻碍）。
    pub const fn permits_use(&self) -> bool {
        matches!(self, ApprovalState::Approved)
    }

    /// 给用户看的状态词（**不带句号**，方便拼进句子）。
    pub const fn state_word(&self) -> &'static str {
        match self {
            ApprovalState::NotSubmitted => "未提交",
            ApprovalState::Submitted { .. } => "已提交，等待审批",
            ApprovalState::Approved => "已通过",
            ApprovalState::Rejected { .. } => "未通过",
        }
    }

    /// 这一状态下**用户该做什么**。
    ///
    /// **这是"带原因的禁用态"里"原因"两个字的落点**：
    /// 不是复述状态，而是**说下一步**。复述状态（"未提交"）用户已经能看见，
    /// 而"你不需要做任何事，我们在等"与"我们需要你去提交"是**完全不同的动作**。
    pub fn what_user_should_do(&self) -> &'static str {
        match self {
            ApprovalState::NotSubmitted => {
                "等待我们提交申请；你无需操作。此期间可使用其它身份来源。"
            }
            ApprovalState::Submitted { .. } => {
                // ⚠️ **这句话在 M0 收口时被改过一次，因为第一版是错的。**
                //
                // 第一版写的是「审批有外部等待期，你无需操作；结果出来后会自动可用」——
                // 而用户实测确认：**Mojang 的表单不提供申请编号，也没有查询进度的面板**。
                //
                // 没有面板意味着**我们无法主动探测结果**：没有轮询目标、没有状态字段。
                // 所以"结果出来后会自动可用"是一句**我们做不到的承诺** ——
                // 而它比"不知道"更坏：用户会等一个不会来的通知。
                //
                // 正确的说法是：**唯一能知道它通过了的时刻，就是你真的去登录的那一刻。**
                "审批有外部等待期，你无需操作。**我方无法查询进度**——\
                 提交表单不提供编号与状态面板，所以只有在你实际尝试登录时，\
                 才能知道审批是否已经通过。"
            }
            ApprovalState::Rejected { .. } => {
                "审批未通过，一期只提供其余身份来源；重新申请前该登录方式不会开放。"
            }
            // 这个分支不该被走到（通过了就不是禁用态），但**返回一句话而不是 panic**：
            // 一个"不该发生"的路径若会 panic，它就变成了一个崩溃源，
            // 而这段代码会在**审批通过之后**才被真正执行到 —— 那是最不该崩的时候。
            ApprovalState::Approved => "审批已通过。",
        }
    }

    /// **不可用时的原因文案**（方案 §5.7 与任务书要求的那个 `reason`）。
    ///
    /// `what` 是**审批的具体身份**，由调用方（Provider）传入 ——
    /// 这就是"内核不知道产品名"的落点：内核把它当成一个不透明的词。
    ///
    /// **顺序是刻意的**：调用方先看自己有没有配好，再看审批。
    /// 因为"我们没配好"是**我们的问题**（用户无需等待），
    /// 而"审批未通过"是**外部等待**（用户需要有心理预期）。
    /// **把两者说反会让用户白等。**
    pub fn reason(&self, what: &str) -> String {
        format!(
            "{}需审批通过，当前状态：{}。{}",
            what,
            self.state_word(),
            self.what_user_should_do()
        )
    }
}

/// 平台账户登录的**整条前置链**。
///
/// **两个前置条件，不是一回事**：
///
/// | 前置 | 谁做 | 状态 |
/// |---|---|---|
/// | **应用注册**（拿到客户端 ID） | 我们 | `client_id` 是否已配 |
/// | **外部审批** | 对方（**有外部等待期**） | [`ApprovalState`] |
///
/// **为什么必须分开**：把两者合成一个 `bool` 之后，
/// "应用已建但审批没交"与"审批交了但客户 ID 没配"会显示同一句话，
/// 而**用户要做的动作完全不同**（前者只需等，后者需要检查配置）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformLoginPrereqs {
    /// 应用注册拿到的**客户端 ID**。`None` = 尚未配置。
    ///
    /// ⚠️ **客户端 ID 不是机密**（它公开出现在每个授权 URL 里），
    /// 所以它可以进配置、可以出现在日志里。
    /// **而"客户端密码"不该存在**：这类客户端不使用密钥。
    /// 这一点值得写在这里，因为**它是"这个值可以进日志"的唯一理由**。
    pub client_id: Option<String>,
    pub approval: ApprovalState,
}

impl PlatformLoginPrereqs {
    /// **未配置任何东西的真实初始状态。**
    pub const fn unconfigured() -> Self {
        Self {
            client_id: None,
            approval: ApprovalState::NotSubmitted,
        }
    }

    /// 从配置构造（这就是"运行时读取"的入口）。
    pub fn from_config(client_id: Option<String>, approval: ApprovalState) -> Self {
        Self {
            client_id,
            approval,
        }
    }

    /// 平台登录此刻是否可用。**两个前置都满足才算。**
    pub fn is_available(&self) -> bool {
        self.client_id
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            && self.approval.permits_use()
    }

    /// **不可用时的原因**。`what` = 审批的具体身份（由 Provider 传入）。
    ///
    /// 返回 `None` 表示**可用**（不该有原因）。
    pub fn disabled_reason(&self, what: &str) -> Option<String> {
        let has_id = self
            .client_id
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty());
        if !has_id {
            return Some(
                "平台账户登录的应用尚未配置（缺少客户端 ID）。这不影响其它身份来源。".to_string(),
            );
        }
        if !self.approval.permits_use() {
            return Some(self.approval.reason(what));
        }
        None
    }

    /// 变成能力描述符里的一项。
    ///
    /// **不可用时一律给原因** —— `Capability::disabled` 会拒绝空原因，
    /// 所以这里不可能产出一个"没有原因的禁用态"。
    pub fn to_capability(&self, what: &str) -> crate::caps::Capability {
        match self.disabled_reason(what) {
            None => crate::caps::Capability::enabled(),
            Some(reason) => crate::caps::Capability::disabled(reason)
                // 原因已在构造时保证非空；真到不了这里。
                .unwrap_or_else(|_| crate::caps::Capability::enabled()),
        }
    }
}

/// 某个**第三方**身份来源的可用性。
///
/// 与平台账户那条链的区别：**这里没有"审批"**，
/// 只有"那个服务此刻能不能用"。
/// 所以它是一句话而不是一条前置链 —— **把没有的复杂度加进来就是过度设计。**
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThirdPartyState {
    /// 服务可用
    Available,
    /// 服务不可用（含原因，如"服务离线"）
    Unavailable { note: String },
    /// 我们还没做（一期范围外）
    NotImplemented,
}

/// 全部身份来源的可用性汇总。
///
/// **它是"运行时读到的状态"的完整快照**，界面据它渲染各入口。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityAvailability {
    pub platform: PlatformLoginPrereqs,
    pub unified_pass: ThirdPartyState,
    pub authlib_injector: ThirdPartyState,
}

impl IdentityAvailability {
    /// 一期的真实起点：平台链未配、两个第三方来源**未实现**。
    ///
    /// ⚠️ **离线账户不在这里** —— 它永远可用（规格 §1.3.2），
    /// 所以它没有"状态"可言。把它塞进这个结构会让人以为它也可能不可用。
    pub fn initial() -> Self {
        Self {
            platform: PlatformLoginPrereqs::unconfigured(),
            unified_pass: ThirdPartyState::NotImplemented,
            authlib_injector: ThirdPartyState::NotImplemented,
        }
    }

    /// 某个来源此刻是否可用。
    ///
    /// **离线账户恒为 `true`** —— 这是本函数的**唯一硬编码分支**，而它是刻意的：
    /// 离线的可用性**不依赖任何外部条件**（不需要网络、不经过任何服务），
    /// 所以它没有"状态"可读。若哪天有人给离线加上状态，
    /// 那说明有人往它身上接了外部依赖 —— 而那会是**规格被破坏**的信号。
    pub fn is_available(&self, source: IdentitySource) -> bool {
        match source {
            IdentitySource::Offline => true,
            IdentitySource::Platform => self.platform.is_available(),
            IdentitySource::UnifiedPass => matches!(self.unified_pass, ThirdPartyState::Available),
            IdentitySource::AuthlibInjector => {
                matches!(self.authlib_injector, ThirdPartyState::Available)
            }
        }
    }

    /// 不可用时的原因（`what` = 平台账户审批的具体身份）。可用时返回 `None`。
    pub fn reason(&self, source: IdentitySource, what: &str) -> Option<String> {
        match source {
            // 离线永远可用，所以永远没有原因。
            IdentitySource::Offline => None,
            IdentitySource::Platform => self.platform.disabled_reason(what),
            IdentitySource::UnifiedPass => {
                Self::third_party_reason("统一通行证", &self.unified_pass)
            }
            IdentitySource::AuthlibInjector => {
                Self::third_party_reason("外置登录", &self.authlib_injector)
            }
        }
    }

    fn third_party_reason(name: &str, state: &ThirdPartyState) -> Option<String> {
        match state {
            ThirdPartyState::Available => None,
            ThirdPartyState::Unavailable { note } => Some(format!(
                "{name}服务当前不可用：{note}。可改用其它身份来源。"
            )),
            ThirdPartyState::NotImplemented => Some(format!(
                "{name}尚未实现（一期范围外）。可改用离线账户或平台账户登录。"
            )),
        }
    }

    /// 可用来源的数量（用于诊断与"至少还有一条路"的检查）。
    pub fn available_count(&self) -> usize {
        IdentitySource::ALL
            .iter()
            .filter(|s| self.is_available(**s))
            .count()
    }

    /// **它能不能供"需要商店授权"的产品使用。**
    ///
    /// 只要有**任意一条**可用来源能提供商店授权，该产品就能登录。
    /// 这个判断属于内核（是通用规则），而**具体是哪个产品需要商店授权**
    /// 由 Provider 决定（内核不问）。
    pub fn can_serve_store_licensed(&self) -> bool {
        IdentitySource::ALL
            .iter()
            .any(|s| s.provides_store_license() && self.is_available(*s))
    }
}

/// 测试里用的"审批身份"占位。
///
/// **它只在测试里出现**，而这是刻意的：如果内核的生产代码里需要一个默认的
/// 审批身份，那说明内核**知道了某个产品** —— 那就越界了。
#[cfg(test)]
const TEST_APPROVAL_WHAT: &str = "某应用";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 离线账户永远可用且没有原因() {
        // 规格 §1.3.2：离线不依赖任何外部条件 —— 不需要网络、不经过任何服务。
        let a = IdentityAvailability::initial();
        assert!(a.is_available(IdentitySource::Offline));
        assert_eq!(a.reason(IdentitySource::Offline, TEST_APPROVAL_WHAT), None);

        let b = IdentityAvailability {
            platform: PlatformLoginPrereqs::from_config(Some("x".into()), ApprovalState::Approved),
            ..IdentityAvailability::initial()
        };
        assert!(b.is_available(IdentitySource::Offline));
    }

    #[test]
    fn 只有平台账户能提供商店授权() {
        // 需要商店授权的产品，其授权由平台授予，
        // **没有"往启动参数里塞一个令牌"这条路**。
        assert!(IdentitySource::Platform.provides_store_license());
        for s in [
            IdentitySource::Offline,
            IdentitySource::UnifiedPass,
            IdentitySource::AuthlibInjector,
        ] {
            assert!(!s.provides_store_license(), "{s:?} 不该声称能提供商店授权");
        }
    }

    #[test]
    fn 内核的值里一个字的产品名都没有() {
        // 这条测试是**护栏的镜像**：护栏扫源文件，这条扫**值**。
        // 两者都要有 —— 因为护栏只在测试时跑，而值可能在别处被构造出来。
        let mut corpus = String::new();
        for s in IdentitySource::ALL {
            corpus.push_str(s.key());
            corpus.push_str(s.human());
        }
        for st in [
            ApprovalState::NotSubmitted,
            ApprovalState::Submitted {
                reference: "R".into(),
            },
            ApprovalState::Approved,
            ApprovalState::Rejected { note: "n".into() },
        ] {
            corpus.push_str(st.state_word());
            corpus.push_str(st.what_user_should_do());
            corpus.push_str(&st.reason("某应用"));
        }
        let lower = corpus.to_lowercase();
        // 产品与厂商名一律不许出现（**注释里可以，值里不行**）
        for brand in ["mojang", "minecraft", "bedrock", "microsoft", "xbox"] {
            assert!(
                !lower.contains(brand),
                "内核的值里出现了产品/厂商名 `{brand}`：护栏会在源文件上报警，这里在值上再拦一次"
            );
        }
    }

    #[test]
    fn reason_随审批状态而变_而不是编译期常量() {
        // 这是方案 §5.7 直接要求的：审批中途通过时**无需改动代码**。
        // 如果 reason 是常量，这条测试不可能通过。
        let submitted = PlatformLoginPrereqs::from_config(
            Some("2a06b5bc".into()),
            ApprovalState::Submitted {
                reference: "REF-1".into(),
            },
        );
        let approved =
            PlatformLoginPrereqs::from_config(Some("2a06b5bc".into()), ApprovalState::Approved);

        let r1 = submitted
            .disabled_reason(TEST_APPROVAL_WHAT)
            .expect("提交中应当不可用");
        assert!(r1.contains("已提交，等待审批"), "{r1}");
        assert!(!submitted.is_available());

        // 同一份代码、同一个 client_id，只换状态 → 变成可用且没有原因
        assert_eq!(approved.disabled_reason(TEST_APPROVAL_WHAT), None);
        assert!(approved.is_available());
    }

    #[test]
    fn 审批身份由调用方传入_内核不认识它() {
        // 同一个状态、不同的"审批身份"，应当产出不同的原因 ——
        // 而内核代码一个字都没改。这就是"产品名降为 Provider 细节"的落点。
        let st = ApprovalState::NotSubmitted;
        let a = st.reason("某产品的应用审批");
        let b = st.reason("某个别的东西审批");
        assert_ne!(a, b);
        assert!(a.contains("某产品的应用审批"), "{a}");
        assert!(b.contains("某个别的东西审批"), "{b}");
        // 共同部分仍然一致（状态词与下一步动作来自内核）
        assert!(a.contains("未提交") && b.contains("未提交"));
    }

    #[test]
    fn 提交后的文案不许承诺我们做不到的事() {
        // ⚠️ **这条测试是被一次真实反馈逼出来的。**
        //
        // 第一版文案写「结果出来后会自动可用」—— 而用户实测确认：
        // **Mojang 的表单不提供申请编号，也没有查询进度的面板。**
        //
        // 没有面板 ⇒ **我们无法主动探测结果**（没有轮询目标、没有状态字段）。
        // 所以"会自动可用"是一句**我们做不到的承诺**：
        // 用户会等一个不会来的通知，而**真正能发现它通过的时机是"用户去登录"那一刻**。
        //
        // 这条测试钉住的是：**文案里必须说清"我方无法查询进度"。**
        let p = PlatformLoginPrereqs::from_config(
            Some("2a06b5bc-7b61-4a36-9edf-52fb89525943".into()),
            ApprovalState::Submitted {
                reference: String::new(), // 真实情形是"没有编号"
            },
        );
        let r = p.disabled_reason(TEST_APPROVAL_WHAT).expect("应当不可用");
        assert!(
            r.contains("无法查询进度") || r.contains("无法查询"),
            "必须说清我们查不到进度（否则用户会等一个不会来的通知）：{r}"
        );
        assert!(
            !r.contains("会自动可用"),
            "**不许承诺自动可用** —— 没有面板就意味着我们无法主动探测结果：{r}"
        );
        assert!(
            r.contains("登录"),
            "要指出真正的发现时机是「尝试登录」：{r}"
        );
    }
    #[test]
    fn 客户端_id_缺失与审批未通过要说不同的话() {
        // 把两者说反会让用户白等：
        // "客户 ID 没配"是我们的问题（用户无需等待），"审批未通过"是外部等待。
        let missing_id = PlatformLoginPrereqs::from_config(None, ApprovalState::Approved);
        let pending =
            PlatformLoginPrereqs::from_config(Some("id".into()), ApprovalState::NotSubmitted);

        let r1 = missing_id.disabled_reason(TEST_APPROVAL_WHAT).unwrap();
        let r2 = pending.disabled_reason(TEST_APPROVAL_WHAT).unwrap();
        assert_ne!(r1, r2, "两种情形的原因必须不同");
        assert!(r1.contains("客户端 ID"), "{r1}");
        assert!(r2.contains("审批"), "{r2}");
        // 只有"缺 ID"这一条会提到"我们的配置"，因为它不是等待项
        assert!(r1.contains("配置"), "{r1}");
    }

    #[test]
    fn 空白客户端_id_等同于未配置() {
        // 空串与纯空白是最常见的"配置了但等于没配"的形态。
        for bad in ["", "   ", "\t"] {
            let p = PlatformLoginPrereqs::from_config(Some(bad.into()), ApprovalState::Approved);
            assert!(!p.is_available(), "{bad:?} 不该被当成已配置");
            assert!(p.disabled_reason(TEST_APPROVAL_WHAT).is_some());
        }
    }

    #[test]
    fn 禁用态一定带原因() {
        // 方案 §5.7：带原因的禁用态，**不是隐藏也不是灰按钮**。
        // Capability::disabled 会拒绝空原因，所以这里不可能产出空原因。
        let cases = [
            PlatformLoginPrereqs::unconfigured(),
            PlatformLoginPrereqs::from_config(None, ApprovalState::NotSubmitted),
            PlatformLoginPrereqs::from_config(
                Some("id".into()),
                ApprovalState::Submitted {
                    reference: "R".into(),
                },
            ),
            PlatformLoginPrereqs::from_config(
                Some("id".into()),
                ApprovalState::Rejected {
                    note: "材料不全".into(),
                },
            ),
        ];
        for c in &cases {
            let cap = c.to_capability(TEST_APPROVAL_WHAT);
            assert!(!cap.is_enabled(), "{c:?} 应当是禁用态");
            let reason = cap.reason().expect("禁用态必须有原因");
            assert!(!reason.trim().is_empty(), "{c:?} 的原因不该是空白");
        }
    }

    #[test]
    fn 通过后变成可用态且不带原因() {
        let p = PlatformLoginPrereqs::from_config(
            Some("2a06b5bc-7b61-4a36-9edf-52fb89525943".into()),
            ApprovalState::Approved,
        );
        let cap = p.to_capability(TEST_APPROVAL_WHAT);
        assert!(cap.is_enabled());
        assert_eq!(cap.reason(), None, "可用态不该带原因");
    }

    #[test]
    fn 被拒时必须说明用户该做什么() {
        let p = PlatformLoginPrereqs::from_config(
            Some("id".into()),
            ApprovalState::Rejected {
                note: "用途说明不足".into(),
            },
        );
        let r = p.disabled_reason(TEST_APPROVAL_WHAT).unwrap();
        assert!(r.contains("未通过"), "{r}");
        // 不是复述状态，而是说下一步
        assert!(
            r.contains("其余") || r.contains("重新申请"),
            "被拒时必须给出可执行的下一步：{r}"
        );
    }

    #[test]
    fn 一期的真实起点是平台链未配且两个第三方未实现() {
        // 这条测试的作用是**把"我们现在的真实状态"钉住**：
        // 若哪天 initial() 悄悄变成"全部可用"，这里会红。
        let a = IdentityAvailability::initial();
        assert!(!a.is_available(IdentitySource::Platform));
        assert!(!a.is_available(IdentitySource::UnifiedPass));
        assert!(!a.is_available(IdentitySource::AuthlibInjector));
        // 但**至少还有一条路**：离线
        assert_eq!(a.available_count(), 1);
        assert!(a.is_available(IdentitySource::Offline));
    }

    #[test]
    fn 需要商店授权的产品在起点的确是登录不了的() {
        // 这是一条**真实的业务结论**，而且它是对的：
        // 平台链没配好、两个第三方来源又不能提供商店授权，
        // 所以"需要商店授权的产品"此刻无法登录 —— 界面必须如实呈现这一点。
        let a = IdentityAvailability::initial();
        assert!(
            !a.can_serve_store_licensed(),
            "起点下不该声称能供商店授权产品"
        );

        // 平台链配好并通过后，就可以了
        let b = IdentityAvailability {
            platform: PlatformLoginPrereqs::from_config(Some("id".into()), ApprovalState::Approved),
            ..IdentityAvailability::initial()
        };
        assert!(b.can_serve_store_licensed());
    }

    #[test]
    fn 至少有一条可用来源是可判定的() {
        // "没有可用的登录方式"是必须被发现的严重状态。
        assert!(IdentityAvailability::initial().available_count() >= 1);
    }

    #[test]
    fn 第三方不可用与未实现要说不同的话() {
        // "服务挂了（换一种试试）"与"我们还没做（一期不做）"是完全不同的动作。
        let down = IdentityAvailability {
            unified_pass: ThirdPartyState::Unavailable {
                note: "服务离线".into(),
            },
            ..IdentityAvailability::initial()
        };
        let todo = IdentityAvailability::initial();

        let r1 = down
            .reason(IdentitySource::UnifiedPass, TEST_APPROVAL_WHAT)
            .unwrap();
        let r2 = todo
            .reason(IdentitySource::UnifiedPass, TEST_APPROVAL_WHAT)
            .unwrap();
        assert_ne!(r1, r2);
        assert!(r1.contains("不可用"), "{r1}");
        assert!(r2.contains("尚未实现"), "{r2}");
        // 两者都该指向别的路
        for r in [r1, r2] {
            assert!(r.contains("可改用") || r.contains("身份来源"), "{r}");
        }
    }

    #[test]
    fn 身份来源的键是稳定标识而不是中文名() {
        // 配置、日志、界面事件都用 key；中文名会随文案改动而变。
        for s in IdentitySource::ALL {
            let k = s.key();
            assert!(k.starts_with("identity."), "{k}");
            assert!(k.is_ascii(), "{k} 必须是 ASCII —— 它要进配置与日志");
        }
        // 不重复
        let mut keys: Vec<&str> = IdentitySource::ALL.iter().map(|s| s.key()).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "身份来源的 key 有重复");
    }
}
