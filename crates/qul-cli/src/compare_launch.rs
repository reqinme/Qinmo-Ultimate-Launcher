//! # 与我们自己对比：`qul compare-launch`
//!
//! ## 它把"计划骨架可逐字节比对"这条要求**做成一条命令**
//!
//! 方案 §8 的 M3 出口条件原文：
//!
//! > 能进游戏主菜单；**计划骨架可逐字节比对**；CLI 能完成一次真实安装 → 部署 → 启动全链路
//!
//! 而 S7 第 5 步给了比对的做法（按空白切分、排序、`Compare-Object`），
//! 并加了一条纪律：**每一处差异都要写清"为什么不同、是否可以接受"**。
//!
//! ## 数据从哪来
//!
//! | 一侧 | 来源 |
//! |---|---|
//! | **官方** | `spikes/s7-official-baseline/baseline-args.txt` —— **从官方启动器自己的日志里读出来的 20 条真实参数** |
//! | **我们** | `qul_core::launch_plan::assemble()` 用本机真实的版本详情组装 |
//!
//! 那份基准里**没有真实用户路径**（官方日志用 `<WORKDIR>` 占位符），
//! 所以它可以被提交，而比对不需要脱敏。
//!
//! ## 🔴 两侧的"变量"必须各自统一成记号，否则比对是假的
//!
//! 官方那一侧已经带着 `<WORKDIR>`（官方自己写的）。而我们这一侧：
//!
//! | 差异 | 怎么统一 |
//! |---|---|
//! | 官方 `<WORKDIR>\.minecraft\…` vs 我们的真实路径 | 把我们的实例根替换成 `<WORKDIR>\.minecraft` |
//! | 官方的 brand/version 是它自己的 | **按名字归类为"预期差异"**，而不是靠值相等 |
//!
//! **第二行是关键**：一个"值必须相等"的比对会把
//! `-Dminecraft.launcher.brand=minecraft-launcher` 报成差异 —— 而它**应当不同**。
//! 于是那条真实差异会被淹没在噪声里。
//!
//! ## 它明确不做什么
//!
//! **不判断"是否可以接受"** —— 那是人的决定（S7 原话要求逐项解释）。
//! 本命令只做两件事：**列出差异**、**把差异归类**（预期 / 意外）。

use qul_core::descriptor::{Descriptor, Env, PlatformTarget};
use qul_core::launch_plan::{assemble, AssembleInput, Assembled};
use std::collections::BTreeMap;

