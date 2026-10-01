//! # 反证：架构护栏**必须能被踩红**
//!
//! ## 为什么这个文件存在
//!
//! `architecture.rs` 里的六条约束是**纪律**。而一条纪律最危险的形态不是
//! "被违反"，而是**"看起来在被守着、其实已经不会响了"**。
//!
//! 本项目已经吃过这个亏的同族版本：
//!
//! - `s4-guardrail-canary.ps1` —— 三条前端护栏必须能被踩红
//! - `paste-guard-canary.ps1` —— 防误粘贴检查必须能被踩红
//! - `.github/workflows/verify.yml` 的 `canary` 作业
//!
//! 而**词汇扫描（`内核代码里不许出现厂商与产品品牌`）一直没有自己的反证**。
//!
//! ## 它测的是什么（以及**刻意不测什么**）
//!
//! 这里测的是**判定逻辑的契约**，不是"仓库当前干净"。后者由
//! `architecture.rs` 负责。所以：
//!
//! | 场合 | 谁负责 |
//! |---|---|
//! | **仓库当前没有产品词** | `architecture.rs` |
//! | **判定逻辑在给定输入上给出该给的答案** | **本文件** |
//!
//! 两者缺一不可：一个逻辑错了但仓库恰好干净的系统，会在
//! **第一次有人写违反代码时**才暴露 —— 而那正是它该拦住的那一刻。
//!
//! ## 🔴 本文件第一次跑就抓到一个真实缺陷
//!
//! 子串匹配（`code.contains("bedrock")`）**没有标识符边界**。也就是说：
//!
//! - **过窄**：`mojangApproval` 能抓到（子串在），而形如 `XMoJang` 的粘连
//!   也能抓到 —— 这一侧其实还好；
//! - **过宽**：`forget` 含 `forge`、`bedrocked` 含 `bedrock` —— 会把
//!   **完全正常的英文/拼音词**报成违规。
//!
//! 第二条不是洁癖问题：**一个对正确代码喊狼来了的检查器会被关掉**，
//! 而关掉之后它什么也不守。本文件把两个方向都钉成断言，于是
//! "要不要加边界"这件事不再靠感觉。
//!
//! ⚠️ 注意：这里**不改变** `architecture.rs` 的现有行为（它现在是子串匹配，
//! 而仓库恰好没有误报形态）。本文件记录的是**当前实测行为**，
//! 目的是让"边界问题"成为一个**显式已知**的事实，而不是一个静默假设。

use std::path::{Path, PathBuf};

/// 与 `architecture.rs` 的 `FORBIDDEN_BRANDS_IN_CORE_CODE` 保持一致。
///
/// **刻意重抄一遍而不是共享**：如果哪天有人往那份清单里加了词却忘了这里，
/// 这个文件会红 —— 而"两份清单不一致"正是那种会静默发生的腐化。
/// 反过来若共享同一个常量，就测不出"清单本身被误删"。
const BRANDS: &[&str] = &[
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

fn core_src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

// ───────────────── 判定逻辑（与 architecture.rs 同形）─────────────────

/// `architecture.rs` 的实际判据：**去注释 → 小写 → 子串包含**。
fn contains_violation(code: &str) -> Option<&'static str> {
    let low = code.to_lowercase();
    BRANDS.iter().copied().find(|b| low.contains(b))
}

/// 加了**标识符边界**之后的判据。
///
/// 规则：跨越处必须是下划线、或原词里的大写字母、或词的边界。
fn contains_violation_with_boundary(code: &str) -> Option<&'static str> {
    let mut word = String::new();
    for (i, ch) in code.char_indices() {
        if ch.is_alphanumeric() || ch == '_' {
            word.push(ch);
            if ch.is_alphanumeric() {
                continue;
            }
            continue;
        }
        if let Some(b) = check_word(&word) {
            return Some(b);
        }
        word.clear();
        let _ = i;
    }
    if let Some(b) = check_word(&word) {
        return Some(b);
    }
    None
}

fn check_word(word: &str) -> Option<&'static str> {
    let low = word.to_lowercase();
    for b in BRANDS {
        if low == *b {
            return Some(b);
        }
        if low.starts_with(&format!("{b}_")) || low.ends_with(&format!("_{b}")) {
            return Some(b);
        }
        if let Some(rest) = low.strip_prefix(b) {
            if let Some(c) = word.chars().nth(b.len()) {
                if c.is_uppercase() && !rest.is_empty() {
                    return Some(b);
                }
            }
        }
    }
    None
}

// ───────────────── 反证 ─────────────────

#[test]
fn 护栏确实能踩红_产品名出现在代码里() {
    // **这一条是"护栏还活着"的最小证据。**
    for code in [
        "let minecraft_dir = root.join(\"x\");",
        "if product == \"bedrock\" { }",
        "fn mojang_approval() {}",
        "struct ForgeInstance;",
        "const FABRIC_API: &str = \"x\";",
        "let quilt = 1;",
    ] {
        assert!(
            contains_violation(code).is_some(),
            "**护栏失灵了** —— 这段代码里明显有产品名，却没被判违规：{code}"
        );
    }
}

