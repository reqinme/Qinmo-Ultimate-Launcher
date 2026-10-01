//! # qul-provider-mock —— **假 Provider**（M1 出口条件专用）
//!
//! ## 它为什么必须是一个真的 crate，而不是测试里的一个 struct
//!
//! 方案 §3.2 的原话：
//!
//! > **收益**：新增一个产品 = **新增一个 Provider crate**，只实现它真有的能力；
//! > 内核与界面代码零改动。
//!
//! 而 M1 的出口条件是 **"`MockProvider` 走通 CLI 调用链"** ——
//! 注意是 **CLI**。若它只住在 `#[cfg(test)]` 里，CLI 就调不到它，
//! 于是那条出口条件只能用"测试里也走了一遍"来交差。**那不算走通调用链。**
//!
//! 所以它做成一个真 crate：**CLI 能注册它、规划它、启动它** ——
//! 而这一路走通，**恰好是"新增一个产品要动几处"的第一次真实测量**。
//!
//! 若某天加真实产品需要改内核或改编排层，**说明这个骨架形式不对**；
//! 而"用假产品先走一遍"是**最便宜的发现方式**（它不依赖任何真实产品装没装）。
//!
//! ## 它刻意不实现任何真实产品
//!
//! 它只有一个假产品码、一个假运行时路径占位符。
//! **它不启动任何真实程序**（执行器由调用方提供，CLI 用的是"只记录不启动"的那个）。
//!
//! ## ⚠️ 这里可以出现产品名，而内核里不行
//!
//! `tests/architecture.rs` 的品牌词规则**只扫 `qul-core` 的生产代码**。
//! Provider crate 正是"产品知识该待的地方"，所以这里出现具体产品名是**对的**。
//! 本 crate 目前不用任何产品名（它连真实产品都不是），但这条边界值得写下来。

use qul_core::plan::{LaunchPlan, ResolvedCommand};
use qul_core::provider::{
    FactKey, Facts, Launchable, PlanError, ProcessExecutor, ProductIdentity, VariantDescriptor,
};
use qul_core::retry::CancelToken;

/// 假产品的产品码。**故意写成一看就知道是假的**。
pub const MOCK_PRODUCT_KEY: &str = "mock-product";

/// 假 Provider。
#[derive(Debug, Default, Clone, Copy)]
pub struct MockProvider;

impl MockProvider {
    pub const fn new() -> Self {
        Self
    }

    /// 本机事实的键。
    ///
    /// **刻意取一个通用名字**（`runtime.path`）而不是某个产品的说法：
    /// 真实的 Java Provider 会用它，而基岩版那类"不由我们启动进程"的产品
    /// 可能完全不需要它 —— 这正是"事实由 Provider 声明"的意思。
    pub fn runtime_key() -> FactKey {
        FactKey::new("runtime.path")
    }

    pub fn memory_key() -> FactKey {
        FactKey::new("memory.mb")
    }

    /// 假的运行时路径（**不指向任何真实文件**）。
    pub const FAKE_RUNTIME: &str = r"C:\fake\runtime\bin\executable.exe";

    /// **演示用参数**：它看起来像一个真实启动参数，但它不带任何品牌。
    ///
    /// 用 `-Ddemo.key=value` 而不是真实的 JVM 参数，
    /// 因为**假的演示不该长得像真的**——否则有人会把它的形状当成规格照抄。
    pub const DEMO_ARG: &str = "-Ddemo.key={MEM}";
}

impl ProductIdentity for MockProvider {
    fn key(&self) -> &'static str {
        MOCK_PRODUCT_KEY
    }

    fn display_name(&self) -> &'static str {
        "假产品（M1 调用链验证用）"
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

impl Launchable for MockProvider {
    fn plan(&self, variant: &str, facts: &Facts) -> Result<LaunchPlan, PlanError> {
        let known = self.variants();
        if !known.iter().any(|v| v.key == variant) {
            let names: Vec<&str> = known.iter().map(|v| v.key.as_str()).collect();
            return Err(PlanError::new(
                qul_core::error::ErrorCode::MetaVersionInvalid,
                format!("未知的版本形态：{variant}"),
                format!("可用形态：{}", names.join("、")),
            ));
        }
        if variant == "store" {
            // 假产品**故意**让商店版失败：这样 CLI 里能演示
            // "能力不可用 → 带原因的禁用态"这条路径。
            return Err(PlanError::new(
                qul_core::error::ErrorCode::PlanSkeletonMismatch,
                "商店版需要商店通道，本假产品不实现它",
                "这是假产品的刻意行为，用于验证错误呈现路径。",
            ));
        }

        let required = self.required_facts();
        let missing = facts.missing(&required);
        if !missing.is_empty() {
            let names: Vec<String> = missing.iter().map(|k| k.to_string()).collect();
            return Err(PlanError::new(
                qul_core::error::ErrorCode::JavaNotFound,
                format!("缺少本机事实：{}", names.join("、")),
                "这通常是启动器内部缺陷；请导出诊断日志以便定位。",
            ));
        }

        Ok(LaunchPlan::new("{RUNTIME}")
            .arg(Self::DEMO_ARG)
            .bind(
                "RUNTIME",
                facts.get(&Self::runtime_key()).unwrap_or_default(),
            )
            .bind("MEM", facts.get(&Self::memory_key()).unwrap_or_default()))
    }

