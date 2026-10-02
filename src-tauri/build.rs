//! Tauri 的构建脚本。
//!
//! ## 它只做一件事
//!
//! `tauri_build::build()` 读 `tauri.conf.json` 并生成**能力（capabilities）的
//! 校验代码**、图标资源与 `cargo:rerun-if-changed` 指令。
//!
//! ## ⚠️ 而能力校验是它最重要的产物
//!
//! 它把 `capabilities/*.json` **在构建期**翻译成"哪些命令在这个窗口上被允许"。
//! 于是"前端能不能调某条命令"不是运行时的判断，而是**构建期就定下来的** ——
//! 一条没被授予权限的命令，前端调它会**失败**，而不是"恰好能调"。
//!
//! 这也解释了为什么 `capabilities/main-window.json` 里那个空
//! `permissions` 数组是**有约束力的**：它让"这个窗口能做什么"的答案是
//! **只有我们显式导出的命令**。
//!
//! ## 为什么没有 `println!("cargo:rerun-if-changed=...")`
//!
//! 因为那是 `tauri_build::build()` 的内部职责 —— 一个在它之外再声明一遍的
//! 实现会让两处不一致，而"哪个在生效"变成一个问题。

fn main() {
    tauri_build::build();
}