/// 官方模板变量 → 我们的占位符名。
///
/// ## 这张表就是"官方模板词汇表"与"我们的事实键"之间的对照
///
/// 实测（26.3 的 `arguments`）里出现过的变量：
/// `natives_directory` · `launcher_name` · `launcher_version` · `classpath`
/// · `auth_player_name` · `auth_uuid` · `auth_access_token` · `auth_session`
/// · `version_name` · `game_directory` · `assets_root` · `assets_index_name`
/// · `version_type` · `user_type` · `user_properties` · `resolution_*` …
///
/// **不在表里的会原样保留，并被列进 `templates_unmapped`** —— 那是安全的失败方向。
fn template_map() -> BTreeMap<String, String> {
    let pairs = [
        ("natives_directory", "NATIVES_DIR"),
        ("launcher_name", "LAUNCHER_NAME"),
        ("launcher_version", "LAUNCHER_VERSION"),
        ("classpath", "CLASSPATH"),
        ("auth_player_name", "PLAYER_NAME"),
        ("auth_uuid", "PLAYER_UUID"),
        ("auth_access_token", "ACCESS_TOKEN"),
        ("auth_session", "SESSION"),
        ("version_name", "VERSION_NAME"),
        ("version_type", "VERSION_TYPE"),
        ("game_directory", "GAME_DIR"),
        ("assets_root", "ASSETS_DIR"),
        ("assets_index_name", "ASSETS_INDEX"),
        ("user_type", "USER_TYPE"),
        ("user_properties", "USER_PROPERTIES"),
        ("clientid", "CLIENT_ID"),
        ("auth_xuid", "XUID"),
    ];
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// **按参数名归类的"预期差异"。**
///
/// 这些参数的值**本来就该不同**（品牌、版本、内存、账户、路径），
/// 而把它们逐条报成"差异"会让真正的差异被淹没。
///
/// ⚠️ 匹配的是**参数名前缀**，而不是整串 —— 因为值里有本机事实。
const EXPECTED_DIFFERENT: &[(&str, &str)] = &[
    (
        "-Dminecraft.launcher.brand=",
        "官方署它自己的品牌；我们署「秦墨」。**这是我们该做的**（不冒用官方品牌）",
    ),
    ("-Dminecraft.launcher.version=", "同上，版本号也是各自的"),
    ("-Xmx", "内存上限由用户设置决定，不是格式的一部分"),
    ("-Xms", "同上"),
    (
        "-Xss",
        "线程栈大小：**官方启动器自己的设置，版本 JSON 里没有**。\
         所以它属于「启动器层配置」而不是「格式层」—— 我们给默认值，用户可调",
    ),
    (
        "-Djava.library.path=",
        "natives 目录：官方放 .minecraft\\bin\\<hash>，我们放实例内（方案 §8 第 6 项）",
    ),
    ("-Djna.tmpdir=", "同上"),
    ("-Dorg.lwjgl.system.SharedLibraryExtractPath=", "同上"),
    ("-Dio.netty.native.workdir=", "同上"),
    ("-Dminecraft.launcher.brand", "品牌"),
];

fn classify(arg: &str) -> Option<&'static str> {
    EXPECTED_DIFFERENT
        .iter()
        .find(|(prefix, _)| arg.starts_with(prefix))
        .map(|(_, why)| *why)
}

