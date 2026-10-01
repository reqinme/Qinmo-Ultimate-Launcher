//! # Provider 窄 trait 与注册表（**纯形状，零 IO**）
//!
//! ## 为什么"窄 trait"而不是一个大接口
//!
//! 方案 §3.2 的原话：
//!
//! > 早先设计成单个 `GameProvider` 承载九个成员，**接口定得太大**——
//! > 后果是每加一个能力，所有 Provider 都要实现一遍占位空方法，
//! > 且测试要构造全量假对象。
//!
//! ## 拆分时机：**M1 只定两个**
//!
//! 方案 §3.2 的纪律是 **"每个 trait 在'出现第二个使用场景'时引入，不在纸面上预抽"**，
//! 并给了明确的时刻表：
//!
//! | 里程碑 | 实现哪些 trait |
//! |---|---|
//! | **M1** | **`ProductIdentity` + `Launchable`** |
//! | M2 | 加 `Discoverable` + `Installable` |
//! | M3 | 加 `Diagnosable` |
//! | M6 / M8 | 加 `ContentManageable` + `Importable` |
//! | M9 | 用第二个产品**验证前六个 trait 的形状** |
//!
//! **所以本文件刻意只有两个 trait。** 加第三个的时机是"出现了第二个使用场景"，
//! 而不是"我想到了它可能有用"。
//!
//! ## 产品名不许进内核（护栏会拦）
//!
//! 与 `identity.rs` 同一条纪律：**本文件里一个字的产品名都没有**。
//! 唯一的 `MockProvider` 住在 `tests/` 下（护栏允许测试里出现产品名）。
//! 这不是洁癖 —— 它保证"加第二个产品"是**注册一个 Provider**，而不是**改内核**。

use crate::plan::LaunchPlan;
use crate::retry::CancelToken;
use std::collections::BTreeMap;

/// 产品的**版本形态**（方案 §3.2 的"名称、图标、版本形态"）。
///
/// 叫"形态"而不是"版本"，因为**同一个产品的两条线不是同一回事**：
/// 例如"正式版"与"快照"是两种发布节奏，而"商店版"与"独立版"是两种分发渠道。
/// 界面要展示的是这个层级，不是具体版本号（具体版本号属于 `Installable`，M2）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VariantDescriptor {
    /// 稳定标识（界面与配置都用它，不用展示名）
    pub key: String,
    /// 展示名（**属于 Provider 的品牌文案**，内核只当它是一个字符串）
    pub label: String,
    /// 这个形态是否**需要商店授权**才能使用。
    ///
    /// 它决定"哪些身份来源能用它"（见 `identity::IdentitySource::provides_store_license`）。
    /// **内核只做这个通用判断，不问是哪个产品。**
    pub needs_store_license: bool,
}

/// **产品的身份与展示信息**（方案 §3.2：全部 Provider 都实现的唯一必需项）。
pub trait ProductIdentity: Send + Sync {
    /// 稳定产品码（如 `"mc-java"`）。**进配置、进日志、进能力键名。**
    ///
    /// **必须 ASCII 且不含空格**：它要进配置文件的键、
    /// 而带空格或非 ASCII 的键在不同工具里表现不一致。
    fn key(&self) -> &'static str;

    /// 展示名。**内核不解析它，只把它交给界面。**
    fn display_name(&self) -> &'static str;

    /// 版本形态列表（至少一个）。
    fn variants(&self) -> Vec<VariantDescriptor>;
}

/// 规划或启动失败的**形状**（不是具体错误）。
///
/// 刻意不用 `Box<dyn Error>`：
/// - 那个 trait 会**把具体错误类型抹掉**，而我们要把它映射到 `error::QulError`
///   才能得到"错误码 + 原因 + 建议 + 可点击操作"；
/// - `qul-core` 也不该依赖调用方会怎么呈现错误。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlanError {
    /// 错误码（取自 [`crate::error::ErrorCode`]）
    pub code: crate::error::ErrorCode,
    /// 发生了什么（面向用户，不含内部路径与令牌）
    pub reason: String,
    /// 你该做什么
    pub suggestion: String,
}

