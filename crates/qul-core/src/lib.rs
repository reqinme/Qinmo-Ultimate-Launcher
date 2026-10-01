//! # qul-core —— 内核
//!
//! **零 IO、零 Tauri、零 Minecraft 词汇。**
//!
//! 这一层只放三样东西：
//! 1. **通用模型**（实例、任务、进度、错误码……）
//! 2. **规则**（权限、能力、依赖方向这类"谁能做什么"的判定）
//! 3. **契约**（各层之间交换的数据形状）
//!
//! 它不认识"Minecraft"，也不认识"窗口"。任何需要碰磁盘、网络或界面的东西
//! 都属于 `qul-infra` / `qul-provider-*` / `src-tauri`。
//!
//! ## 为什么这个约束是硬的
//!
//! 方案 §1.4 的红线之一是**"不做别的游戏的 Provider"**——但真正会腐化内核的
//! 不是别的游戏，是**游戏专属概念自己长进来**（比如内核里出现 `mods_dir`）。
//! 所以这条靠**架构测试**（`tests/architecture.rs` 的关键词扫描）强制，不靠自觉。

#![forbid(unsafe_code)]

pub mod caps;
pub mod layout;

pub mod crash;
pub mod descriptor;
pub mod download;
pub mod error;
pub mod http;
pub mod i18n;
pub mod identity;
pub mod inflate;
pub mod instance;
pub mod java;
pub mod migrate;
pub mod offline;
pub mod plan;
pub mod provider;
pub mod retry;
pub mod scrub;
pub mod source;
pub mod tasks;
pub mod timeout;
pub mod zip;

pub use caps::{
    Capabilities, Capability, CapabilityKey, CapabilityKind, DetailKey, DisabledItem,
    InstanceDetail, Overview, Packaging, ReasonError,
};
pub use layout::{Layer, RelPath, RelPathError};