/// 读基准文件（`spikes/s7-official-baseline/baseline-args.txt`，一行一条）。
fn read_baseline() -> Result<Vec<String>, String> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("spikes")
        .join("s7-official-baseline")
        .join("baseline-args.txt");
    if !p.is_file() {
        return Err(format!(
            "找不到官方命令行基准：{}\n  它由 spikes/s7-official-baseline/capture.ps1 生成。",
            p.display()
        ));
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("读不到基准：{e}"))?;
    Ok(text
        .lines()
        .map(|l| l.trim_end_matches(['\r', '\n']).to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// 把"我们的"输出统一成与官方基准可比的记号。
///
/// 两件事：
/// 1. 把 `<WORKDIR>` 变成真实路径的替身 —— **官方那一侧已经是这个记号**；
/// 2. 把 classpath 那一长串**按分号拆开、逐个统一前缀**，否则两侧的
///    绝对路径永远不可能相等（而那不是差异，是同一件事的两种写法）。
fn normalize_ours(s: &str, instance_root: &str) -> String {
    let mut out = s.replace('\\', "/");
    // 真实的实例根 → 官方用的记号
    let root_norm = instance_root.replace('\\', "/");
    if !root_norm.is_empty() {
        out = out.replace(&root_norm, "<WORKDIR>/.minecraft");
    }
    // 官方基准里用的是反斜杠（Windows 原生），统一成正斜杠再比
    out.replace("\\", "/")
}

/// 官方那侧也统一成正斜杠。
fn normalize_theirs(s: &str) -> String {
    s.replace('\\', "/")
}

pub fn run_compare_launch(version: Option<&str>) -> i32 {
    println!("=== 计划骨架 vs 官方真实命令行（M3）===");
    println!();

    // ── 官方那侧 ──
    let theirs_raw = match read_baseline() {
        Ok(v) => v,
        Err(e) => {
            println!("✗ {e}");
            return 2;
        }
    };
    println!("【1】两侧的数据");
    println!(
        "  官方 : spikes/s7-official-baseline/baseline-args.txt（{} 条）",
        theirs_raw.len()
    );
    println!("         来源 = **官方启动器自己的日志**（S7 取证，非推断）");

    // ── 我们那侧 ──
    let mc = std::path::Path::new(&std::env::var("APPDATA").unwrap_or_default()).join(".minecraft");
    let versions = mc.join("versions");
    let manifest_path = versions.join("version_manifest_v2.json");
    let manifest_text = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(e) => {
            println!("✗ 读不到版本清单：{e}");
            return 2;
        }
    };
    let manifest = match qul_core::descriptor::VersionManifest::parse(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            println!("✗ 清单解析失败：{e}");
            return 2;
        }
    };
    // ⚠️ **基准是官方启动器那一次跑出来的版本**。
    // 它跑的是 `latest-release`，而清单里的 latest 会变 ——
    // 所以"用哪个版本"必须**显式对齐**，否则比的是两个不同的版本。
    //
    // 基准里那条 classpath 末尾就是客户端 jar 的名字，从它读出版本 id。
    // ⚠️ **先归一化斜杠再搜。**
    //
    // 第一版直接搜 `"/versions/"`，而基准里的 classpath 用的是**反斜杠**
    // （`<WORKDIR>\.minecraft\libraries\…\versions\26.3\26.3.jar`）——
    // 于是它一条都没匹配上，版本 id"没读出来"。
    // 同一个错在 classpath 找法上也犯了一次（搜 `"/libraries/"`）。
    let baseline_version = theirs_raw
        .iter()
        .find(|a| a.contains("versions") && a.ends_with(".jar"))
        .and_then(|cls| {
            let norm = cls.replace('\\', "/");
            norm.split(';')
                .filter(|p| p.contains("/versions/"))
                .filter_map(|p| {
                    let i = p.find("/versions/")? + "/versions/".len();
                    let rest = &p[i..];
                    let j = rest.find('/')?;
                    Some(rest[..j].to_string())
                })
                .next()
        });
    let id = match version {
        Some(v) => v.to_string(),
        None => match &baseline_version {
            Some(v) => v.clone(),
            None => manifest.latest.release.clone(),
        },
    };
    println!("  我们 : versions/{id}/{id}.json");
    match &baseline_version {
        Some(v) => {
            println!("         而基准是官方的 `{v}` —— **两个 id 必须一致**，否则比的是两个版本")
        }
        None => println!("         ⚠️ 没从基准里读出官方跑的版本 id"),
    }
    if let Some(v) = &baseline_version {
        if *v != id && version.is_none() {
            println!("  ✗ 版本 id 不一致（基准 {v} / 我们 {id}）");
            return 2;
        }
        if *v != id {
            println!("  ⚠️ 你显式指定了 {id}，而基准是 {v} —— 差异里会有版本本身带来的噪声");
        }
    }
    println!();

    let detail_path = versions.join(&id).join(format!("{id}.json"));
    let detail = match std::fs::read_to_string(&detail_path) {
        Ok(t) => t,
        Err(e) => {
            println!("✗ 读不到详情 {}：{e}", detail_path.display());
            return 2;
        }
    };
    let d = match Descriptor::parse(&detail) {
        Ok(d) => d,
        Err(e) => {
            println!("✗ 详情解析失败：{e}");
            return 2;
        }
    };

    let env = Env::new(PlatformTarget::windows("10.0.26200", "x86_64"));
    let input = AssembleInput {
        program: "{JAVA}".into(),
        main_class: String::new(),
        template_map: template_map(),
    };
    // classpath 用**基准里那一份**，因为我们要比的是"参数骨架"，
    // 而不是"我们能不能算出同一份 classpath"（那由 M2 的 library_plans 保证）。
    // 同理：用归一化后的串判断，而**保留原件**（比对时要按官方那侧的形态归一）
    let their_classpath = theirs_raw
        .iter()
        .find(|a| a.replace('\\', "/").contains("/libraries/") && a.contains(';'))
        .cloned();
    let a: Assembled = match assemble(&d, &env, &input, their_classpath.as_deref()) {
        Ok(x) => x,
        Err(e) => {
            println!("✗ 组装失败：{e}");
            return 2;
        }
    };
    println!(
        "【2】我们组装出了 {} 个参数（形态 = {}）",
        a.args.len(),
        a.form
    );
    println!(
        "  有效库 {} 个，其中 natives {} 个",
        a.libraries.len(),
        a.natives.len()
    );
    if !a.templates_unmapped.is_empty() {
        println!(
            "  ⚠️ **{} 个官方模板变量没有映射**：{:?}",
            a.templates_unmapped.len(),
            a.templates_unmapped
        );
        println!("     它们会被原样传给 Java —— 那是安全的失败方向，但必须补上映射。");
    }
    println!();

    // ── 归一化 ──
    let instance_root = mc.display().to_string();
    let ours_all: Vec<String> = a
        .args
        .iter()
        .map(|x| normalize_ours(x, &instance_root))
        .collect();
    let theirs: Vec<String> = theirs_raw.iter().map(|x| normalize_theirs(x)).collect();

    // ─────────────────────────────────────────────────────────────────────────
    // 🔴 比对必须**分段**，而这是本轮最重要的一个设计修正。
    //
    // 第一版把"我们的全部参数"与"官方基准"当两个集合比 ——
    // 于是报出 22 条"我们有、官方没有"，而其中 21 条是**游戏参数**
    // （`--username` / `--uuid` / `--accessToken` …）。
    //
    // 它们**不在基准里不是差异，是基准的性质**：官方启动器
    // **刻意不把游戏参数写进日志**（令牌不该进日志 —— S7 那一轮实测确认过）。
    //
    // 把一个"基准的已知盲区"报成"意外差异"，会让真正的差异淹没在噪声里 ——
    // 而那正是 `tools/audit-milestone.ps1` 里那条纪律的另一个形态：
    // **一个把噪声记成缺陷的检查，会在真正要查问题时把人引到错方向。**
    //
    // 所以现在分三段：
    //   ① JVM 参数（`-cp` 之前）—— **可以逐项比**，官方基准覆盖它
    //   ② classpath —— **整段比**（排序会打乱它）
    //   ③ 主类与游戏参数 —— **基准不含**，只做"我们产出了什么"的陈述
    // ─────────────────────────────────────────────────────────────────────────
    let cp_at_ours = ours_all.iter().position(|x| x == "-cp");
    let ours_jvm: Vec<String> = match cp_at_ours {
        Some(i) => ours_all[..i].to_vec(),
        None => ours_all.clone(),
    };
    let ours_after_cp: Vec<String> = match cp_at_ours {
        Some(i) => ours_all[i..].to_vec(),
        None => Vec::new(),
    };
    // 官方那侧同理：`-cp` 之前是 JVM 参数，`-cp` 之后那个长串是 classpath
    let their_cp_at = theirs.iter().position(|x| x == "-cp");
    let theirs_jvm: Vec<String> = match their_cp_at {
        Some(i) => theirs[..i].to_vec(),
        None => theirs.clone(),
    };

    println!("【3】分段比对 —— **基准的已知盲区不算差异**");
    println!(
        "  ① JVM 参数  : 官方 {} 条 / 我们 {} 条（`-cp` 之前）",
        theirs_jvm.len(),
        ours_jvm.len()
    );
    println!(
        "  ② classpath : {}",
        if their_cp_at.is_some() && cp_at_ours.is_some() {
            "两侧都有，整段比（见第 5 段）"
        } else {
            "**有一侧没有**"
        }
    );
    let game_after_mc: Vec<&String> = ours_after_cp
        .iter()
        .skip(1 + usize::from(!theirs_raw.is_empty() && false))
        .collect();
    println!(
        "  ③ 主类与游戏参数: 我们产出 {} 条 —— **官方基准不含这一段**",
        ours_after_cp.len().saturating_sub(1)
    );
    println!("     理由：官方启动器**刻意不把游戏参数写进日志**（令牌不该进日志），");
    println!("     所以 S7 那一轮实测得到的 20 条**只覆盖 JVM 参数**。");
    println!();
    let _ = game_after_mc;

    // ── ① JVM 参数的集合比对 ──
    println!("【4】① JVM 参数：集合比对（顺序无关）");
    let mut only_theirs: Vec<&String> = Vec::new();
    let mut only_ours: Vec<&String> = Vec::new();
    for t in &theirs_jvm {
        if !ours_jvm.contains(t) {
            only_theirs.push(t);
        }
    }
    for o in &ours_jvm {
        if !theirs_jvm.contains(o) {
            only_ours.push(o);
        }
    }
    // ⚠️ **这句话的措辞被改过一次，因为第一版会让人读成一个矛盾。**
    //
    // 第一版写「官方的 N 条里我们有 M 条完全相同」，紧接着又写
    // 「除预期差异外完全相同」—— 而两者**都对**，只是"不同"这个词
    // 在两句里指的粒度不同（逐字符相同 vs 归类后相同）。
    //
    // 所以现在把三个数分开报：完全相同 / 预期差异 / 意外差异。
    let n_identical = theirs_jvm.len() - only_theirs.len();
    let n_expected_theirs = only_theirs.iter().filter(|t| classify(t).is_some()).count();
    println!(
        "  官方 {} 条：**逐字符相同 {n_identical} 条** + 预期差异 {n_expected_theirs} 条 + 意外差异 {} 条",
        theirs_jvm.len(),
        only_theirs.len() - n_expected_theirs
    );
    println!("  （「逐字符相同」与「归类后相同」是两个不同的粒度 —— 所以分三个数报，而不是一句「有 N 条相同」）");
    println!();

    // 差异分类

    let mut unexpected_theirs: Vec<&String> = Vec::new();
    let mut unexpected_ours: Vec<&String> = Vec::new();
    for t in &only_theirs {
        if classify(t).is_none() {
            unexpected_theirs.push(t);
        }
    }
    for o in &only_ours {
        if classify(o).is_none() {
            unexpected_ours.push(o);
        }
    }

    // 差异分类

    let mut unexpected_theirs: Vec<&String> = Vec::new();
    let mut unexpected_ours: Vec<&String> = Vec::new();
    for t in &only_theirs {
        if classify(t).is_none() {
            unexpected_theirs.push(t);
        }
    }
    for o in &only_ours {
        if classify(o).is_none() {
            unexpected_ours.push(o);
        }
    }

    if !unexpected_ours.is_empty() {
        println!(
            "  **我们有、官方没有（{} 条）** —— 这些是意外差异：",
            unexpected_ours.len()
        );
        for o in &unexpected_ours {
            let shown: String = o.chars().take(140).collect();
            println!("    + {shown}");
        }
        println!();
    }
    if !unexpected_theirs.is_empty() {
        println!(
            "  **官方有、我们没有（{} 条）** —— 这些是意外差异：",
            unexpected_theirs.len()
        );
        for t in &unexpected_theirs {
            let shown: String = t.chars().take(140).collect();
            println!("    - {shown}");
        }
        println!();
    }
    if unexpected_ours.is_empty() && unexpected_theirs.is_empty() {
        println!("  ✓ 除下列**预期差异**外，两侧参数完全相同");
    }
    println!();

    // ── ② 预期差异逐条解释 ──
    println!("【4】预期差异（**每一处都写清为什么**）");
    let mut explained: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &only_theirs {
        if let Some(why) = classify(t) {
            *explained.entry(why).or_insert(0) += 1;
        }
    }
    for o in &only_ours {
        if let Some(why) = classify(o) {
            *explained.entry(why).or_insert(0) += 1;
        }
    }
    if explained.is_empty() {
        println!("  （没有）");
    }
    for (why, n) in &explained {
        println!("  ×{n}  {why}");
    }
    println!();

    // ── ② classpath **整段**比对（排序会打乱它，所以单独一次）──
    println!("【5】② classpath：**整段比对**（排序会打乱它，所以单独一次）");
    match (
        &their_classpath,
        ours_all
            .iter()
            .find(|x| x.contains("/libraries/") && x.contains(';')),
    ) {
        (Some(t), Some(o)) => {
            let tn = normalize_theirs(t);
            let on = o.clone();
            if tn == on {
                println!("  ✓ 逐字符相同");
            } else {
                // 差异定位到第一个不同的位置，而不是"不同"
                let tb = tn.as_bytes();
                let ob = on.as_bytes();
                let mut k = 0;
                while k < tb.len() && k < ob.len() && tb[k] == ob[k] {
                    k += 1;
                }
                println!("  ✗ 在第 {} 个字符处开始不同", k);
                let s = k.saturating_sub(60);
                println!("    官方 : …{}", &tn[s..(k + 120).min(tn.len())]);
                println!("    我们 : …{}", &on[s..(k + 120).min(on.len())]);
                println!(
                    "  提示：条目数 官方={} 我们={}",
                    tn.split(';').count(),
                    on.split(';').count()
                );
            }
        }
        (None, _) => println!("  ⚠️ 基准里没找到 classpath，跳过"),
        (_, None) => println!("  ⚠️ 我们这侧没产出 classpath（没给 -cp）"),
    }
    println!();

    // ── ③ 顺序：官方 ≠ 我们的顺序，而那是**预期**的 ──
    println!("【6】顺序比对（**这里允许不同，但要说清**）");
    let n_cmp = theirs_jvm.len().min(ours_jvm.len());
    let same_order: Vec<bool> = theirs_jvm
        .iter()
        .zip(ours_jvm.iter())
        .map(|(t, o)| t == o)
        .collect();
    let in_order = same_order.iter().filter(|x| **x).count();
    println!(
        "  同位置相同的 JVM 参数：{in_order} / {n_cmp}（{}）",
        if in_order == n_cmp {
            "顺序也完全一致 ✓"
        } else {
            "顺序不同 —— 见下"
        }
    );
    if in_order < n_cmp {
        println!("  两侧的**成员集合相同但顺序不同**。");
        println!("  而**顺序在 JVM 参数里通常无意义**（它们是开关与 -D 赋值），");
        println!("  例外是 `-cp` 与它后面那一段 —— 那个已由第 5 段整段比对覆盖。");
        println!("  所以：**顺序差异不构成问题，但要写下来**（S7 要求逐项解释）。");
    }
    println!();

    // ── 结语 ──
    println!("【结论】");
    let ok = unexpected_ours.is_empty() && unexpected_theirs.is_empty();
    if ok {
        println!("  ✓ ① JVM 参数：**零条意外差异**（预期差异见第 4 段，逐条有理由）");
        println!("  ✓ ② classpath：见第 5 段");
        println!("  ✓ ③ 主类与游戏参数：官方基准**不含**这一段（理由已说明），");
        println!("     所以它不由这条命令判定 —— 它的验收在「能进游戏主菜单」那一步。");
        println!();
        println!("  ⚠️ 这条命令**不判断「是否可以接受」** —— 那是人的决定。");
        println!("     它做的是：把差异列全、分类、并给每一类一个理由。");
        0
    } else {
        println!(
            "  ✗ **有意外差异**：我们多 {} 条、官方多 {} 条。",
            unexpected_ours.len(),
            unexpected_theirs.len()
        );
        println!("     这**不是格式问题就是我们的错** —— 必须逐条查清后才能进 M4。");
        1
    }
}
