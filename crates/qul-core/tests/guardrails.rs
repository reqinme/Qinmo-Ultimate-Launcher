//! # 护栏的"能红"证明（M0 · S4）
//!
//! ## 为什么需要这个文件
//!
//! 任务书 S4 的通过标准不是"护栏能绿"，而是：
//!
//! > **上述三条护栏都能故意踩红。**
//!
//! **"能绿"和"能红"是两件事。** 一个永远绿的检查，和没有检查是一样的——
//! 而它更危险，因为它**给人"已经守住了"的错觉**。
//!
//! 本项目已经吃过一次同形状的亏：架构测试最初扫的是整份文件文本，
//! 于是**注释里的产品名**也会触发（假红），而**真正该拦的依赖问题却被文本淹没**。
//! 假红会被当成噪声忽略，**接着真红也被忽略**。
//!
//! ## 本文件的做法：把"判定"从"断言"里拆出来
//!
//! 每个护栏都是一个**纯函数**：`输入 → 违规清单`。
//! 于是可以对它做两件独立的事：
//!
//! 1. **对干净输入**：断言清单为空（护栏不该误报）
//! 2. **对脏输入**：断言清单非空（护栏必须能报）
//!
//! 第 2 条就是"踩红"——**它不需要把仓库改坏**，因为脏输入是**构造出来的**，
//! 而不是**注入到真实文件里的**。
//!
//! > **这是本文件最重要的设计决定**：S4 要求"能踩红"，
//! > 但如果真的往 `Cargo.toml` 里写个 `reqwest` 来踩，`cargo test` 就再也跑不起来，
//! > 那样得到的"证据"是**一次性的、不可回归的**。
//! > 用构造输入来踩，则这条证明**每次 CI 都会重跑一遍**。

use std::path::{Path, PathBuf};

// ───────────────────────── 护栏 1：内核不许依赖 IO 或界面框架 ─────────────────────────

/// 这些名字一旦出现在 `qul-core` 的依赖里，内核就不再是内核。
///
/// **为什么是黑名单而不是白名单**：白名单会随正常开发不断要改
/// （每加一个纯数据类型都要放行），**摩擦会让人想绕过它**；
/// 黑名单只需要在"引入 IO/网络/界面能力"时挡住，而那正是我们要挡的。
const FORBIDDEN_CORE_DEPS: &[&str] = &[
    // 文件系统 / 进程 / 网络
    "std-fs-shim",
    "tokio",
    "async-std",
    "reqwest",
    "ureq",
    "hyper",
    // 界面 / 运行时
    "tauri",
    "wry",
    "tao",
    "winapi",
    "windows",
    "windows-sys",
    // 直接绑窗口材质的库（属于 infra，不属于内核）
    "window-vibrancy",
];

/// 解析 `Cargo.toml` 文本，返回**依赖名**（全小写）。
///
/// 只看 `[dependencies]` / `[dev-dependencies]` / `[build-dependencies]` 段。
/// **刻意不扫整份文件**：注释与 `[package]` 里的描述都会含正常英文词，
/// 扫全文会误报（这条教训在 `architecture.rs` 里已经记过一次）。
pub fn parse_dependency_names(cargo_toml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_deps = false;

    for line in cargo_toml.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            in_deps = line.contains("dependencies");
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            let name = key.trim().split('.').next().unwrap_or("").trim();
            if !name.is_empty() {
                names.push(name.to_lowercase());
            }
        }
    }
    names
}

/// 护栏 1 的判定：返回**违规的依赖名**（空 = 通过）。
pub fn forbidden_core_deps(cargo_toml: &str) -> Vec<String> {
    parse_dependency_names(cargo_toml)
        .into_iter()
        .filter(|n| FORBIDDEN_CORE_DEPS.contains(&n.as_str()))
        .collect()
}

/// 源码里出现这些**路径**也说明内核在碰外部世界。
///
/// 依赖黑名单拦不住 `std::fs`——它是标准库，不在 `Cargo.toml` 里。
/// **两个检查缺一不可**：一个管"外部 crate"，一个管"标准库能力"。
const FORBIDDEN_CORE_PATHS: &[&str] = &[
    "std::fs",
    "std::net",
    "std::process",
    "std::env::set_var",
    "std::thread::spawn",
];

