//! # qul-cli —— 命令行入口
//!
//! ## 它为什么现在就要有
//!
//! 方案 §8 把 **M3 的验收点定在 CLI**：
//!
//! > **M3 · Java 版启动链路**：**CLI 能完成一次真实安装 → 部署 → 启动全链路**
//! > （此处才是"完整链路"的验证点）
//!
//! 也就是说**界面不是链路的第一现场，命令行才是**。
//! 那么"命令行与界面复用同一套逻辑"就不能等到 M3 才成立——
//! 一旦两边各自长起来，后面合不回。**所以 M0 就把这个骨架立住**（S4 的要求）。
//!
//! ## 子命令
//!
//! | 命令 | 作用 | 来源 |
//! |---|---|---|
//! | （无参数） | 打印能力概览 | M0 · S4 的链路样板 |
//! | `java` | 探测本机 Java，并按五个游戏版本场景给出选择 | M0 · S6 |
//!
//! **刻意手写参数解析**：现阶段引入 clap 只是为两条子命令，
//! 却要承担一条新依赖及其许可审查。等到子命令真的多起来再换。

mod launch_demo;
use qul_app::AppService;
use qul_core::java::{choose_java, missing_java_message, GameVersion};
use qul_core::{Capabilities, Capability, CapabilityKey};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("java") => run_java(args.iter().any(|a| a == "--json")),
        Some("launch-demo") => std::process::exit(launch_demo::run_launch_demo()),
        Some("--help") | Some("-h") => print_help(),
        None => run_overview(),
        Some(other) => {
            eprintln!("未知子命令：{other}");
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!("秦墨（qul）命令行");
    println!();
    println!("用法：");
    println!("  qul            打印能力概览");
    println!("  qul java       探测本机 Java，并按游戏版本给出选择");
    println!("  qul java --json  同上，输出 JSON（供自动化消费）");
    println!("  qul launch-demo  产品调用链演示（M1 出口条件；**不启动任何真实程序**）");
    println!("  qul --help     显示本帮助");
}

fn run_overview() {
    let capabilities = demo_capabilities();
    let service = AppService::with_capabilities(capabilities);
    let overview = service.capability_overview();

    println!("秦墨 · 能力概览");
    println!("{}", service.capability_summary_line());
    println!();

    println!("可用：");
    for key in &overview.enabled {
        println!("  + {key}");
    }

    if !overview.disabled.is_empty() {
        println!();
        println!("不可用：");
        for item in &overview.disabled {
            // **原因必须打出来。** 只说"不可用"而不说为什么，
            // 是方案 §5.7 明确禁止的（"结论必须带可执行建议"）。
            println!("  - {}   （{}）", item.key, item.reason);
        }
    }
}

/// S6 的五个场景。**用真实的正式版号**，不用编造的。
const SCENARIOS: [&str; 5] = ["1.8.9", "1.12.2", "1.16.5", "1.20.1", "1.21.4"];

fn run_java(as_json: bool) {
    let (discovery, failed) = qul_infra::java::discover_and_probe();

    if as_json {
        let payload = serde_json::json!({
            "candidates": discovery.candidates,
            "sources": discovery
                .sources
                .iter()
                .map(|s| serde_json::json!({ "kind": s.kind, "path": s.path }))
                .collect::<Vec<_>>(),
            "skipped": discovery
                .skipped
                .iter()
                .map(|(p, why)| serde_json::json!({ "path": p, "why": why }))
                .collect::<Vec<_>>(),
            "probe_failures": failed
                .iter()
                .map(|(p, why)| serde_json::json!({ "path": p, "why": why }))
                .collect::<Vec<_>>(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
        return;
    }

    println!("=== 发现的 Java 候选（按来源去重后逐条探测）===");
    if discovery.candidates.is_empty() {
        println!("  （无）");
    }
    for c in &discovery.candidates {
        println!(
            "  {:<10} {:<12} {:<4}位  {}  [{}]",
            format!("{}", c.major),
            c.version,
            c.bits,
            c.vendor,
            c.path
        );
    }

    println!();
    println!(
        "=== 探测失败（**必须显示**，否则「发现 N 个」可能其实是「N+1 个里有一个问不出来」）==="
    );
    if failed.is_empty() {
        println!("  （无）");
    }
    for (p, why) in &failed {
        println!("  {p}\n      {why}");
    }

    println!();
    println!("=== 五个版本场景的选择 ===");
    let mut correct = 0;
    for s in SCENARIOS {
        let Some(gv) = GameVersion::parse(s) else {
            println!("  {s:<8} 版本号解析失败（这本身是要告诉用户的事实）");
            continue;
        };
        let req = gv.requirement();
        let choice = choose_java(&discovery.candidates, req);
        match choice.selected() {
            Some(c) => {
                correct += 1;
                println!(
                    "  {s:<8} 需要 {:<14} -> Java {:<3} {} 位   ({})",
                    req.human(),
                    c.major,
                    c.bits,
                    c.path
                );
            }
            None => {
                println!("  {s:<8} 需要 {:<14} -> **未选出**", req.human());
                if let Some(msg) = missing_java_message(&choice) {
                    for line in msg.lines() {
                        println!("      {line}");
                    }
                }
            }
        }
    }
    println!();
    println!("场景满足率：{correct}/{}", SCENARIOS.len());
}

/// 演示用的能力表：**故意同时含启用项与禁用项**。
///
/// 为什么不让它全是启用的：那样就测不到"禁用项必须带原因"这条输出路径，
/// 而那条路径是**命令行最需要正确的一段**（用户看命令行就是为了知道为什么不行）。
fn demo_capabilities() -> Capabilities {
    let mut caps = Capabilities::new();

    // 通用能力：这一项可用
    caps.set(CapabilityKey::Launch, Capability::enabled());

    // 通用能力：这一项不可用，**必须给原因**
    let reason = "沙箱形态下内容包不可管理";
    match Capability::disabled(reason) {
        Ok(cap) => {
            caps.set(CapabilityKey::Mods, cap);
        }
        Err(_) => {
            // `Capability::disabled("")` 会被拒——空原因是不允许的。
            // 演示路径不该走到这里，所以直接说明而不是静默跳过。
            eprintln!("内部错误：禁用原因不该被拒（{reason}）");
            std::process::exit(2);
        }
    }

    // 详情项留空：M1 起由 provider 产出。
    // 概览会同时遍历通用表与详情表这件事，已由 `qul-core` 的单测覆盖，
    // 不必在命令行里再摆一个空壳来"证明"。
    caps
}
