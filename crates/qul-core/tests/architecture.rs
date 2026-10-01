//! # 架构约束测试
//!
//! 这些测试**不测功能，测"不许长歪"**。
//!
//! 它们存在的理由：方案里有很多"纪律"，而**纪律靠自觉一定会腐化**。
//! 凡是能写成断言的就写成断言——这样违反的人是"测试挂了"，
//! 而不是"我没注意到那条规定"。
//!
//! 本文件对应方案 §3.3 变更流程第 5 步与 §1.4 的红线。

use std::fs;
use std::path::{Path, PathBuf};

/// 集成测试是**独立的 crate**，必须用库名路径引用，不能用 `crate::`。
use qul_core::CapabilityKey;

/// `qul-core` 的源码根目录。
fn core_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// 递归收集 `.rs` 文件。
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// 去掉行注释与块注释，只留代码。
///
/// **为什么必须去注释**：内核的文件头注释里**必须**能说"Minecraft"
/// （否则没法解释"为什么这一层不许出现游戏词汇"）。
/// 扫注释会把这些解释文字当成违规，测试就变成了逼人删文档。
/// 所以规则是：**代码里不许有游戏词汇，注释里随便说。**
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_block = false;
    for line in src.lines() {
        let mut cur = line;
        if in_block {
            match cur.find("*/") {
                Some(i) => {
                    in_block = false;
                    cur = &cur[i + 2..];
                }
                None => continue,
            }
        }
        // 逐段处理本行的块注释
        loop {
            let open = cur.find("/*");
            let line_comment = cur.find("//");
            match (open, line_comment) {
                // 行注释在块注释之前 → 本行代码到此为止
                (Some(o), Some(l)) if l < o => {
                    out.push_str(&cur[..l]);
                    out.push('\n');
                    break;
                }
                (None, Some(l)) => {
                    out.push_str(&cur[..l]);
                    out.push('\n');
                    break;
                }
                (Some(o), _) => {
                    out.push_str(&cur[..o]);
                    match cur[o..].find("*/") {
                        Some(c) => cur = &cur[o + c + 2..],
                        None => {
                            in_block = true;
                            out.push('\n');
                            break;
                        }
                    }
                }
                (None, None) => {
                    out.push_str(cur);
                    out.push('\n');
                    break;
                }
            }
        }
    }
    out
}

/// 内核里**不许出现**的**厂商 / 品牌 / 具体产品名**。
///
/// **这份表经过一次修正，值得记下为什么。**
///
/// 初版还把 `shader` / `resource_pack` / `mods` 也列进来，理由是"内核不许认识
/// 具体游戏"。实测后这条**判断错了**：`CapabilityKey::Shaders` 描述的是
/// **"能不能管理某一类用户内容"**——任何支持模组化内容的游戏都会有这类
/// **通用内容类别**，它不是某个游戏的专属概念。禁掉它等于逼内核放弃表达力。
///
/// **修正后的规则**：禁**品牌与厂商**（下面这些），不禁**通用内容类别**。
/// 真正该禁的"游戏专属"是**路径名**——见 [`PATH_FRAGMENT_MARKERS`]。
const FORBIDDEN_BRANDS_IN_CORE_CODE: &[&str] = &[
    "minecraft",
    "mojang",
    "curseforge",
    "modrinth",
    "forge",
    "fabric",
    "neoforge",
    "quilt",
    "optifine",
    "bedrock",
];

/// 内核里不许出现的**文件系统路径片段**。
///
/// **这才是"内核不认识具体游戏"的真正落点**：内核可以知道"存在一类内容"，
/// 但**不许知道它放在哪个目录**——一旦内核里出现目录名，换个游戏就得改内核，
/// 那正是要避免的腐化。
///
/// 只列**明确指向"某类内容的目录"**的组合，不列 `dir` / `path` / `parents`
/// 这类通用词，否则会误报（`RelPath` 的实现里就有通用的路径处理）。
const PATH_FRAGMENT_MARKERS: &[&str] = &[
    "mods_dir",
    "mod_dir",
    "shaders_dir",
    "shader_dir",
    "resourcepacks_dir",
    "resource_packs_dir",
    "datapacks_dir",
    "saves_dir",
    "screenshots_dir",
    "configs_dir",
    "instances_dir",
    "versions_dir",
    "libraries_dir",
    "assets_dir",
];