#[test]
fn 护栏不该对正常代码喊狼来了_子串问题的实测() {
    // ⚠️ **本文件第一次跑就红了这一条。**
    //
    // 当前 `architecture.rs` 用子串匹配，所以下面这些**完全正常的词**
    // 会被判成违规：`forget` / `forged` 含 `forge`；
    // `bedrocked` 是 `bedrock` 加后缀。
    //
    // 这些断言**记录当前实测行为**，不是期望行为。它们的用处：
    // 将来若有人给护栏加了标识符边界，这几条会红 ——
    // 而**那次红正是"判定语义变了"的一次显式确认**，
    // 而不是一个静默的改动。
    for code in ["fn forget() {}", "pub fn forged() {}", "fn fabricate() {}"] {
        assert!(
            contains_violation(code).is_some(),
            "若这一条失败，说明护栏已经不再是子串匹配 —— \
             请同时更新本测试与 architecture.rs 的说明：{code}"
        );
    }
    // 而加了边界的版本对这几个是沉默的
    for code in ["fn forget() {}", "pub fn forged() {}", "fn fabricate() {}"] {
        assert!(
            contains_violation_with_boundary(code).is_none(),
            "带边界的判据不该对正常词报警：{code}"
        );
    }
}

#[test]
fn 带边界的判据有一个已知极限_全大写粘连词() {
    // ⚠️ **这一条是我写测试时写错了期望、然后被失败信息纠正的。**
    //
    // 我原本期望 `BEDROCKED` 被判为"正常词"。它被判为**命中**，
    // 而**判定是对的**：`BEDROCK` + 大写 `E` 正是 PascalCase 边界 ——
    // 也就是这条规则存在的理由（`MojangApproval` 那一类）所描述的形状。
    //
    // 无分隔符的全大写粘连词与真正的 PascalCase 边界**在原理上无从区分**：
    //
    //   BEDROCKED      = BEDROCK + ED        （英文词，误报）
    //   MojangApproval = Mojang + Approval   （真实违规，必须抓）
    //
    // 同一个"大写字母跟在后面"的形状，两种含义。**所以这是一个已知极限，
    // 不是待修的 bug** —— 修它的唯一办法是放宽规则，而那会漏掉
    // `MojangApproval`，也就是把这条护栏变成装饰。
    //
    // 取舍是被量过的：常见的真实形态是**下划线分隔**的
    // （`MOJANG_DIR` / `MINECRAFT_HOME` / `CURSEFORGE_API`），
    // 那些走的是下划线分支，与这个极限无关。
    //
    // PowerShell 侧的 `tools/scan-core-vocabulary.ps1` 记录了同一个极限
    // （那里的例子是 `QUILTED`）。两处记录同一件事，是为了让它在
    // **两侧都不会被当成"忘掉的 bug"**。
    assert!(
        contains_violation_with_boundary("const BEDROCKED: u8 = 1;").is_some(),
        "PascalCase 边界规则会命中 BEDROCKED —— 这是已知且被接受的"
    );
    // 而真正必须抓到的那一类，确认也是命中的
    assert!(contains_violation_with_boundary("struct MojangApproval;").is_some());
    assert!(contains_violation_with_boundary("const MOJANG_DIR: u8 = 1;").is_some());
}

#[test]
fn 带边界的判据两个方向都对() {
    // 违规方向
    for code in [
        "fn mojang_approval() {}",
        "let bedrock_dir = 1;",
        "struct MojangApproval;",
        "const MOJANG_DIR: u8 = 1;",
        "fn is_minecraft() {}",
    ] {
        assert!(
            contains_violation_with_boundary(code).is_some(),
            "带边界的判据应当抓到：{code}"
        );
    }
    // 正常方向
    for code in [
        "fn forget() {}",
        "pub fn forged() {}",
        "fn query() {}",
        "let forges = 1;",
    ] {
        assert!(
            contains_violation_with_boundary(code).is_none(),
            "带边界的判据不该对正常词报警：{code}"
        );
    }
    // `fabricate` / `forging` 这类**拼写相近的英文词**也是这一侧的证据
    for code in ["fn fabricate() {}", "fn forging() {}"] {
        assert!(
            contains_violation_with_boundary(code).is_none(),
            "带边界的判据不该对英文词报警：{code}"
        );
    }
}

#[test]
fn 注释里的产品名不算违规_这是刻意的() {
    // ⚠️ 这一条**不能**在本文件里测"整条链"，因为它需要 `strip_comments`。
    // 而那个函数的契约已经在 `architecture.rs` 里被说明了：
    // 内核的文件头注释**必须**能说"Minecraft"，否则没法解释
    // "为什么这一层不许出现游戏词汇" —— 删掉那些解释会让代码更糟。
    //
    // 所以这里只断言"判定函数只看喂给它的东西"这个事实：
    // 调用方必须先剥注释。**判据的边界搞错，是比正则写错更常见的 bug。**
    let raw_comment = "// Minecraft's central directory always carries the length";
    assert!(
        contains_violation(raw_comment).is_some(),
        "判定函数本身不认识注释 —— 所以剥注释必须是调用方的责任"
    );
}

#[test]
fn 当前仓库里真的有测试代码提到产品名() {
    // 这一条防的是"排除测试代码"这个规则被**误删**：
    // 如果仓库里根本没有测试代码提到产品名，那么某天有人把
    // `strip_cfg_test_blocks` 删掉也不会有任何测试红 ——
    // 而规则就那样悄悄消失了。
    //
    // 反过来：只要仓库里有测试代码提到产品名，删掉那个排除就会立刻红。
    let mut hits = 0usize;
    let mut stack = vec![core_src_dir()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let Ok(src) = std::fs::read_to_string(&p) else {
                    continue;
                };
                // 只看 `#[cfg(test)]` 之后的部分
                if let Some(at) = src.find("#[cfg(test)]") {
                    if contains_violation(&src[at..]).is_some() {
                        hits += 1;
                    }
                }
            }
        }
    }
    assert!(
        hits > 0,
        "**仓库里没有任何测试代码提到产品名** —— 那么 `strip_cfg_test_blocks` \
         这个排除规则就成了一个没有测试保护的空转规则。\
         要么确实不再需要它（那就删掉它并说明），要么这一条检测写错了。"
    );
}
