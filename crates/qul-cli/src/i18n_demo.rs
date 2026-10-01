//! # 语言包取证（M1 · i18n 框架）
//!
//! ## 它为什么值得是一条真子命令
//!
//! 因为 i18n 的正确性有两半，而**它们各自会被不同的东西破坏**：
//!
//! | 半 | 会被什么破坏 | 怎么验 |
//! |---|---|---|
//! | **包本身合法** | 有人手写 JSON 时把键写成中文、或加了重复键 | `cargo test -p qul-core --test locales`（8 项） |
//! | **运行时行为对** | 有人"优化"了回退逻辑，于是缺键变成空白 | **这条命令**（它打印出缺键时的**实际显示**） |
//!
//! **第二半是这条命令存在的理由**：它把"缺键时用户会看到什么"
//! 变成一个**可以直接看的东西**，而不是一句"我们实现了标注"。
//!
//! ## 它读的是**真实的语言包文件**
//!
//! 不是我在这里造一份 —— 那样验的是"我能构造一份合法的包"，
//! 而不是"仓库里那份包能用"。

use qul_core::i18n::{LangPack, SuiteReport, Translator};

/// 找语言包目录（**开发时在源码树里，发布时在资源目录里**）。
fn locales_dir() -> Option<std::path::PathBuf> {
    // ① 环境变量（打包/便携模式用）
    if let Some(p) = std::env::var_os("QUL_LOCALES_DIR") {
        let p = std::path::PathBuf::from(p);
        if p.is_dir() {
            return Some(p);
        }
    }
    // ② 源码树（开发时）
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = here.parent()?.join("qul-core").join("locales");
    if src.is_dir() {
        return Some(src);
    }
    // ③ 可执行文件旁边（发布形态）
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("locales");
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

fn load_all() -> Vec<(String, LangPack)> {
    let Some(dir) = locales_dir() else {
        return Vec::new();
    };
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        if p.extension().map(|x| x == "json").unwrap_or(false) {
            let file = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            match std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|t| LangPack::from_json(&t).map_err(|e| e.to_string()))
            {
                Ok(pack) => out.push((file, pack)),
                Err(e) => {
                    println!("  ✗ {file} 解析失败：{e}");
                    return Vec::new();
                }
            }
        }
    }
    out
}