/// 去掉 `#[cfg(test)]` 块——**只留生产代码**。
///
/// **为什么必须排除测试代码**：
/// 扫描生产代码是为了守"**内核不许按产品分支**"。
/// 而**测试里写 `let mut bedrock = ...` 是完全正当的**——
/// 测试必须在代码里提到产品，否则它就没法验证"切到基岩版时行为包出现"。
///
/// 这条也是踩出来的：初版扫了 `src/` 全部内容，于是测试里的
/// `let mut bedrock` 被当成"内核代码出现产品名"而报警。
/// **判据应当是"产品名出现在生产代码的代码/字符串里"，不是"文件里出现过这个词"。**
///
/// 实现用花括号配对跳过整个 `#[cfg(test)]` 项，而不是"从该行到文件末尾"——
/// 后者会在有人把测试模块写在文件中间时**静默漏掉后面的生产代码**。
fn strip_cfg_test_blocks(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0usize;

    while i < src.len() {
        // 找下一个 `#[cfg(test)]`
        match src[i..].find("#[cfg(test)]") {
            None => {
                out.push_str(&src[i..]);
                break;
            }
            Some(off) => {
                let attr_at = i + off;
                out.push_str(&src[i..attr_at]);

                // 从属性往前找它修饰的那个项的 `{`
                let mut j = attr_at;
                let mut depth_seen = false;
                while j < src.len() {
                    match bytes[j] {
                        b'{' => {
                            depth_seen = true;
                            break;
                        }
                        // 遇到 `;` 说明是 `#[cfg(test)] mod x;` 这类声明，没有块
                        b';' => break,
                        _ => j += 1,
                    }
                }
                if !depth_seen {
                    // 没有块：跳过属性本身即可
                    i = attr_at + "#[cfg(test)]".len();
                    continue;
                }

                // 花括号配对，跳过整个块
                let mut depth = 0i32;
                let mut k = j;
                while k < src.len() {
                    match bytes[k] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                k += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    k += 1;
                }
                i = k;
            }
        }
    }
    out
}

