//! # `qul-cli` 的**产品调用链演示**（M1 出口条件）
//!
//! ## 这个文件存在的唯一理由
//!
//! M1 的出口条件是 **"`MockProvider` 走通 CLI 调用链"**。
//! 而"走通"必须能被**复现和检查** —— 所以它是一条真子命令，
//! 而不是测试里的一段代码。
//!
//! ## 它刻意演示四条路径，而不只是一条"成功"
//!
//! | 演示 | 为什么必须有 |
//! |---|---|
//! | **注册 → 列出** | 证明"新增一个产品"这一步在 CLI 里是通的 |
//! | **规划 → 解析 → 预览命令** | 证明"参数对不对"**不必启动进程**就能看见 |
//! | **缺事实 → 报错** | 报错若只报"未知失败"，用户无法归因 |
//! | **取消 → 不启动** | 取消是用户的意愿，不是故障；**它必须与失败可区分** |
//!
//! **只演示成功路径的链路验证等于没验证** —— 因为链路上真正会坏的是错误路径。
//!
//! ## 为什么它不启动任何真实程序
//!
//! 用的是 `RecordingExecutor`（只记录）。三个理由：
//! ① 假产品**不该触发真实副作用**；
//! ② 真的启动进程是 `qul-infra` 的事，**M1 还没到那一步**；
//! ③ 出口条件要验的是**调用链通不通**，而不是游戏能不能跑。

use qul_app::AppService;
use qul_core::provider::{Facts, ProductRegistry};
use qul_core::retry::CancelToken;
use qul_provider_mock::{MockProvider, RecordingExecutor};

/// 演示用的假"本机事实"。
///
/// **刻意是两个键而不是一个**：只有一个键的话，
/// "缺一个事实"与"缺全部事实"的表现一样，而它们该说不同的话。
fn demo_facts() -> Facts {
    Facts::new()
        .set(MockProvider::runtime_key(), MockProvider::FAKE_RUNTIME)
        .set(MockProvider::memory_key(), "4096")
}

/// 假产品的产品码。
///
/// **走 trait 方法而不是自己另存一个常量**：产品码的定义处只能有一处，
/// 否则两处会漂（而漂了之后的表现是"CLI 说 A、界面说 B"）。
fn product_key() -> &'static str {
    use qul_core::provider::ProductIdentity;
    MockProvider.key()
}
/// 建注册表并注册假产品。
///
/// **这一步是"新增一个产品要动几处"的第一次真实测量**：
/// 在这里只多了一行 `register(Box::new(MockProvider))`。
/// 若哪天加真实产品需要改内核或改编排层的签名，**说明骨架形式不对**。
fn registry_with_mock() -> Result<ProductRegistry, String> {
    let mut r = ProductRegistry::new();
    r.register(Box::new(MockProvider))?;
    Ok(r)
}

