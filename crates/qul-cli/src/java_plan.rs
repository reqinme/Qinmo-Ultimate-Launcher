//! # Java 计划的取证（M2 · Java 探测与选择）
//!
//! ## 它回答的是一个**具体**问题
//!
//! > **为了启动版本 X，这台机器上该用哪个 Java？为什么是它？**
//!
//! 而"为什么"必须能被**看见** —— 这正是 S6 要消灭的那类失败：
//! 用户看不懂的失败，第一种就是"它选了一个我没装的 Java"。
//!
//! ## 数据从哪来（**刻意离线优先**）
//!
//! | 数据 | 来源 |
//! |---|---|
//! | 版本清单 | 官方启动器的缓存 `version_manifest_v2.json` |
//! | 版本详情 | 本机 `versions/<id>/<id>.json`；**没有就报错，不偷偷联网** |
//! | 本机 Java | `qul_infra::java::discover_and_probe()` |
//!
//! **为什么不在这个命令里联网**：它是**取证**工具，而不是安装器。
//! 一个"顺手下载"的取证命令会让"我看到的结果"与"当时的数据"对不上。
//! 缺数据时它**明确说缺什么**，而那比一个静默的网络回退有用。
//!
//! ## 它输出什么（四段）
//!
//! ```text
//! ① 需求：版本 X 需要哪个 Java，以及那个结论是**事实**还是**假设**
//! ② 候选：本机探测到的每一个 Java，含**来源**与**它不满足时的原因**
//! ③ 选择：选中哪个 + 理由
//! ④ 缺口：探测时被跳过的路径与原因（S6 要求"无漏报"）
//! ```
//!
//! 第四段是最容易被省掉的一段，而它恰恰是"漏报查不出来"的根源。

use qul_core::descriptor::Descriptor;
use qul_core::java::{choose_java, JavaChoice};

/// 读官方启动器的缓存清单与某个版本详情。
fn read_metadata(
    version: Option<&str>,
) -> Result<(String, String, String, String, String), String> {
    let mc = std::path::Path::new(&std::env::var("APPDATA").unwrap_or_default()).join(".minecraft");
    let versions = mc.join("versions");
    let manifest_path = versions.join("version_manifest_v2.json");
    if !manifest_path.is_file() {
        return Err(format!(
            "找不到版本清单：{}\n  它由官方启动器写下来。没跑过官方启动器的话，先跑一次。",
            manifest_path.display()
        ));
    }
    let manifest_text =
        std::fs::read_to_string(&manifest_path).map_err(|e| format!("读不到清单：{e}"))?;

    let manifest = qul_core::descriptor::VersionManifest::parse(&manifest_text)
        .map_err(|e| format!("清单解析失败：{e}"))?;

    // 没指定就用清单里的最新正式版 —— **从指针读，不是数组第一条**。
    let id = match version {
        Some(v) => v.to_string(),
        None => manifest
            .latest_release()
            .ok_or_else(|| {
                format!(
                    "清单里的「最新正式版」指向 `{}`，而那个 id 不在清单里 —— \
                     这本身是一个值得报出来的不一致",
                    manifest.latest.release
                )
            })?
            .id
            .clone(),
    };

    let entry = manifest.find(&id).ok_or_else(|| {
        let mut near: Vec<&str> = manifest
            .versions
            .iter()
            .filter(|v| v.id.starts_with(&id))
            .map(|v| v.id.as_str())
            .take(5)
            .collect();
        near.sort();
        if near.is_empty() {
            format!("清单里没有版本 `{id}`")
        } else {
            format!("清单里没有版本 `{id}`。相近的：{}", near.join(", "))
        }
    })?;

    let detail_path = versions.join(&id).join(format!("{id}.json"));
    if !detail_path.is_file() {
        return Err(format!(
            "本机没有版本 `{id}` 的详情：{}\n  \
             这个命令**不联网** —— 它是取证工具，不是安装器。\n  \
             要拿到详情，用官方启动器启动一次那个版本，或先做 M2 的下载路径。",
            detail_path.display()
        ));
    }
    let detail = std::fs::read_to_string(&detail_path).map_err(|e| format!("读不到详情：{e}"))?;

    Ok((
        id,
        manifest_text,
        detail,
        entry.kind.clone(),
        entry.sha1.clone(),
    ))
}