#[test]
fn 内核代码里不许出现厂商与产品品牌() {
    // ⚠️ **这条规则修正过两次，两次的理由都必须留着。**
    //
    // **修正一：注释不算。**
    // 加入基岩版能力（`BehaviorPacks` / `SkinPacks`）后它报警
    // `caps.rs: 代码中出现 bedrock` —— 因为我们**必须在文档注释里解释
    // "为什么行为包是产品专属的"**，而那必然要提到基岩版。
    // 注释**不进二进制**，没有技术后果；真正有后果的是
    // **代码/标识符/字符串**（`bedrock_dir`、`if product == "bedrock"`、
    // 能力 key 拼错）。而 `strip_comments` 恰好保留这两者。
    // 另一条独立证据：`BehaviorPacks` 这个**枚举名本身已经是中性的**
    // （不叫 `BedrockBehaviorPacks`）。该禁的是"标识符带产品名"。
    //
    // **修正二：测试代码不算。**
    // 修完注释后它仍报警，这次命中的是测试里的 `let mut bedrock = ...`。
    // 而**测试必须在代码里提到产品**，否则没法验证"切到基岩版时行为包出现"。
    // → **只扫生产代码**（排除 `#[cfg(test)]` 块）。
    //
    // 两条修正指向同一个判据：
    // **"产品名出现在生产代码的代码/字符串里"才算违规**，
    // 而不是"某个文件里出现过这个词"。
    let mut violations = Vec::new();

    for file in rust_files(&core_src_dir()) {
        let src = fs::read_to_string(&file).expect("源码应为 UTF-8");
        let production = strip_cfg_test_blocks(&src);
        let code = strip_comments(&production).to_lowercase();
        for bad in FORBIDDEN_BRANDS_IN_CORE_CODE {
            if code.contains(bad) {
                violations.push(format!(
                    "{}: 生产代码/字符串中出现 `{}`（产品名只许出现在注释与测试里）",
                    file.display(),
                    bad
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "架构约束被违反：\n  {}\n\n\
         内核只放通用模型与规则。产品概念属于 qul-provider-*，\n\
         产品差异要靠**能力描述符的 `reason`** 表达，不是按产品名分支。\n\
         （注释与测试里提到产品是允许的——见本测试上方说明。）",
        violations.join("\n  ")
    );
}

#[test]
fn 内核代码里不许出现游戏数据目录名() {
    let mut violations = Vec::new();

    for file in rust_files(&core_src_dir()) {
        let src = fs::read_to_string(&file).expect("源码应为 UTF-8");
        let code = strip_comments(&src).to_lowercase();
        for bad in PATH_FRAGMENT_MARKERS {
            if code.contains(bad) {
                violations.push(format!(
                    "{}: 代码中出现 `{}`——内核不许知道某类内容放在哪个目录（那是 Provider 的事）",
                    file.display(),
                    bad
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "架构约束被违反：\n  {}\n\n\
         内核可以表达\"存在一类内容\"（如 CapabilityKey::Mods），\n\
         但不许表达\"它在哪个目录\"——否则换个游戏就得改内核。",
        violations.join("\n  ")
    );
}

/// 从 Cargo.toml 的依赖表里解析出**依赖名**。
///
/// **为什么要解析而不是扫全文**：本文件的初版对整个 Cargo.toml 做
/// `to_lowercase()` 再找关键词，结果被两处**无关文本**命中而误报：
///   - `description` 里的"零 Tauri"（文档本身就在声明这条纪律）
///   - `# 零 Tauri` 注释（Cargo.toml 用 `#` 注释，而 `strip_comments` 处理的是 `//`）
///
/// **两次误报指向同一个根因**：断言的对象是"**依赖**"，
/// 那就只该看依赖名，而不是整份文件的字面文本。
/// 精确化之后，这个测试既不会被文档措辞干扰，也真的能拦住新增依赖。
fn dependency_names() -> Vec<String> {
    let raw = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("应能读到 Cargo.toml");

    let mut names = Vec::new();
    let mut in_deps = false;

    for line in raw.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            // `[dependencies]` / `[dev-dependencies]` / `[build-dependencies]`
            in_deps = line.contains("dependencies");
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 依赖行形如 `serde = { ... }` 或 `serde.workspace = true`
        if let Some((key, _)) = line.split_once('=') {
            let name = key.trim().split('.').next().unwrap_or("").trim();
            if !name.is_empty() {
                names.push(name.to_lowercase());
            }
        }
    }
    names
}

#[test]
fn 内核不许依赖_io_或界面框架() {
    let deps = dependency_names();
    assert!(
        !deps.is_empty(),
        "解析依赖名失败（Cargo.toml 结构可能变了）"
    );

    for banned in ["tauri", "tokio", "reqwest", "hyper", "axum", "warp"] {
        assert!(
            !deps.iter().any(|d| d == banned),
            "qul-core 不应依赖 `{}`——IO/网络/界面属于 qul-infra 或更外层。\n\
             当前依赖表：{:?}",
            banned,
            deps
        );
    }
}

/// 从 `caps.rs` 里解析出 `CapabilityKey` 的所有变体名。
///
/// **为什么要解析源码而不是硬编码一份清单**：如果测试里也手写一份清单，
/// 那么"新增变体时忘记登记"这种错误**测试自己也会犯**（两边一起漏）。
/// 直接从 `enum CapabilityKey { ... }` 的正文里抠变体名，才能真的发现遗漏。
fn declared_capability_variants() -> Vec<String> {
    let src = fs::read_to_string(core_src_dir().join("caps.rs")).expect("应能读到 caps.rs");

    let start = src
        .find("pub enum CapabilityKey {")
        .expect("找不到 CapabilityKey 定义")
        + "pub enum CapabilityKey {".len();
    let end = src[start..].find("\n}").expect("CapabilityKey 定义未闭合") + start;
    let body = &src[start..end];

    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        // 变体形如 `    Mods,`
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        let name = line.trim_end_matches(',').trim();
        if !name.is_empty() && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            out.push(name.to_string());
        }
    }
    out
}

/// 从 `caps.rs` 的 `as_str` 里解析出 `Self::X => "y"` 的映射。
fn declared_as_str_map() -> Vec<(String, String)> {
    let src = fs::read_to_string(core_src_dir().join("caps.rs")).expect("应能读到 caps.rs");
    let start = src.find("pub const fn as_str").expect("找不到 as_str");

    // 取到 match 块的闭合 `}`：从 `match self {` 起做花括号配对。
    // **不用"找第一个 \n    }"这种脆弱做法**——它会随缩进变化而静默截断，
    // 而静默截断的测试比没有测试更糟（它让人以为覆盖到了）。
    let match_at = src[start..]
        .find("match self {")
        .expect("as_str 里应有 `match self {`")
        + start
        + "match self {".len();
    let bytes = src.as_bytes();
    let mut depth = 1usize;
    let mut end = match_at;
    for (i, b) in bytes[match_at..].iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = match_at + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &src[match_at..end];

    let mut out = Vec::new();
    for line in body.lines() {
        let Some(arrow) = line.find("=>") else {
            continue;
        };
        let left = line[..arrow].trim();
        let Some(variant) = left.strip_prefix("Self::") else {
            continue;
        };
        let right = line[arrow + 2..].trim();
        let Some(q1) = right.find('"') else { continue };
        let Some(q2) = right[q1 + 1..].find('"') else {
            continue;
        };
        out.push((
            variant.trim().to_string(),
            right[q1 + 1..q1 + 1 + q2].to_string(),
        ));
    }
    out
}

#[test]
fn 每个能力变体都必须登记进_列表() {
    let declared = declared_capability_variants();
    assert!(!declared.is_empty(), "解析 CapabilityKey 变体失败");

    // ALL 是一个静态数组，直接比对它的字符串形式即可
    let all_debug = format!("{:?}", CapabilityKey::ALL);
    let mut missing = Vec::new();
    for v in &declared {
        if !all_debug.contains(v.as_str()) {
            missing.push(v.clone());
        }
    }

    assert!(
        missing.is_empty(),
        "以下 CapabilityKey 变体未登记进 `ALL`：{:?}\n\
         未登记的 key 不会出现在界面遍历路径上——这正是\"能力注册混乱\"的开端。",
        missing
    );

    // 反向：ALL 里不许有重复
    let mut seen = std::collections::BTreeSet::new();
    for k in CapabilityKey::ALL {
        assert!(seen.insert(*k), "`ALL` 里出现重复项：{:?}", k);
    }
    assert_eq!(
        seen.len(),
        declared.len(),
        "`ALL` 有 {} 项，但 `CapabilityKey` 声明了 {} 个变体",
        seen.len(),
        declared.len()
    );
}

#[test]
fn 每个能力变体都必须有稳定字符串() {
    let declared = declared_capability_variants();
    let map = declared_as_str_map();

    let mapped: Vec<&String> = map.iter().map(|(v, _)| v).collect();
    let mut missing = Vec::new();
    for v in &declared {
        if !mapped.contains(&v) {
            missing.push(v.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "以下变体缺少 `as_str` 映射（会 panic 或落进错误的分支）：{:?}",
        missing
    );

    // 稳定字符串必须互不相同，且要么全用 snake_case
    let mut strings = std::collections::BTreeSet::new();
    for (variant, s) in &map {
        assert!(
            strings.insert(s.clone()),
            "`as_str` 出现重复值：`{}`（变体 {}）",
            s,
            variant
        );
        assert!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "稳定字符串应为 snake_case，但 `{}` 不是（变体 {}）",
            s,
            variant
        );
    }
}

#[test]
fn 文件名不许是中文或含空格() {
    // 仓库要跨平台、要进 CI。文件名带空格或非 ASCII 会在
    // 某些工具链上炸掉，而这类问题发现得越晚越贵。
    let mut bad = Vec::new();
    for file in rust_files(&core_src_dir()) {
        if let Some(name) = file.file_name().and_then(|s| s.to_str()) {
            if !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            {
                bad.push(file.display().to_string());
            }
        }
    }
    assert!(
        bad.is_empty(),
        "以下文件名不合规（应为 ASCII + 下划线）：{:#?}",
        bad
    );
}