    fn required_facts(&self) -> Vec<FactKey> {
        vec![Self::runtime_key(), Self::memory_key()]
    }
}

/// **只记录、不启动**的执行器（M1 用）。
///
/// ## 为什么它是"生产代码"而不是"测试代码"
///
/// 因为它要被 **CLI 调用**（M1 出口条件要求走 CLI 调用链）。
/// 而 CLI 不该在启动一个假产品时真的去 `Command::spawn` ——
/// 那个路径是 `qul-infra` 的事，且**假产品不该触发真实副作用**。
///
/// 所以它是一个**诚实的实现**：它实现了 `ProcessExecutor` 的语义
/// （返回退出码、响应取消），只是不产生真实进程。
#[derive(Debug, Default)]
pub struct RecordingExecutor {
    seen: std::sync::Mutex<Vec<ResolvedCommand>>,
}

impl RecordingExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 收到过几条命令。
    pub fn count(&self) -> usize {
        self.seen.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// 最后一次收到的命令。
    pub fn last(&self) -> Option<ResolvedCommand> {
        self.seen.lock().ok().and_then(|v| v.last().cloned())
    }

    /// 全部收到的命令（按顺序）。
    pub fn all(&self) -> Vec<ResolvedCommand> {
        self.seen.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

impl ProcessExecutor for RecordingExecutor {
    fn run(&self, cmd: &ResolvedCommand, cancel: &CancelToken) -> Result<Option<i32>, PlanError> {
        // 取消要**先判**：这样"取消后不产生任何记录"是可断言的，
        // 而"先记录再判取消"会让取消看起来像成功启动过。
        if cancel.is_cancelled() {
            return Ok(None);
        }
        match self.seen.lock() {
            Ok(mut v) => {
                v.push(cmd.clone());
                Ok(Some(0))
            }
            Err(_) => Err(PlanError::new(
                qul_core::error::ErrorCode::Unregistered,
                "记录执行器内部状态不可用",
                "这是启动器内部缺陷；请导出诊断日志。",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts::new()
            .set(MockProvider::runtime_key(), MockProvider::FAKE_RUNTIME)
            .set(MockProvider::memory_key(), "4096")
    }

    #[test]
    fn 正式版能产出可解析的计划() {
        let plan = MockProvider.plan("release", &facts()).unwrap();
        assert!(plan.unresolved().is_empty());
        let cmd = plan.resolve().unwrap();
        assert_eq!(cmd.program, MockProvider::FAKE_RUNTIME);
        assert_eq!(cmd.args, vec!["-Ddemo.key=4096"]);
    }

    #[test]
    fn 商店版刻意失败用于验证错误路径() {
        let e = MockProvider.plan("store", &facts()).unwrap_err();
        assert_eq!(e.code, qul_core::error::ErrorCode::PlanSkeletonMismatch);
        // 原因里必须说清"这是刻意的"，否则人会去查一个不存在的缺陷
        assert!(e.suggestion.contains("刻意"), "{}", e.suggestion);
    }

    #[test]
    fn 未知形态的报错要列出可用项() {
        let e = MockProvider.plan("nope", &facts()).unwrap_err();
        assert!(e.reason.contains("nope"), "{}", e.reason);
        assert!(e.suggestion.contains("release"), "{}", e.suggestion);
        assert!(e.suggestion.contains("store"), "{}", e.suggestion);
    }

    #[test]
    fn 缺事实时一次报全() {
        let e = MockProvider.plan("release", &Facts::new()).unwrap_err();
        assert!(e.reason.contains("runtime.path"), "{}", e.reason);
        assert!(e.reason.contains("memory.mb"), "{}", e.reason);
    }

    #[test]
    fn 记录的退出码是零而取消返回空() {
        let ex = RecordingExecutor::new();
        let cancel = CancelToken::new();
        assert_eq!(
            MockProvider
                .launch("release", &facts(), &ex, &cancel)
                .unwrap(),
            Some(0)
        );
        assert_eq!(ex.count(), 1);

        let ex2 = RecordingExecutor::new();
        let c2 = CancelToken::new();
        c2.cancel();
        assert_eq!(
            MockProvider.launch("release", &facts(), &ex2, &c2).unwrap(),
            None
        );
        assert_eq!(ex2.count(), 0, "取消后不该有任何记录");
    }

    #[test]
    fn 本_crate_不出现任何真实产品名() {
        // 它是假产品，所以连"像真的"都不该。
        // 这条测试防的是有人图省事把真实参数名抄进来当演示。
        let corpus = format!(
            "{}{}{}{}",
            MockProvider.display_name(),
            MockProvider::DEMO_ARG,
            MockProvider::FAKE_RUNTIME,
            MockProvider::runtime_key().as_str()
        )
        .to_lowercase();
        for brand in ["mojang", "minecraft", "bedrock", "microsoft", "java"] {
            assert!(
                !corpus.contains(brand),
                "假产品里出现了真实产品名 `{brand}` —— 假的演示不该长得像真的，否则有人会照抄它的形状"
            );
        }
    }
}