/// 剥掉行注释与块注释，避免注释里的词触发误报。
pub fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut in_line = false;
    let mut in_block = 0usize;

    while i < bytes.len() {
        let c = bytes[i];
        let next = bytes.get(i + 1).copied().unwrap_or('\0');

        if in_line {
            if c == '\n' {
                in_line = false;
                out.push(c);
            }
            i += 1;
            continue;
        }
        if in_block > 0 {
            if c == '*' && next == '/' {
                in_block -= 1;
                i += 2;
                continue;
            }
            if c == '/' && next == '*' {
                in_block += 1;
                i += 2;
                continue;
            }
            if c == '\n' {
                out.push(c);
            }
            i += 1;
            continue;
        }
        if c == '/' && next == '/' {
            in_line = true;
            i += 2;
            continue;
        }
        if c == '/' && next == '*' {
            in_block += 1;
            i += 2;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// 护栏 1b 的判定：源码里出现的**违规路径**（空 = 通过）。
pub fn forbidden_core_paths(src: &str) -> Vec<String> {
    let code = strip_comments(src);
    FORBIDDEN_CORE_PATHS
        .iter()
        .filter(|p| code.contains(**p))
        .map(|p| (*p).to_string())
        .collect()
}

// ───────────────────────── 真实仓库检查 ─────────────────────────

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn core_src_files() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(&manifest_dir().join("src"), &mut out);
    out.sort();
    out
}

// ───────────────────────── 护栏 1 的证明 ─────────────────────────

#[test]
fn 护栏一_对干净输入不误报() {
    let cargo = std::fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("读 Cargo.toml");
    assert_eq!(
        forbidden_core_deps(&cargo),
        Vec::<String>::new(),
        "qul-core 真的引入了 IO/界面依赖——这正是护栏要拦的"
    );

    for f in core_src_files() {
        let src = std::fs::read_to_string(&f).expect("读源文件");
        let bad = forbidden_core_paths(&src);
        assert!(
            bad.is_empty(),
            "{} 里出现了违规路径 {bad:?}——内核不该碰 IO",
            f.display()
        );
    }
}

#[test]
fn 护栏一_对脏输入必须报红() {
    // ── 脏依赖 ────────────────────────────────────────────────
    let dirty_cargo = r#"
[package]
name = "qul-core"
description = "这里提到 windows 与 tauri 也不算数"

[dependencies]
serde = "1"
reqwest = "0.12"
window-vibrancy = "0.6"

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
"#;
    let mut hit = forbidden_core_deps(dirty_cargo);
    hit.sort();
    assert_eq!(
        hit,
        vec!["reqwest", "tokio", "window-vibrancy"],
        "护栏一没能识别出脏依赖"
    );

    // ── 脏路径 ────────────────────────────────────────────────
    let dirty_src = r#"
// 注释里写 std::fs 不该算数
fn f() { let _ = std::fs::read_to_string("x"); }
fn g() { std::process::Command::new("cmd"); }
"#;
    let mut hit2 = forbidden_core_paths(dirty_src);
    hit2.sort();
    assert_eq!(
        hit2,
        vec!["std::fs", "std::process"],
        "护栏一没能识别出脏路径"
    );
}

#[test]
fn 护栏一_注释里的违规词不算违规() {
    // 这条是"假红"的防线：本项目曾因扫全文文本而误报。
    let commented = r#"
//! 内核绝不调用 std::fs 或 reqwest。
// std::process::Command
/* std::net 也不行 */
fn clean() -> u32 { 1 }
"#;
    assert_eq!(
        forbidden_core_paths(commented),
        Vec::<String>::new(),
        "被注释掉的违规词被误判成真违规（假红）"
    );
}

// ───────────────────────── 护栏 3：CLI 与界面必须复用同一个内核 ─────────────────────────

/// 护栏的**内容**是"`qul-app` 是唯一的编排层"：
/// `src-tauri` 与 CLI 都只许调它，不许各自实现一套。
///
/// 这里证明的是**判定函数能红**；真实检查在 `qul-app` 的测试里。
///
/// 判据：编排层文件里出现"直接调 provider/infra"的路径即为违规。
pub fn forbidden_direct_infra_calls(src: &str) -> Vec<String> {
    let code = strip_comments(src);
    ["qul_infra::", "qul_provider_"]
        .iter()
        .filter(|p| code.contains(**p))
        .map(|p| (*p).to_string())
        .collect()
}

#[test]
fn 护栏三_对干净输入不误报() {
    let ok = r#"
use qul_app::AppService;
fn main() { println!("{}", AppService::new().describe()); }
"#;
    assert_eq!(forbidden_direct_infra_calls(ok), Vec::<String>::new());
}

#[test]
fn 护栏三_对脏输入必须报红() {
    let dirty = r#"
fn main() {
    let x = qul_infra::fs::read("a");
    let y = qul_provider_java::discover();
}
"#;
    let mut hit = forbidden_direct_infra_calls(dirty);
    hit.sort();
    assert_eq!(hit, vec!["qul_infra::", "qul_provider_"]);
}
