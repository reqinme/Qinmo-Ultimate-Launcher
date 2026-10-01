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
}
