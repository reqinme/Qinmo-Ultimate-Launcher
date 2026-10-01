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
//! ## 现在它只做一件事
//!
//! 打印能力概览。**刻意只做一件**：S4 要证的是**调用路径**，
//! 而不是命令行的功能面。命令行的功能面（`--help`、子命令、退出码约定）
//! 属于 M1 的任务队列与 M3 的链路。
//!
//! ## 它不许做什么
//!
//! **不许绕开 `qul-app` 直接调 `qul-core` 的业务逻辑。**
//! 本文件里 `use qul_core::…` 只出现在构造**输入数据**的地方
//! （能力表要有人建），**判断本身一律走 `AppService`**。
//! 这条由 `qul-app/tests/layering.rs` 的同类检查守住。

use qul_app::AppService;
use qul_core::{Capabilities, Capability, CapabilityKey};

fn main() {
    // ── 建一份**演示用的能力表** ────────────────────────────────────────
    //
    // 这不是"默认配置"，只是让命令行**现在就能打出东西**。
    // M1 起，这张表会由 provider 的 `discover()` 产出。
    let capabilities = demo_capabilities();

    // ── 判断在这里发生（与界面调的是同一个函数）─────────────────────────
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

/// 演示用的能力表：**故意同时含启用项与禁用项**。
///
/// 为什么不让它全是启用的：那样就测不到"禁用项必须带原因"这条输出路径，
/// 而那条路径是**命令行最需要正确的一段**（用户看命令行就是为了知道为什么不行）。
fn demo_capabilities() -> Capabilities {
    let mut caps = Capabilities::new();

    // 通用能力：这一项可用
    caps.set(CapabilityKey::Launch, Capability::enabled());

    // 通用能力：这一项不可用，**必须给原因**
    //
    // 用 `InstanceDetail::StoreSandbox` 这个真实场景：沙箱形态下内容包不可管。
    // 这里只用它的**包装**来造一个禁用态，不引入产品名（内核里不许有产品名）。
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
