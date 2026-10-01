//! # 语言包的**只读校验**（M1 · 把"漏翻"变成 CI 里会红的事实）
//!
//! ## 这个文件在防什么
//!
//! 一个"缺键就显示空白"的实现会让漏翻的文案**在界面上完全看不出来** ——
//! 而它通常是这样被发现的：**用户截图说你这里空了一块**。
//!
//! `qul_core::i18n` 已经把"缺键必须可见"做成规则（标注 + 统计）。
//! 而本文件把那条规则**接到真实的语言包文件上** ——
//! 于是"这一版漏了 3 个键"成为一个**在这里会红的事实**。
//!
//! ## 它与 `qul_core::i18n` 的单测分工
//!
//! | | 单测 | 本文件 |
//! |---|---|---|
//! | 问的问题 | "这套规则在给定输入下对吗" | **"仓库里那些真实的包合法吗、一致吗"** |
//! | 输入 | 现场构造的包 | **`locales/*.json`** |
//!
//! 两者都要有：规则对了但文件写错，是**两种不同的失败**。
//!
//! ## 为什么它读 `locales/` 而不是内嵌进二进制
//!
//! 因为方案要求**语言是数据**（"主题 / 图标 / 语言做成数据……零代码风险"）。
//! 内嵌会让"改一个错别字"变成"改代码 + 重编译" —— 而那正好否定了那个要求。
//!
//! ⚠️ **但它也带来一个必须记住的后果**：语言包文件在发布时**必须被一起带上**。
//! 有一条测试确认"至少有一份语言包"，所以"文件丢了"会在这里红 ——
//! 而不是等到用户看到一屏 `⟦app.name⟧`。

use qul_core::i18n::{LangPack, SuiteReport};
use std::path::PathBuf;

/// 语言包目录。
fn locales_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("locales")
}

/// 读出目录下全部 `*.json` 语言包（**按文件名排序，让结果确定**）。
fn load_all() -> Vec<(String, LangPack)> {
    let dir = locales_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("读不到 {}：{e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    paths.sort();

    let mut out = Vec::new();
    for p in paths {
        let text =
            std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}：{e}", p.display()));
        let file = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let pack = LangPack::from_json(&text)
            .unwrap_or_else(|e| panic!("{file} 不是合法的语言包 JSON：{e}"));
        out.push((file, pack));
    }
    out
}

#[test]
fn 至少有一份语言包() {
    // **"文件丢了"要在这里红** —— 而不是等到用户看到一屏 `⟦app.name⟧`。
    let packs = load_all();
    assert!(
        !packs.is_empty(),
        "{} 下没有任何 .json 语言包 —— 发布时必须把它们一起带上",
        locales_dir().display()
    );
}