pub fn run_launch_demo() -> i32 {
    println!("=== 产品调用链演示（M1 出口条件：MockProvider 走通 CLI 调用链）===");
    println!();
    println!("⚠️ 本命令**不启动任何真实程序** —— 执行器只记录收到的命令。");
    println!("   它验的是「注册 → 规划 → 解析 → 执行」这条链路通不通，");
    println!("   而不是任何产品能不能跑。");
    println!();

    let registry = match registry_with_mock() {
        Ok(r) => r,
        Err(e) => {
            println!("✗ 注册失败：{e}");
            return 2;
        }
    };
    let service = AppService::new();

    // ── ① 注册 → 列出 ──
    println!("【1】注册表里有哪些产品");
    for (key, name, variants) in service.products(&registry) {
        println!("  {key}  「{name}」");
        for v in variants {
            let tag = if v.needs_store_license {
                "（需要商店授权）"
            } else {
                ""
            };
            println!("      - {} 「{}」{tag}", v.key, v.label);
        }
    }
    println!();

    // ── ② 规划 → 解析 → 预览命令 ──
    println!("【2】规划 → 解析 → 预览命令（**不启动进程**）");
    let facts = demo_facts();
    match service.plan_command(&registry, product_key(), "release", &facts) {
        Ok(cmd) => {
            println!("  程序：{}", cmd.program);
            println!("  参数：{}", cmd.args.join(" "));
            if cmd.env.is_empty() {
                println!("  环境：无增量");
            } else {
                for (k, v) in &cmd.env {
                    println!("  环境：{k}={v}");
                }
            }
            match &cmd.cwd {
                Some(d) => println!("  工作目录：{d}"),
                None => println!("  工作目录：由调用方决定"),
            }
        }
        Err(e) => {
            println!("  ✗ 规划失败：[{}] {}", e.code.as_str(), e.reason);
            println!("     建议：{}", e.suggestion);
            return 2;
        }
    }
    println!();

    // ── ③ 缺事实 → 报错（一次报全）──
    println!("【3】缺本机事实时会怎样（**必须报全，不许只报第一个**）");
    let empty = Facts::new();
    match service.plan_command(&registry, product_key(), "release", &empty) {
        Ok(_) => {
            println!("  ✗ 不该成功 —— 缺了全部事实却产出了命令，这是个缺陷");
            return 2;
        }
        Err(e) => {
            println!("  [{}] {}", e.code.as_str(), e.reason);
            println!("  建议：{}", e.suggestion);
            let both = e.reason.contains("runtime.path") && e.reason.contains("memory.mb");
            println!(
                "  两个缺失都被报出：{}",
                if both {
                    "✓ 是"
                } else {
                    "✗ 否（这是缺陷）"
                }
            );
            if !both {
                return 2;
            }
        }
    }
    println!();

    // ── ④ 取消 → 不启动，且**不是失败** ──
    println!("【4】取消与失败必须可区分");
    let ex = RecordingExecutor::new();
    let cancel = CancelToken::new();
    cancel.cancel();
    match service.launch(&registry, product_key(), "release", &facts, &ex, &cancel) {
        Ok(None) => {
            println!(
                "  取消 → Ok(None)（**不是错误**），执行器收到 {} 条命令",
                ex.count()
            );
            if ex.count() != 0 {
                println!("  ✗ 取消后仍产生了命令，这是个缺陷");
                return 2;
            }
        }
        Ok(Some(code)) => {
            println!("  ✗ 已被取消却执行了，退出码 {code} —— 这是个缺陷");
            return 2;
        }
        Err(e) => {
            println!("  ✗ 取消被当成了失败：[{}] {}", e.code.as_str(), e.reason);
            return 2;
        }
    }
    println!();

    // ── ⑤ 正常启动 → 回收退出码 ──
    println!("【5】正常启动 → 回收退出码");
    let ex2 = RecordingExecutor::new();
    let c2 = CancelToken::new();
    match service.launch(&registry, product_key(), "release", &facts, &ex2, &c2) {
        Ok(Some(code)) => {
            println!("  退出码：{code}   执行器收到 {} 条命令", ex2.count());
            if let Some(cmd) = ex2.last() {
                println!(
                    "  实际交给执行器的命令：{} {}",
                    cmd.program,
                    cmd.args.join(" ")
                );
            }
            // **最后一步断言：执行器收到的命令里不许残留占位符。**
            // 这一条是整条链路的意义所在 —— 一个没解析的 `{RUNTIME}`
            // 会被操作系统当成路径去打开，而报错会说"找不到文件"。
            if let Some(cmd) = ex2.last() {
                let residue: Vec<&String> = cmd
                    .args
                    .iter()
                    .chain(std::iter::once(&cmd.program))
                    .filter(|s| s.contains('{') || s.contains('}'))
                    .collect();
                if !residue.is_empty() {
                    println!("  ✗ 命令里残留占位符：{residue:?} —— 这是个缺陷");
                    return 2;
                }
                println!("  命令里无占位符残留：✓ 是");
            }
        }
        Ok(None) => {
            println!("  ✗ 未被取消却返回了 None");
            return 2;
        }
        Err(e) => {
            println!("  ✗ 启动失败：[{}] {}", e.code.as_str(), e.reason);
            return 2;
        }
    }
    println!();
    println!("✓ 五条路径全部走通：注册 → 列出 → 规划 → 解析 → 缺事实报错 → 取消 → 启动 → 回收");
    0
}
