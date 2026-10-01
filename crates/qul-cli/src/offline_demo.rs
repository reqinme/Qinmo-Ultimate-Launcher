//! # 离线身份的取证（M2 · 最后一项）
//!
//! ## 它要让人看见的三件事
//!
//! 1. **UUID 是推出来的，且与外部实现一致** —— 用三份参照实现确认过；
//! 2. **三条规格纪律是类型级的**，不是靠自觉：
//!    门禁默认关闭、告知不存在"不提示"、令牌是常量；
//! 3. **它给出的"事实"是什么** —— 那是 M3 组参数要用的东西。
//!
//! ## 为什么把这三件事放在一条命令里
//!
//! 因为离线身份**最容易出错的地方不是算法，而是边界** ——
//! 一个"顺手默认打开"、"顺手加了不再提示"的实现，算法完全正确，
//! 而它越界了。所以这条命令**刻意把边界打印在第 2 段**。

use qul_core::offline::{
    derive_uuid, Disclosure, DisclosureFrequency, OfflineGate, OfflineIdentity,
    OFFLINE_NAMESPACE_PREFIX, OFFLINE_TOKEN,
};

pub fn run_offline_demo(name: Option<&str>) -> i32 {
    println!("=== 离线身份取证（M2）===");
    println!();

    let n = name.unwrap_or("Steve");

    // ── ① 推导 ──
    println!("【1】UUID 推导（**协议契约，不是我们的发明**）");
    println!("  前缀 : {OFFLINE_NAMESPACE_PREFIX}  （协议常量；服务器用的是同一个串）");
    println!("  算法 : MD5(前缀 + 名字) → 版本3 UUID（不是 SHA-1 / v5）");
    println!("  名字 : {n}");
    match OfflineIdentity::derive(n) {
        Ok(id) => {
            println!("  UUID : {}", id.uuid);
            println!("  令牌 : {}", id.token);
            println!();
            println!("  与三份外部实现比对（见 tests/offline_uuid_vectors.rs）：");
            println!("    HMCL  OfflineAccountFactory.java:86");
            println!("    Prism MinecraftAccount.cpp:282");
            println!("    XMCL  packages/user-offline-uuid/");
            println!("  而向量本身是**用 XMCL 的实现在 Node 里算出来的**，");
            println!("  且算了两遍（Node crypto 版 vs 手写 MD5 版，必须一致）。");
        }
        Err(e) => {
            println!("  ✗ 名字不合法：{e}");
            println!();
            println!("  合法的名字：3–16 个字符，只用英文字母/数字/下划线，");
            println!("  且不以下划线开头或结尾。");
            return 2;
        }
    }
    println!();

    // ── ② 三条规格纪律（**类型级，不是靠自觉**）──
    println!("【2】三条纪律：它们是**类型级的**，不靠自觉");
    {
        let g = OfflineGate::default();
        println!(
            "  ① 门禁默认值 : {}",
            if g.is_enabled() {
                "**开启（错！）**"
            } else {
                "关闭 ✓"
            }
        );
        println!("     关着的时候不许启动，且错误里带着用户该读的那句话：");
        match g.allow_launch() {
            Ok(()) => println!("       ✗ 竟然允许启动了"),
            Err(e) => println!("       {e}"),
        }
    }
    {
        let d = Disclosure::default();
        println!();
        println!("  ② 告知文案返回 `String` 而不是 `Option<String>`");
        println!("     —— 一个 `Option` 的 `None` 分支**就是**「永久关掉」。");
        println!("     当前频率：{:?}（**枚举里没有 `Never`**）", d.frequency);
        println!("     文案（产品名由调用方给）：");
        for line in d.text("某产品").lines() {
            println!("       {line}");
        }
    }
    {
        println!();
        println!("  ③ 令牌是常量 `{OFFLINE_TOKEN}`，不是随机值");
        println!("     —— 随机会让人（与日志）误以为这是真实会话；");
        println!("        而它**只发给本地游戏进程**，绝不发给任何官方服务。");
    }
    println!();

    // ── ③ 降频之后仍然有文本 ──
    println!("【3】「可降低频率，但不能取消」的落点");
    let d2 = Disclosure {
        frequency: DisclosureFrequency::HourlyAtMost,
    };
    println!("  降到 HourlyAtMost 之后：");
    println!(
        "    同一小时内重复显示吗 : {}",
        if d2.should_show_now("2026-10-02T05:30:00", Some("2026-10-02T05:01:00")) {
            "是（错）"
        } else {
            "否 ✓"
        }
    );
    println!(
        "    换一小时之后        : {}",
        if d2.should_show_now("2026-10-02T06:00:00", Some("2026-10-02T05:59:00")) {
            "是 ✓"
        } else {
            "否（错）"
        }
    );
    println!(
        "    文案还在吗          : {}",
        if d2.text("p").contains("不会消失") {
            "在，且明说了不会消失 ✓"
        } else {
            "**没了（错）**"
        }
    );
    println!();

    // ── ④ 它给出的事实（M3 要用）──
    println!("【4】它给出的「事实」—— M3 组参数时取这些");
    let id = OfflineIdentity::derive(n).unwrap();
    for (k, v) in id.fact_pairs() {
        println!("  {k:<26} = {v}");
    }
    println!();
    println!("  ⚠️ 注意最后一行：`identity.authenticated` **永远是 false**。");
    println!("     离线身份不许被界面显示成「已登录」——");
    println!("     把它变成一个显式的事实，比在界面里各处判断来源更难写错。");
    println!();

    // ── ⑤ 确定性 ──
    println!("【5】确定性：同一个名字永远同一个 UUID");
    for who in ["Steve", "Steve", "Alex", "Steve"] {
        println!("  {who:<8} → {}", derive_uuid(who));
    }
    println!("  —— 这不是「顺便的性质」，而是协议要求：");
    println!("     否则同一个玩家每次启动都会被服务器当成新人。");
    println!();
    println!("✓ 离线身份：UUID 与三份外部实现一致 · 三条纪律是类型级的 · 事实已产出");
    0
}