pub fn run_java_plan(version: Option<&str>) -> i32 {
    println!("=== Java 计划取证（M2 · Java 探测与选择）===");
    println!();

    // ── 元数据 ──
    let (id, _manifest_text, detail, kind, declared_sha1) = match read_metadata(version) {
        Ok(x) => x,
        Err(e) => {
            println!("✗ {e}");
            return 2;
        }
    };
    let d = match Descriptor::parse(&detail) {
        Ok(d) => d,
        Err(e) => {
            println!("✗ 版本详情解析失败（{id}）：{e}");
            return 2;
        }
    };

    // ── ① 需求：事实还是假设 ──
    println!("【1】需求：这个版本要哪个 Java");
    let req = d.java_requirement();
    println!("  版本      : {id}");
    println!("  类型      : {kind}");
    // **把清单声明的 sha1 报出来**，于是"我看的是哪份数据"可被独立复核。
    println!("  详情 sha1 : {declared_sha1}  （官方清单声明的值）");
    println!("  最低主版本: {}", req.requirement.min_major());
    println!(
        "  来源      : {} ({})",
        req.source.as_str(),
        req.source.explain()
    );
    match req.declared_major {
        Some(n) => println!("  清单声明  : majorVersion={n}  ← **这是事实**"),
        None => println!("  清单声明  : （没有）  ← **所以下面是推断，不是事实**"),
    }
    if let Some(j) = d.java_version.as_ref() {
        if let Some(c) = j.component.as_ref() {
            println!("  运行时组件: {c}  （官方自己的运行时名字，我们不用它，但要知道）");
        }
    }
    println!();

    // ── ② 候选：探测本机 ──
    println!("【2】候选：本机探测到的 Java");
    let (discovery, failed) = qul_infra::java::discover_and_probe();
    if discovery.candidates.is_empty() {
        println!("  （一个都没有）");
        if failed.is_empty() {
            println!("  原因：没有任何来源给出路径。**这不是「没装 Java」的证据** ——");
            println!("        它只说明我们没有找到入口（见第 4 段的跳过记录）。");
        }
    }
    for c in &discovery.candidates {
        let stub = qul_core::java::is_launcher_stub(&c.path);
        let ok = c.major >= req.requirement.min_major();
        println!(
            "  {} Java {:<3} {}位  {:<18} {}",
            if ok { "✓" } else { "✗" },
            c.major,
            c.bits,
            c.vendor,
            c.version
        );
        println!(
            "      {}{}",
            c.path,
            if stub {
                "   ← **启动器存根**（不是运行时）"
            } else {
                ""
            }
        );
    }
    println!();

    // ── ③ 选择 ──
    println!("【3】选择");
    let choice = choose_java(&discovery.candidates, req.requirement);
    match &choice {
        JavaChoice::Selected { candidate, reason } => {
            println!("  ✓ {}", candidate.path);
            println!(
                "    Java {} · {} 位 · {}",
                candidate.major, candidate.bits, candidate.version
            );
            println!("    理由：{reason}");
        }
        JavaChoice::NoJavaAtAll { requirement } => {
            println!(
                "  ✗ 本机没有任何 Java 候选，而该版本需要 {}",
                requirement.human()
            );
        }
        JavaChoice::NoneSatisfies {
            requirement,
            available,
        } => {
            println!(
                "  ✗ 本机有 {} 个候选，但没有一个满足 {}",
                available.len(),
                requirement.human()
            );
            let mut majors: Vec<u32> = available.iter().map(|c| c.major).collect();
            majors.sort_unstable();
            majors.dedup();
            println!("    本机可用的主版本：{majors:?}");
            println!("    → 需要装一个 {}+ 的 JDK", requirement.min_major());
        }
    }
    // 用户可读的一句话（界面会用它）
    if let Some(msg) = qul_core::java::missing_java_message(&choice) {
        println!();
        println!("  给用户的话：{msg}");
    }
    println!();

    // ── ④ 缺口：S6 要求"无漏报" ──
    println!("【4】探测缺口：**被跳过/失败的来源与原因**");
    if failed.is_empty() && discovery.skipped.is_empty() {
        println!("  （没有跳过的来源）");
    }
    for (path, why) in &failed {
        println!("  ✗ 探测失败 {path}\n      {why}");
    }
    for (path, why) in &discovery.skipped {
        println!("  – 已跳过   {path}\n      {why}");
    }
    println!();
    println!(
        "  来源合计 {} 个：{} 个可用 / {} 个探测失败 / {} 个跳过",
        discovery.sources.len(),
        discovery.candidates.len(),
        failed.len(),
        discovery.skipped.len()
    );

    // ── 最后一条纪律提醒 ──
    println!();
    println!("⚠️ 「一个都没找到」与「没装 Java」是两件事。");
    println!("   前者我们有证据（第 4 段的来源清单），后者我们没有 ——");
    println!("   所以上面任何一句话都不许说成「你没装 Java」。");

    match choice {
        JavaChoice::Selected { .. } => 0,
        _ => 1, // 没选出 Java 时**非零退出** —— 让脚本能据此判断
    }
}

/// 「假设清单」的取证（**与选 Java 分开，因为它们是两件事**）。
pub fn run_assumptions(version: Option<&str>) -> i32 {
    let (id, _m, detail, _k, _s) = match read_metadata(version) {
        Ok(x) => x,
        Err(e) => {
            println!("✗ {e}");
            return 2;
        }
    };
    let d = match Descriptor::parse(&detail) {
        Ok(d) => d,
        Err(e) => {
            println!("✗ 版本详情解析失败（{id}）：{e}");
            return 2;
        }
    };
    println!("=== 我们替调用方做了哪些假设（{id}）===");
    println!();
    let a = d.assumptions();
    if a.is_empty() {
        println!("  （这个版本的元数据是完整的，不需要任何假设）");
    }
    for x in &a {
        println!("  字段 : {}", x.field);
        println!("  假设 : {}", x.assumed);
        println!("  理由 : {}", x.why);
        println!();
    }
    // 参数形态也要报 —— 它是这一层最重要的形态判定
    match d.argument_form() {
        Ok(f) => println!("  参数形态：{}", f.which()),
        Err(e) => println!("  ✗ 参数形态：{e}"),
    }
    println!(
        "  参数来源：{}",
        if d.java_requirement().source.is_declared() {
            "清单声明（事实）"
        } else {
            "版本号表推断（假设）"
        }
    );
    0
}
