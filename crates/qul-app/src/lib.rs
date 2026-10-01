//! # qul-app —— 编排层
//!
//! ## 这一层为什么存在
//!
//! 方案 §3 的分层是：`qul-core`（规则）→ **`qul-app`（编排）** → `qul-infra`（IO）
//! 与 `src-tauri`（界面）/ CLI。
//!
//! **编排层是"界面与命令行共用的那一份逻辑"的落点。**
//! 判据很简单：**如果一段逻辑既要给按钮用、又要给 CLI 用，它就必须在这一层**，
//! 而不许在两边各写一遍。
//!
//! ## 它不许做什么（靠测试强制，不靠自觉）
//!
//! - **不许直接 `use qul_infra::` 或 `qul_provider_*`**：
//!   那会绕开编排、让两层逻辑分叉。真实调用要走本层的公开函数。
//! - **不许碰界面类型**（`tauri::`）：编排层要能被 CLI 复用，
//!   一旦依赖 Tauri 就复用了不了。
//!
//! 这两条由 `tests/layering.rs` 检查，并且**那个测试自己也被证明过能红**
//! （见 `qul-core` 的 `tests/guardrails.rs`）。

use qul_core::{Capabilities, Overview};

/// 启动器的编排入口。
///
/// **现阶段（M0）它只做一件事**：把内核的能力判断包成"可展示的概览"。
/// 这是 S4 要求的**最小链路样板**——它要验的是**调用路径通不通**，
/// 所以刻意选一个**纯计算、无副作用**的动作：
/// 链路测试不该被"副作用对不对"干扰。
///
/// **`S` 这个泛型参数是为 M1 留的**（注入 provider），
/// 现在不用但先立在那儿，避免以后为了测试性回头改所有签名。
#[derive(Debug, Clone, Default)]
pub struct AppService {
    capabilities: Capabilities,
}

impl AppService {
    /// 用一份能力表构造。
    pub fn with_capabilities(capabilities: Capabilities) -> Self {
        Self { capabilities }
    }

    /// 空服务（没有任何能力登记）。
    pub fn new() -> Self {
        Self::default()
    }

    /// **界面与 CLI 共用的那一次计算**：把能力表压成可展示的概览。
    ///
    /// 界面侧：Tauri 命令 `capability_overview` 调它。
    /// 命令行侧：`qul-cli` 调它。
    /// **两处调的是同一个函数**——这正是 S4 要证明的事。
    pub fn capability_overview(&self) -> Overview {
        self.capabilities.overview()
    }

    /// **界面与 CLI 共用的第二件事**：给定游戏版本与候选，选出该用的 Java。
    ///
    /// 编排层只做一次转接，但这次转接是必要的：
    /// 否则界面会去调 `qul_core::java::choose_java`，而 CLI 也去调它，
    /// 两处各写一遍"先把版本号解析成要求"的顺序 —— 那种分叉正是 S4 要防的。
    ///
    /// 解析失败返回 `Err`，**不返回一个猜的选择**（见 `GameVersion::parse` 的注释）。
    pub fn java_choice_for(
        &self,
        game_version: &str,
        candidates: &[qul_core::java::JavaCandidate],
    ) -> Result<qul_core::java::JavaChoice, String> {
        let gv = qul_core::java::GameVersion::parse(game_version)
            .ok_or_else(|| format!("无法解析游戏版本号：{game_version}"))?;
        Ok(qul_core::java::choose_java(candidates, gv.requirement()))
    }
    /// 概览的一行文字版（给 CLI 用；界面不用它）。
    ///
    /// **放在编排层而不是 CLI 里**：措辞属于产品行为，
    /// 不该因为"只有命令行用"就写进命令行程序——否则以后界面也要这句话时，
    /// 就得从 CLI 里搬出来。
    pub fn capability_summary_line(&self) -> String {
        let o = self.capability_overview();
        format!(
            "可用 {} 项 / 不可用 {} 项",
            o.enabled_count, o.disabled_count
        )
    }