impl PlanError {
    pub fn new(
        code: crate::error::ErrorCode,
        reason: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            code,
            reason: reason.into(),
            suggestion: suggestion.into(),
        }
    }

    /// 转成统一的用户可见错误（级别、容器、可点击操作都在那一侧）。
    pub fn to_qul_error(&self) -> crate::error::QulError {
        crate::error::QulError::new(self.code, self.reason.clone(), self.suggestion.clone())
    }
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.reason)
    }
}

impl std::error::Error for PlanError {}

/// **进程执行器**：把"启动一个进程"这件事抽象出来。
///
/// ## 为什么它是一个参数而不是直接调 `std::process`
///
/// 三条理由，每条都具体：
///
/// 1. **内核不许碰 IO**（架构测试强制）→ 所以 trait 在 `qul-core`，
///    而实现（真的 `Command::spawn`）在 `qul-infra`。
/// 2. **M1 的出口条件要验的是"调用链通不通"**，而不是"游戏能不能跑"。
///    用一个假执行器就能验前者，**而那让测试不依赖本机装没装游戏**。
/// 3. **M3 要对着真实游戏调几十个参数**。若每次都真启动，
///    成本是几十次游戏启动；而用执行器记录"打算怎么启动"是免费的。
pub trait ProcessExecutor {
    /// 启动并等待结束，返回退出码。
    ///
    /// `cancel` 用于**可中断**：方案 M1 明写"取消/重试语义"。
    /// 返回 `None` 表示**被取消**（而不是"失败"）——
    /// 取消是用户的意愿，不是故障（见 `retry::Outcome`）。
    fn run(
        &self,
        cmd: &crate::plan::ResolvedCommand,
        cancel: &CancelToken,
    ) -> Result<Option<i32>, PlanError>;
}

/// **可启动**（方案 §3.2：产出**可复现**启动计划 + 执行启动并回收进程）。
pub trait Launchable: ProductIdentity {
    /// 产出启动计划。**`facts` 是本机事实**（运行时路径、内存、账户……）。
    ///
    /// ## 为什么 `facts` 是一个不透明的键值表
    ///
    /// 如果签名写成 `fn plan(&self, java: &Path, memory_mb: u32, account: &Account)`，
    /// 那么**内核就知道了"启动需要这些东西"** —— 而第二个产品不需要 Java。
    /// 于是加产品就变成改签名、改所有 Provider、改所有调用点。
    ///
    /// 用键值表之后，**"需要什么"是 Provider 的知识**，
    /// 调用方只需保证"给它要的那些"；而"它要哪些"可以被列举
    /// （见 [`Launchable::required_facts`]）。
    fn plan(&self, variant: &str, facts: &Facts) -> Result<LaunchPlan, PlanError>;

    /// **这个产品需要调用方准备哪些事实？**
    ///
    /// 它的作用是让"给全"成为一件**可检查**的事：
    /// 调用方可以在启动前比对"需要 vs 已有"，而不是等启动失败再猜。
    fn required_facts(&self) -> Vec<FactKey>;

    /// 执行一个**已解析**的命令并回收进程。
    fn launch(
        &self,
        variant: &str,
        facts: &Facts,
        executor: &dyn ProcessExecutor,
        cancel: &CancelToken,
    ) -> Result<Option<i32>, PlanError> {
        let plan = self.plan(variant, facts)?;
        let cmd = plan.resolve().map_err(|un| {
            // 未解析的占位符必须变成一个**能定位**的错误，
            // 而不是一个泛泛的"启动失败"。
            let names: Vec<String> = un.iter().map(|u| u.name.clone()).collect();
            PlanError::new(
                crate::error::ErrorCode::PlanUnresolvedPlaceholder,
                format!("启动计划里有未解析的占位符：{}", names.join("、")),
                "这通常是启动器内部缺陷；请导出诊断日志以便定位。",
            )
        })?;
        executor.run(&cmd, cancel)
    }
}