pub fn run_i18n_demo() -> i32 {
    println!("=== 语言包取证（M1 · i18n 框架）===");
    println!();

    let Some(dir) = locales_dir() else {
        println!("✗ 找不到语言包目录。");
        println!("  开发时它在 crates/qul-core/locales/；发布时必须把它一起带上。");
        println!("  也可以设 QUL_LOCALES_DIR 指定。");
        return 2;
    };
    println!("【1】语言包目录");
    println!("  {}", dir.display());
    let packs = load_all();
    if packs.is_empty() {
        println!("  ✗ 里面没有任何 .json");
        return 2;
    }
    for (file, p) in &packs {
        println!("  {file}  「{}」  {} 个键", p.name, p.entries.len());
    }
    println!();

    // ── ② 合法性 + 键一致性（CI 的判据）──
    println!("【2】合法性 + 键一致性（这就是 CI 里会红的那两条）");
    let plain: Vec<LangPack> = packs.iter().map(|(_, p)| p.clone()).collect();
    let report = SuiteReport::check(&plain);
    for line in report.summary().lines() {
        println!("  {line}");
    }
    if !report.is_clean() {
        println!();
        println!("✗ 语言包不干净 —— **这不是可以忽略的告警**：");
        println!("  它会表现为界面上出现 ⟦键名⟧ 而不是文案。");
        return 2;
    }
    println!("  ✓ 干净");
    println!();

    // ── ③ 键的形态（把第一语言单独点出来）──
    println!("【3】键的形态：**必须是 ASCII 且带分组**");
    println!("  理由：键会进日志、进诊断包、进测试断言 —— 而含中文的键");
    println!("  在那些地方会遭遇编码问题（本项目已经踩过一次）。");
    let zh = packs
        .iter()
        .find(|(_, p)| p.locale == "zh-CN")
        .map(|(_, p)| p.clone())
        .unwrap_or_else(|| packs[0].1.clone());
    let mut bad_keys: Vec<&String> = Vec::new();
    let mut groups: std::collections::BTreeMap<String, usize> = Default::default();
    for k in zh.entries.keys() {
        if !qul_core::i18n::is_valid_key(k) {
            bad_keys.push(k);
        }
        *groups
            .entry(k.split('.').next().unwrap_or("(无)").to_string())
            .or_insert(0) += 1;
    }
    println!(
        "  分组分布（{} 组 / {} 个键）：",
        groups.len(),
        zh.entries.len()
    );
    for (g, n) in &groups {
        println!("    {g:<12} {n}");
    }
    if !bad_keys.is_empty() {
        println!("  ✗ 非法键：{bad_keys:?}");
        return 2;
    }
    println!("  ✓ 全部合法");
    println!();

    // ── ④ 缺键时的**实际显示**（这条命令的核心）──
    println!("【4】缺键时用户会看到什么（**这一条必须肉眼可见**）");
    let mut t = Translator::single(zh.clone());
    let missing_key = "this.key.does.not.exist";
    let shown = t.t(missing_key);
    println!("  查一个不存在的键：{missing_key}");
    println!("  显示为：{shown}");
    println!("  ⚠️ 若上面是**空白**，那说明有人把标注逻辑「优化」掉了 ——");
    println!("     而那种优化的表现是：**漏翻的文案在界面上完全看不出来**，");
    println!("     直到用户截图说你这里空了一块。");
    if shown.trim().is_empty() || !shown.contains(missing_key) {
        println!("  ✗ 缺键没有被可见地标注");
        return 2;
    }
    println!("  ✓ 可见且能看出是哪个键");
    println!();

    // ── ⑤ 参数替换与"缺参数时原样保留" ──
    println!("【5】参数替换");
    for (key, args) in [
        ("error.code", vec![("code", "QUL-JAVA-0001")]),
        (
            "migrate.backup_at",
            vec![("path", r"%DATA%\backups\20261001")],
        ),
        ("progress.file", vec![("done", "12"), ("total", "40")]),
    ] {
        if zh.entries.contains_key(key) {
            println!("  {key} → {}", t.tp(key, &args));
        }
    }
    // 缺参数
    if zh.entries.contains_key("error.code") {
        let got = t.tp("error.code", &[]);
        println!("  error.code（**不给参数**）→ {got}");
        println!("    说明：缺参数时占位符**原样保留** —— 与启动计划刻意相反。");
        println!("    启动计划缺占位符要**失败**（一个没替换的 {{JAVA}} 会被当成路径），");
        println!("    而文案缺参数时用户看到 {{code}} 就知道是文案没配上，");
        println!("    而「显示 0」会让用户以为真的是 0。");
        if !got.contains("{code}") {
            println!("  ✗ 缺参数时没有原样保留");
            return 2;
        }
    }
    println!();

    // ── ⑥ 缺失统计（CI 与诊断包都用它）──
    println!("【6】缺失统计：**让「这一版漏了几个键」成为可断言的事实**");
    // ⚠️ **这一步必须清零，而第一版漏了它。**
    //
    // 第 4 步查过一个不存在的键（`this.key.does.not.exist`），
    // 所以它已经在统计里了 —— 于是在这里断言 `len() != 2` 会红，
    // 而**红的原因不是统计错了，是我忘了那个键**。
    //
    // 这条注释留着，因为它是"统计是累积的"这个事实的证据：
    // 换语言时必须清零，否则"英文包缺的键"会与"中文包缺的键"混在一起。
    t.reset_missing();
    let _ = t.t("hot.missing.key");
    let _ = t.t("hot.missing.key");
    let _ = t.t("cold.missing.key");
    println!("  {}", t.missing_summary());
    let m = t.missing();
    if m.len() != 2 {
        println!("  ✗ 统计不对：{m:?}");
        return 2;
    }
    // **次数降序**：一个被查 500 次的缺键（主界面标题）
    // 与只查 1 次的（错误分支）优先级完全不同。
    if m[0].0 != "hot.missing.key" || m[0].1 != 2 {
        println!("  ✗ 次数降序不对：{m:?}");
        return 2;
    }
    println!("  ✓ 按次数降序（高频缺键排在前面）");
    println!();

    // ── ⑦ 回退语言：**不会静默变成别的语言** ──
    println!("【7】回退语言的行为");
    let act = LangPack::new("zh-CN", "简体中文").set("app.name", "秦墨");
    let fb = zh.clone();
    let mut t2 = Translator::with_fallback(act, fb);
    println!("  当前包只有 app.name；回退包是完整的 zh-CN");
    println!("  app.name            → {}（来自当前包）", t2.t("app.name"));
    let r = t2.t_full("state.loading");
    println!(
        "  state.loading       → {}（来自回退：{}）",
        r.text, r.from_fallback
    );
    println!("  ⚠️ 但它**被记账了**：{}", t2.missing_summary());
    println!("     因为「当前语言缺了这个键」是要修的，哪怕回退能显示出来。");
    if !r.from_fallback || t2.missing_count() != 1 {
        println!("  ✗ 回退行为不对");
        return 2;
    }
    println!();

    println!("✓ i18n：包合法 · 键一致 · 缺键可见 · 参数正确 · 统计可用 · 回退不静默");
    0
}