    // ─────────────────── M1：产品调用链（注册 → 规划 → 执行）───────────────────
    //
    // ## 为什么这三个函数必须在编排层，而不是在 CLI 里
    //
    // M1 的出口条件是 **"`MockProvider` 走通 CLI 调用链"**。
    // 若把"查注册表 → 查缺哪些事实 → 规划 → 解析 → 执行"这段顺序写在 CLI 里，
    // 那么界面（M4）要用同一条链时**只能再写一遍** —— 而那正是 S4 要防的分叉。
    //
    // **顺序本身是这里唯一的知识**，而它是会出错的那部分：
    // 少查一次"缺哪些事实"，就会把一个带默认值的命令交给操作系统；
    // 忘了先 `resolve` 就执行，就会把 `{RUNTIME}` 当成路径去打开。
    // 所以顺序留在这一层，调用方只提供"用哪个产品、有哪些事实、用谁执行"。
    //
    // ## 为什么注册表是参数而不是字段
    //
    // 因为 `AppService` 是 `Clone + Default` 的（M0 的链路样板就依赖这一点），
    // 而装着 `Box<dyn ...>` 的字段会让两者都失去。
    // **把"提供者从哪来"与"顺序是什么"分开**，两边都简单。

    /// 列出注册表里的全部产品（界面侧渲染产品切换用）。
    ///
    /// 返回 `(产品码, 展示名, 形态列表)`。
    /// **刻意返回元组而不是自定义结构体**：`tests/layering.rs` 断言
    /// 编排层只暴露 `AppService` 一个公开结构体，
    /// 而那条断言是在防"编排层成为第二个内核"——
    /// 所以这里宁可用元组，也不新造一个契约。
    pub fn products(
        &self,
        registry: &qul_core::provider::ProductRegistry,
    ) -> Vec<(
        &'static str,
        &'static str,
        Vec<qul_core::provider::VariantDescriptor>,
    )> {
        registry
            .iter()
            .map(|p| (p.key(), p.display_name(), p.variants()))
            .collect()
    }

    /// **规划并解析**：产出可以直接交给操作系统的命令。
    ///
    /// 界面侧用它做"启动前预览"（把要执行的命令显示出来，
    /// 这是方案 §5.8 安全基线里"执行必须可被检查"的落点）；
    /// CLI 用它打印命令。
    ///
    /// **它不执行任何东西** —— 所以"参数对不对"可以在不启动进程的前提下被断言。
    pub fn plan_command(
        &self,
        registry: &qul_core::provider::ProductRegistry,
        product: &str,
        variant: &str,
        facts: &qul_core::provider::Facts,
    ) -> Result<qul_core::plan::ResolvedCommand, qul_core::provider::PlanError> {
        let p = registry.get(product).ok_or_else(|| {
            let known: Vec<&str> = registry.iter().map(|x| x.key()).collect();
            qul_core::provider::PlanError::new(
                qul_core::error::ErrorCode::MetaVersionInvalid,
                format!("没有注册名为 {product} 的产品"),
                format!("已注册的产品：{}", known.join("、")),
            )
        })?;

        // **先查"缺哪些本机事实"，再规划。**
        // 反过来（先规划，失败再猜）会让错误信息变成"未知失败"——
        // 而"缺一个事实"与"事实的值不对"是完全不同的两件事。
        let missing = facts.missing(&p.required_facts());
        if !missing.is_empty() {
            let names: Vec<String> = missing.iter().map(|k| k.to_string()).collect();
            return Err(qul_core::provider::PlanError::new(
                qul_core::error::ErrorCode::PlanUnresolvedPlaceholder,
                format!("缺少本机事实：{}", names.join("、")),
                "这通常是启动器内部缺陷；请导出诊断日志以便定位。",
            ));
        }

        let plan = p.plan(variant, facts)?;
        plan.resolve().map_err(|un| {
            let names: Vec<String> = un.iter().map(|u| u.name.clone()).collect();
            qul_core::provider::PlanError::new(
                qul_core::error::ErrorCode::PlanUnresolvedPlaceholder,
                format!("启动计划里有未解析的占位符：{}", names.join("、")),
                "这通常是启动器内部缺陷；请导出诊断日志以便定位。",
            )
        })
    }

    /// **执行启动并回收进程。**
    ///
    /// 它内部走的是 [`AppService::plan_command`] —— **同一个顺序**，
    /// 所以界面与 CLI 不可能"一个先查事实、一个不查"。
    ///
    /// 返回 `Ok(None)` 表示**被取消**（不是失败）：
    /// 取消是用户的意愿，见的 `retry::Outcome` 文档。
    pub fn launch(
        &self,
        registry: &qul_core::provider::ProductRegistry,
        product: &str,
        variant: &str,
        facts: &qul_core::provider::Facts,
        executor: &dyn qul_core::provider::ProcessExecutor,
        cancel: &qul_core::retry::CancelToken,
    ) -> Result<Option<i32>, qul_core::provider::PlanError> {
        let cmd = self.plan_command(registry, product, variant, facts)?;
        executor.run(&cmd, cancel)
    }
}