/// 一个**本机事实**的键。
///
/// 用新类型而不是裸 `String`，因为**它要进比较与列举**，
/// 而裸字符串会让"拼错了的键"静默地永远取不到值 ——
/// 那会表现成"启动参数里少了一项"，非常难查。
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct FactKey(String);

impl FactKey {
    /// 构造。**必须是 ASCII 小写点分形式**（`java.path` / `memory.mb`）。
    ///
    /// 强制这一条的理由：它是**跨层契约**（Provider 声明、编排层提供、
    /// 界面可能展示），而带空格或大写的键会在某处被规范化后对不上。
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for FactKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 本机事实表。**这就是"给它要的那些"的载体。**
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Facts {
    map: BTreeMap<String, String>,
}

impl Facts {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(mut self, key: FactKey, value: impl Into<String>) -> Self {
        self.map.insert(key.0, value.into());
        self
    }

    pub fn get(&self, key: &FactKey) -> Option<&str> {
        self.map.get(&key.0).map(String::as_str)
    }

    /// **缺哪些事实？** 返回全部缺失项（不只看第一个）。
    ///
    /// 与 [`crate::plan::LaunchPlan::unresolved`] 同一个理由：
    /// 只报一个的话，用户修一个再跑一次又冒出一个。
    pub fn missing(&self, required: &[FactKey]) -> Vec<FactKey> {
        required
            .iter()
            .filter(|k| !self.map.contains_key(k.as_str()))
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// **产品注册表**：按能力**查询**，而不是要求每个 Provider 都实现全部。
///
/// 方案 §3.2 的落点：*"内核对形状的依赖：`ProductRegistry` 按 trait 能力查询"*。
///
/// ## M1 的形状是刻意的
///
/// 现在只登记**一个** `ProductIdentity + Launchable` 的组合。
/// 等 M2 有了 `Discoverable`，这里会长出**按能力查询**的方法
/// （例如"列出所有能发现的 Provider"）——**而不是给现在这个加方法**。
/// 那正是"窄 trait"要换来的东西：**加能力 = 加查询，不是加必填实现**。
#[derive(Default)]
pub struct ProductRegistry {
    products: Vec<Box<dyn Launchable>>,
}

impl ProductRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册一个产品。**重复的产品码会被拒绝**——
    /// 两个 Provider 用同一个码会让"按码查"变成一件不确定的事，
    /// 而那种错误会表现成"界面上某个入口指向了别的产品"。
    pub fn register(&mut self, p: Box<dyn Launchable>) -> Result<(), String> {
        let k = p.key();
        if self.products.iter().any(|e| e.key() == k) {
            return Err(format!("产品码 {k} 已注册"));
        }
        self.products.push(p);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.products.len()
    }

    pub fn is_empty(&self) -> bool {
        self.products.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&dyn Launchable> {
        self.products
            .iter()
            .find(|p| p.key() == key)
            .map(|b| b.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Launchable> {
        self.products.iter().map(|b| b.as_ref())
    }
}

impl std::fmt::Debug for ProductRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProductRegistry")
            .field(
                "products",
                &self.products.iter().map(|p| p.key()).collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::plan::ResolvedCommand;
    use std::sync::Mutex;

    /// 测试用的**假产品**。
    ///
    /// ⚠️ **它住在 `tests` 里，而这是刻意的**：真实产品名属于 Provider crate，
    /// 不属于内核。若哪天有人把一份具体产品搬进 `qul-core` 的生产代码，
    /// `tests/architecture.rs` 会红。
    struct FakeProduct;

    impl ProductIdentity for FakeProduct {
        fn key(&self) -> &'static str {
            "fake-product"
        }
        fn display_name(&self) -> &'static str {
            "假产品"
        }
        fn variants(&self) -> Vec<VariantDescriptor> {
            vec![
                VariantDescriptor {
                    key: "release".into(),
                    label: "正式版".into(),
                    needs_store_license: false,
                },
                VariantDescriptor {
                    key: "store".into(),
                    label: "商店版".into(),
                    needs_store_license: true,
                },
            ]
        }
    }

    impl Launchable for FakeProduct {
        fn plan(&self, variant: &str, facts: &Facts) -> Result<LaunchPlan, PlanError> {
            if variant == "store" {
                return Err(PlanError::new(
                    ErrorCode::PlanSkeletonMismatch,
                    "商店版不能这样启动",
                    "请改用商店通道。",
                ));
            }
            let runtime = FactKey::new("runtime.path");
            let mem = FactKey::new("memory.mb");
            let missing = facts.missing(&[runtime.clone(), mem.clone()]);
            if !missing.is_empty() {
                let names: Vec<String> = missing.iter().map(|k| k.to_string()).collect();
                return Err(PlanError::new(
                    ErrorCode::JavaNotFound,
                    format!("缺少本机事实：{}", names.join("、")),
                    "这通常是启动器内部缺陷；请导出诊断日志。",
                ));
            }
            Ok(LaunchPlan::new("{RUNTIME}")
                .arg("-Xmx{MEM}M")
                .bind("RUNTIME", facts.get(&runtime).unwrap_or(""))
                .bind("MEM", facts.get(&mem).unwrap_or("")))
        }

        fn required_facts(&self) -> Vec<FactKey> {
            vec![FactKey::new("runtime.path"), FactKey::new("memory.mb")]
        }
    }

    /// 记录型执行器：**不真的启动任何东西**，只把收到的命令记下来。
    struct RecordingExecutor {
        seen: Mutex<Vec<ResolvedCommand>>,
    }

    impl RecordingExecutor {
        fn new() -> Self {
            Self {
                seen: Mutex::new(Vec::new()),
            }
        }
        fn count(&self) -> usize {
            self.seen.lock().unwrap().len()
        }
        fn last(&self) -> Option<ResolvedCommand> {
            self.seen.lock().unwrap().last().cloned()
        }
    }

    impl ProcessExecutor for RecordingExecutor {
        fn run(
            &self,
            cmd: &ResolvedCommand,
            cancel: &CancelToken,
        ) -> Result<Option<i32>, PlanError> {
            if cancel.is_cancelled() {
                return Ok(None);
            }
            self.seen.lock().unwrap().push(cmd.clone());
            Ok(Some(0))
        }
    }

    fn facts() -> Facts {
        Facts::new()
            .set(FactKey::new("runtime.path"), r"C:\jdk\bin\java.exe")
            .set(FactKey::new("memory.mb"), "4096")
    }

    #[test]
    fn 窄_trait_只要求产品身份与可启动() {
        // 这条测试钉住"M1 只定两个 trait"这个决定：
        // 若有人提前加了第三个必填方法，这里的实现会编不过 ——
        // **而那正是我们想要的提醒**（方案 §3.2 的纪律：
        // "每个 trait 在出现第二个使用场景时引入，不在纸面上预抽"）。
        let p = FakeProduct;
        assert_eq!(p.key(), "fake-product");
        assert_eq!(p.variants().len(), 2);
        assert_eq!(p.required_facts().len(), 2);
    }

    #[test]
    fn 计划里没有未解析的占位符() {
        let plan = FakeProduct.plan("release", &facts()).unwrap();
        assert!(plan.unresolved().is_empty());
        let cmd = plan.resolve().unwrap();
        assert_eq!(cmd.program, r"C:\jdk\bin\java.exe");
        assert_eq!(cmd.args, vec!["-Xmx4096M"]);
    }

    #[test]
    fn 缺少本机事实时报错而不是产出坏计划() {
        // 若缺了内存值却照常产计划，参数会变成 `-XmxM` ——
        // 游戏会拒绝启动，而报错信息里看不出是启动器少给了一个值。
        let e = FakeProduct.plan("release", &Facts::new()).unwrap_err();
        assert!(e.reason.contains("runtime.path"), "{}", e.reason);
        assert!(e.reason.contains("memory.mb"), "必须一次报全：{}", e.reason);
    }

    #[test]
    fn 缺事实的列举是完整的() {
        let partial = Facts::new().set(FactKey::new("memory.mb"), "1024");
        let missing = partial.missing(&FakeProduct.required_facts());
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].as_str(), "runtime.path");
    }