#[test]
fn 每份语言包都合法() {
    // 一次报**全部**问题（不是第一个）—— 只报一个会把一次能说完的事变成 N 轮。
    let packs = load_all();
    let mut problems: Vec<String> = Vec::new();
    for (file, p) in &packs {
        for e in p.validate() {
            problems.push(format!("{file}：{e}"));
        }
    }
    assert!(
        problems.is_empty(),
        "语言包有问题：\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn 文件名与_locale_字段一致() {
    // 一个"文件名是 `zh-CN.json` 而 `locale` 写 `zh-TW`"的包
    // 会让按 locale 查表的代码找不到它 —— 而那种失败**只在换语言时才出现**。
    let packs = load_all();
    for (file, p) in &packs {
        let stem = file.trim_end_matches(".json");
        assert_eq!(
            stem, p.locale,
            "{file} 的文件名与它的 locale 字段不一致（按 locale 查表会找不到它）"
        );
    }
}

#[test]
fn 全部语言包的键一致() {
    // 这就是"漏翻"的判据。
    let packs: Vec<LangPack> = load_all().into_iter().map(|(_, p)| p).collect();
    let report = SuiteReport::check(&packs);
    assert!(report.is_clean(), "语言包不一致：\n{}", report.summary());
}

#[test]
fn 中文包里的键不许是中文() {
    // 这一条看起来与 `每份语言包都合法` 重复，而它**不重复**：
    // 它把"键必须是 ASCII"这条纪律**直接指向我们的第一语言** ——
    // 而那是最容易发生"图省事用中文当键"的地方。
    for (file, p) in load_all() {
        for k in p.entries.keys() {
            assert!(
                k.is_ascii(),
                "{file} 里的键 {k:?} 含非 ASCII —— 键会进日志与诊断包，必须是 ASCII"
            );
        }
    }
}

#[test]
fn 中文包覆盖了我们要求的最低键集() {
    // ⚠️ **这条测试的用意不是"完整性"**（键会随功能增长到几百个），
    // 而是**钉住那些"缺了就会让用户看到标注"的键**：
    // 错误提示、状态词、审批说明 —— 它们是**用户在最需要信息时看到的东西**。
    //
    // 一个"在错误路径上缺文案"的包，比"缺十个设置项标签"严重得多。
    const MUST_HAVE: &[&str] = &[
        "app.name",
        "state.loading",
        "state.failed",
        "state.cancelled",
        "error.title",
        "error.code",
        "error.suggestion",
        "approval.pending",
        "approval.no_progress_query",
        "java.not_found",
        "download.verifying",
        "migrate.failed",
        "migrate.backup_at",
        "migrate.restore_hint",
        "migrate.others_unaffected",
        "extract.skipped",
        "extract.reason",
    ];
    let packs = load_all();
    let zh = packs
        .iter()
        .find(|(_, p)| p.locale == "zh-CN")
        .expect("必须有一份 zh-CN");
    let missing: Vec<&str> = MUST_HAVE
        .iter()
        .copied()
        .filter(|k| !zh.1.entries.contains_key(*k))
        .collect();
    assert!(
        missing.is_empty(),
        "zh-CN 缺了这些**用户在最需要信息时会看到**的键：{missing:?}"
    );
}

#[test]
fn 占位符在全部语言包里成对出现() {
    // 一个"中文里有 `{code}` 而英文里没有"的包会让**同一条错误在两种语言下
    // 展示不同的信息量** —— 而更坏的形态是占位符写成了 `{code` 或 `{cod}`：
    // 那时它**不会被替换**，用户看到的是字面的 `{cod}`。
    for (file, p) in load_all() {
        for (k, v) in &p.entries {
            let opens = v.matches('{').count();
            let closes = v.matches('}').count();
            assert_eq!(
                opens, closes,
                "{file} 的 {k} 里花括号不成对（{opens} 个 {{ 对 {closes} 个 }}）：{v}"
            );
        }
    }
}

#[test]
fn 各语言包的占位符集合一致() {
    // `zh-CN` 的 `error.code` 用了 `{code}`，那么 `en-US` 的也必须用 `{code}`——
    // 否则两种语言下**同一条错误的可读性不同**，而用户会把那当成 bug。
    let packs = load_all();
    let Some((_, base)) = packs.first() else {
        return;
    };
    let placeholders = |s: &str| {
        let mut v: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut in_ph = false;
        for c in s.chars() {
            match c {
                '{' => {
                    in_ph = true;
                    cur.clear();
                }
                '}' if in_ph => {
                    in_ph = false;
                    v.push(cur.clone());
                }
                _ if in_ph => cur.push(c),
                _ => {}
            }
        }
        v.sort();
        v
    };
    for (file, p) in packs.iter().skip(1) {
        for (k, base_text) in &base.entries {
            let Some(other) = p.entries.get(k) else {
                continue;
            };
            assert_eq!(
                placeholders(base_text),
                placeholders(other),
                "{file} 的 {k} 占位符与 {} 的不一致\n  基准：{base_text}\n  实际：{other}",
                base.locale
            );
        }
    }
}
