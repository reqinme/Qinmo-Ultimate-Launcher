//! # 实例通用模型的取证（M1 最后一项 · 方案 §8）
//!
//! ## 它要让人看见的是什么
//!
//! `实例是什么` 这个问题有**一个正确答法和一个看起来也对的答法**：
//!
//! | | 做法 | 它在哪里崩 |
//! |---|---|---|
//! | ❌ 看起来也对 | "实例 = 一个 `.minecraft` 目录的包装" | 基岩版没有 `versions/`、没有 `.jar`；UWP 形态的目录由系统管 |
//! | ✅ 正确 | **实例 = 身份 + 目录 + 状态**，产品维度靠 `product_key` 外挂 | 不崩 —— 因为**通用层里没有产品假设** |
//!
//! 所以这条命令的核心是**把"通用"这件事变成可见的**：
//! 打印同一个模型装下 Java 版 / 基岩版 / 与 MC 无关的产品，
//! 并让人**亲眼看到记录里没有任何产品专属的字段**。
//!
//! ## 它还演示两件"设计选择"的回报
//!
//! 1. **ID 是 `key` 的函数** ⇒ 能回答"这条记录还对得上那个目录吗"；
//! 2. **`key` 与 `name` 分开** ⇒ 改名不动身份、不动目录。

use qul_core::instance::{Instance, InstanceId, InstanceState, ProductRef};