    #[test]
    fn 启动链路走通并回收退出码() {
        let ex = RecordingExecutor::new();
        let cancel = CancelToken::new();
        let code = FakeProduct
            .launch("release", &facts(), &ex, &cancel)
            .unwrap();
        assert_eq!(code, Some(0));
        assert_eq!(ex.count(), 1, "执行器应当收到恰好一次命令");
        let cmd = ex.last().unwrap();
        assert_eq!(cmd.program, r"C:\jdk\bin\java.exe");
        assert_eq!(cmd.args, vec!["-Xmx4096M"]);
    }

    #[test]
    fn 取消不产生启动而且不是失败() {
        // 取消是用户的意愿，不是故障（见 retry::Outcome 的文档）。
        let ex = RecordingExecutor::new();
        let cancel = CancelToken::new();
        cancel.cancel();
        let got = FakeProduct
            .launch("release", &facts(), &ex, &cancel)
            .unwrap();
        assert_eq!(got, None, "被取消时返回 None 而不是错误");
        assert_eq!(ex.count(), 0, "取消后不该真的启动");
    }

    #[test]
    fn 计划失败不会被送到执行器() {
        // 规划都失败了还去启动，等于把一个已知错误的命令交给操作系统。
        let ex = RecordingExecutor::new();
        let cancel = CancelToken::new();
        let e = FakeProduct
            .launch("store", &facts(), &ex, &cancel)
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::PlanSkeletonMismatch);
        assert_eq!(ex.count(), 0, "规划失败时执行器不该被调用");
    }

