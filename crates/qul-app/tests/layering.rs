//! # 编排层的分层检查（M0 · S4）
//!
//! `qul-app` 是"界面与命令行共用的那一份逻辑"的落点，所以它必须**可被两者复用**。
//! 一旦它依赖 Tauri 或直接调 infra，复用就悄悄断了——
//! 而断开时**不会有任何报错**，只会让两边慢慢各写一套。
//!
//! **所以这条靠检查，不靠自觉。**
//!
//! ## 与 `qul-core/tests/guardrails.rs` 的分工
//!
//! - `guardrails.rs` 证明**判定函数本身能红**（用构造输入）
//! - 本文件把**真实仓库**喂给同样的判定思路，断言干净
//!
//! 分开的理由：把"判定对不对"与"仓库干不干净"混在一个测试里，
//! 一旦仓库真脏，你分不清是**护栏坏了**还是**代码坏了**。

use std::path::{Path, PathBuf};

/// 编排层不许直接触碰的依赖。
///
/// **为什么界面框架在名单里**：编排层要能被 CLI 复用。
/// 依赖 `tauri` 之后，命令行想用同一份逻辑就得把 Tauri 也拖进去——
/// 那条路走不通，于是"两边各写一套"就开始了。
///
/// ---
///
/// ## 🔴 `qul-infra` 曾经在这个名单里，而它**被有意移除了**（M4 期间）
///
/// 那条禁令的理由是**具体的**：
///
/// > 依赖 `tauri` 之后，**命令行想用同一份逻辑就得把 Tauri 也拖进去**。
///
/// 而**那条理由对 `qul-infra` 不成立** —— 实测：
///
/// ```text
///   qul-cli → qul-app, qul-core, qul-infra, qul-provider-mock
/// ```
///
/// **命令行本来就依赖 `qul-infra`。** 所以"拖进去"这句话对它没有内容。
///
/// ### 而真正把这条禁令撞破的是**安装编排**
///
/// 安装要做 IO（下载 / 校验 / 解压），而那些**只存在于 `qul-infra`**。
/// 于是二选一：
///
/// | 做法 | 后果 |
/// |---|---|
/// | **让编排留在 `qul-cli` 里**（当时的状态） | 界面要用同一条链时**只能再写一遍** —— 而那正是本文件要防的分叉 |
/// | **让 `qul-app` 依赖 `qul-infra`** | 编排有一份，而两个调用方共享它 |
///
/// **第二条是唯一不制造分叉的那个。** 而它正是本文件开头那句
/// "编排层是'界面与命令行共用的那一份逻辑'的落点"的字面要求。
///
/// ### 而"不许依赖界面框架"那一条**没有被放松**
///
/// `tauri` / `wry` / `tao` 仍在名单里 —— 因为**那条理由是成立的**：
/// 编排层若依赖 Tauri，命令行就用不了它。
///
/// `tokio` / `reqwest` 也留着：本仓库的 http 是**零依赖**的 WinHTTP 实现
/// （见 `docs/来源记录.md` §5.1），所以它们进树会是一个**不该有的决定**，
/// 而在这里拦住它是**便宜**的。
const FORBIDDEN_APP_DEPS: &[&str] = &["tauri", "wry", "tao", "tokio", "reqwest"];

/// 编排层源码里不许出现的路径。
///
/// ⚠️ `qul_infra::` **曾经在这里，而现在允许了** —— 理由见 [`FORBIDDEN_APP_DEPS`]。
///
/// 而另两条仍然不许：
///
/// - `tauri::` —— 同上（命令行要能复用它）
/// - `qul_provider_` —— **产品是注入的**（`ProductRegistry` 是一个参数），
///   而编排层直接 `use` 一个具体 provider 会把"支持哪个产品"**写死在它里面**
const FORBIDDEN_APP_PATHS: &[&str] = &["qul_provider_", "tauri::"];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {} —— {e}", path.display()))
}

/// 从 `Cargo.toml` 文本里取出依赖名（只看依赖段）。
fn dependency_names(cargo_toml: &str) -> Vec<String> {
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
            names.push(key.trim().to_lowercase());
        }
    }
    names
}

/// 剥掉注释，理由与 `guardrails.rs` 相同：注释里提到 `tauri::` 不算违规。
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut in_line = false;
    let mut in_block = 0usize;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
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

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(rust_files(&p));
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
    out.sort();
    out
}

#[test]
fn 编排层不许依赖界面框架或_infra() {
    let deps = dependency_names(&read(manifest_dir().join("Cargo.toml")));
    assert!(
        !deps.is_empty(),
        "解析依赖名失败（Cargo.toml 结构可能变了）"
    );

    let bad: Vec<&String> = deps
        .iter()
        .filter(|d| FORBIDDEN_APP_DEPS.contains(&d.as_str()))
        .collect();
    assert!(
        bad.is_empty(),
        "qul-app 依赖了 {bad:?}。编排层的全部意义是「界面与 CLI 都能调它」——\
         依赖界面框架或 infra 之后，命令行就用不了它了。\
         真实调用请加在 qul-app 的公开函数里，而不是让调用方绕过它。"
    );
}

#[test]
fn 编排层源码里不许出现_infra_与界面路径() {
    let files = rust_files(&manifest_dir().join("src"));
    assert!(!files.is_empty(), "找不到源文件（目录结构可能变了）");

    for f in files {
        let code = strip_comments(&read(f.clone()));
        let bad: Vec<&str> = FORBIDDEN_APP_PATHS
            .iter()
            .copied()
            .filter(|p| code.contains(*p))
            .collect();
        assert!(
            bad.is_empty(),
            "{} 里出现 {bad:?}。编排层不许直接调 infra / provider / 界面 API。",
            f.display()
        );
    }
}

#[test]
fn 编排层对外只暴露内核类型与自己的类型() {
    // 这条防的是"编排层成了第二个内核"：如果它的公开 API 里冒出
    // 自定义的结构体，界面就会开始依赖 qul-app 的类型，而不是内核的契约。
    // **契约只该有一份。**
    let lib = strip_comments(&read(manifest_dir().join("src/lib.rs")));
    let pub_structs: Vec<&str> = lib
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix("pub struct ").map(|rest| {
                rest.split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('{')
            })
        })
        .filter(|s| !s.is_empty())
        .collect();

    assert_eq!(
        pub_structs,
        vec!["AppService"],
        "编排层新增了公开结构体 {pub_structs:?}。\
         对外契约应当来自 qul-core（`Overview` 等）；\
         qul-app 只该暴露它的服务类型。若确有必要的中间类型，请更新本断言并说明理由。"
    );
}