/// 只在内存里演示（**不落盘** —— 落盘往返由
/// `crates/qul-infra/tests/instance_record.rs` 的 8 项测试负责）。
pub fn run_instance_demo() -> i32 {
    println!("=== 实例通用模型取证（M1 · 方案 §8）===");
    println!();

    // ── ① 同一个模型装下三种产品 ──
    println!("【1】同一个模型装下三种产品（**这是「通用」的实质检验**）");
    let cases: Vec<(&str, ProductRef, &str)> = vec![
        (
            "java-main",
            ProductRef::new("java", "26.3", "vanilla"),
            "Java 版（有变体概念）",
        ),
        (
            "bedrock-main",
            ProductRef::new("bedrock", "1.21.90", ""),
            "基岩版（**没有**变体概念）",
        ),
        (
            "other-1",
            ProductRef::new("some-other-product", "2026.1", ""),
            "**与 MC 无关的产品**",
        ),
    ];
    for (key, product, note) in &cases {
        match Instance::create(*key, *key, product.clone(), 3) {
            Ok(i) => {
                println!("  ✓ {:<14} {:<28} {}", i.key, i.product.display(), note);
                println!("      id={}  dir={}", i.id, i.dir.as_str());
            }
            Err(e) => {
                println!("  ✗ {key}：{e}");
                return 2;
            }
        }
    }
    println!("  ⚠️ 注意上面三个的字段**完全一样**：");
    println!("     id / key / name / dir / product / state / data_version");
    println!("     —— 没有一个字段提到 java / jar / versions。");
    println!();

    // ── ② 记录里没有产品专属的词 ──
    println!("【2】通用层里**不许有产品假设**（逐个字段检查）");
    let bedrock = Instance::create(
        "bedrock-main",
        "基岩版",
        ProductRef::new("bedrock", "1.21.90", ""),
        3,
    )
    .unwrap();
    let json = serde_json::to_string(&bedrock).unwrap();
    println!("  {}", json);
    let lower = json.to_lowercase();
    let forbidden = ["jar", "versions/", "libraries", "assets", "manifest"];
    let mut hit: Vec<&str> = Vec::new();
    for w in forbidden {
        if lower.contains(w) {
            hit.push(w);
        }
        println!(
            "  {} {:<12} {}",
            if lower.contains(w) { "✗" } else { "✓" },
            w,
            if lower.contains(w) {
                "**出现了 —— 产品假设渗进了通用层**"
            } else {
                "没有"
            }
        );
    }
    if !hit.is_empty() {
        println!("  ✗ 通用模型里出现了 {hit:?}");
        return 2;
    }
    println!("  ✓ 全都没有");
    println!();

    // ── ③ ID 是 key 的函数（**这是可校验性的来源**）──
    println!("【3】ID 是 key 的**函数**，不是随机数");
    println!("  理由：随机 ID 无法回答「这条记录还对得上那个目录吗」。");
    for k in ["alpha", "alpha", "beta"] {
        println!("  from_key({k:<6}) = {}", InstanceId::from_key(k));
    }
    println!("  ✓ 同一个 key 永远同一个 ID（上面两行 alpha 完全一样）");
    println!();

    // ── ④ 改坏记录能被抓住 ──
    println!("【4】记录被改坏时能**说清**是什么坏了");
    let mut bad =
        Instance::create("alpha", "测试", ProductRef::new("java", "26.3", ""), 3).unwrap();
    bad.id = InstanceId::from_key("beta"); // 模拟"陈旧或被改过"
    match bad.check_integrity() {
        Ok(()) => {
            println!("  ✗ 该发现 ID 与 key 不符，但没有");
            return 2;
        }
        Err(errs) => {
            for e in &errs {
                println!("  ✓ {e}");
            }
        }
    }
    let mut bad2 =
        Instance::create("alpha", "测试", ProductRef::new("java", "26.3", ""), 3).unwrap();
    bad2.dir = qul_core::layout::RelPath::new("instances/手工改过的目录").unwrap();
    if let Err(errs) = bad2.check_integrity() {
        for e in &errs {
            println!("  ✓ {e}");
        }
    } else {
        println!("  ✗ 该发现 dir 与 key 不符");
        return 2;
    }
    println!();

    // ── ⑤ 改名不动身份 ──
    println!("【5】改名**不动** key、ID、目录（身份与标签分开）");
    let mut i = Instance::create(
        "alpha",
        "原来的名字",
        ProductRef::new("java", "26.3", ""),
        3,
    )
    .unwrap();
    let before = (
        i.id.as_str().to_string(),
        i.key.clone(),
        i.dir.as_str().to_string(),
    );
    println!("  改名前：name={}  key={}  id={}", i.name, i.key, i.id);
    if let Err(e) = i.rename("一个全新的名字") {
        println!("  ✗ 改名失败：{e}");
        return 2;
    }
    let after = (
        i.id.as_str().to_string(),
        i.key.clone(),
        i.dir.as_str().to_string(),
    );
    println!("  改名后：name={}  key={}  id={}", i.name, i.key, i.id);
    if before != after {
        println!("  ✗ 改名动了身份：{before:?} → {after:?}");
        return 2;
    }
    println!("  ✓ 变了（name）与没变（key/id/dir）分得很清楚");
    println!("     —— 一个把显示名当目录名的实现，在这里必须移动目录（可能撞车、可能失败）。");
    println!();

    // ── ⑥ 运行态不落盘 ──
    println!("【6】运行态**不是**持久状态");
    for s in InstanceState::ALL {
        println!(
            "  {:<9} 持久={:<5} 可启动={}",
            s.key(),
            s.is_persistent(),
            s.can_launch()
        );
    }
    if InstanceState::Running.is_persistent() {
        println!("  ✗ Running 不该是持久状态");
        return 2;
    }
    println!("  理由：一个把 Running 写进磁盘的实现，会在**上次异常退出后**");
    println!("  让界面显示「正在运行」—— 而那个状态**永远无法自愈**。");
    println!();

    // ── ⑦ 排序是确定的（且不承诺语言学正确）──
    println!("【7】排序：**确定**，但不承诺语言学正确");
    let mut v = vec![
        Instance::create("a", "甲", ProductRef::new("java", "1", ""), 3).unwrap(),
        Instance::create("b", "乙", ProductRef::new("java", "1", ""), 3).unwrap(),
        Instance::create("c", "Alpha", ProductRef::new("java", "1", ""), 3).unwrap(),
    ];
    Instance::sort_stable(&mut v);
    let names: Vec<&str> = v.iter().map(|i| i.name.as_str()).collect();
    println!("  排出来：{names:?}");
    println!("  ⚠️ 中文名是按 **UTF-8 字节序**（乙 < 甲），**不是拼音序**。");
    println!("     我在写测试时正是被它绊了一下 —— 我以为甲会在乙前面。");
    println!("     一期不做 collator 的理由：它需要按语言而定的排序表（几百 KB），");
    println!("     而「实例列表怎么排」目前不是产品决策。");
    println!("     重要的是它**确定** —— 界面每次刷新顺序都一样。");
    if names != vec!["Alpha", "乙", "甲"] {
        println!("  ✗ 排序行为变了（若这是有意的，请同时更新这条命令与那条测试）");
        return 2;
    }
    println!();

    println!("✓ 实例通用模型：一个模型装三种产品 · 无产品假设 · ID 可校验 · 改名不动身份");
    0
}