    #[test]
    fn 注册表拒绝重复产品码() {
        let mut r = ProductRegistry::new();
        r.register(Box::new(FakeProduct)).unwrap();
        let e = r.register(Box::new(FakeProduct)).unwrap_err();
        assert!(e.contains("fake-product"), "{e}");
        assert_eq!(r.len(), 1, "重复注册不该改变数量");
    }

    #[test]
    fn 注册表可以按码查到产品() {
        let mut r = ProductRegistry::new();
        r.register(Box::new(FakeProduct)).unwrap();
        let p = r.get("fake-product").expect("应当能查到");
        assert_eq!(p.display_name(), "假产品");
        assert!(r.get("nope").is_none());
        assert_eq!(r.iter().count(), 1);
    }

    #[test]
    fn 计划的错误能转成统一的用户可见错误() {
        // 方案 §5.7 要求错误带级别与容器；`PlanError` 只带码与文案，
        // 转换时必须把级别与容器补齐（它们来自错误码本身）。
        let e = PlanError::new(ErrorCode::JavaNotFound, "没找到运行时", "请安装运行时。");
        let q = e.to_qul_error();
        assert_eq!(q.id, "QUL-JAVA-0001");
        assert_eq!(q.severity, crate::error::Severity::Panel);
        assert_eq!(q.presentation, "island-expanded");
        assert!(q.is_user_visible());
        // Display 里必须带编号，否则用户报错时无法定位
        assert!(format!("{e}").contains("QUL-JAVA-0001"));
    }

    #[test]
    fn 形态描述符区分是否需要商店授权() {
        // 这个布尔值是"哪些身份来源能用它"的唯一依据，
        // 所以它必须是**每个形态自己声明**的，而不是产品级的一个值。
        let vs = FakeProduct.variants();
        assert!(!vs[0].needs_store_license);
        assert!(vs[1].needs_store_license);
    }
}
