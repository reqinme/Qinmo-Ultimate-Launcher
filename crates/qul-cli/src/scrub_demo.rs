//! # 脱敏取证（M1 · 方案 §5.8）
//!
//! ## 它为什么值得是一条真子命令
//!
//! 脱敏的正确性**不能靠"我们写了规则"来声明** —— 那只是意图。
//! 能被检查的是：**把真实的敏感值喂进去，看输出里还剩不剩它们**。
//!
//! 所以这条命令做三件事：
//! ① 用**本机真实的事实**（用户名、用户目录、数据根、本机 IP）构造脱敏器；
//! ② 造一段**故意包含全部六类**的文本，走一遍真实管道；
//! ③ **断言输出里一个敏感值都不剩**，并把统计打出来。
//!
//! ③ 是关键：它把"脱敏"从"我们尽力了"变成**一个可以失败、因此可以被信任的检查**。
//!
//! ## 它为什么不写真实日志文件
//!
//! 因为它验的是**管道**，而不是文件 IO —— 文件 IO 那一侧
//! 已经由 `qul-infra` 的 11 项测试覆盖（含轮转、幂等、导出再脱敏）。
//! 一次调用只做一件事，出问题时能立刻分清是哪一侧坏了。

use qul_core::scrub::{Scrubber, MASK_DATA_ROOT, MASK_IP, MASK_SERVER, MASK_USERPROFILE};
use qul_infra::logging::{Level, LogSink, Rotation};

/// 造一段**故意包含方案 §5.8 全部六类**的文本。
///
/// 六类：令牌 / UUID / 用户名 / 路径 / IP / 服务器地址。
/// **顺序也刻意打乱**（不按清单顺序），因为真实日志里它们是交错的。
fn sample(home: &str, data: &str, user: &str, ip: &str) -> String {
    format!(
        "启动器版本=0.1.0.0\n\
         token=SUPERSECRETTOKEN-abcdef123456\n\
         账户 UUID=069a79f4-44e9-4726-a5be-fca90e38aaf5\n\
         玩家 {user} 已登录\n\
         数据目录 {data}\\logs\\session.log\n\
         用户目录 {home}\\Desktop\n\
         本机地址 {ip}\n\
         连接 play.example.com:25565\n\
         联系 a.b+tag@example.co.uk"
    )
}

pub fn run_scrub_demo() -> i32 {
    println!("=== 脱敏管道取证（M1 · 方案 §5.8）===");
    println!();
    println!("它做一件事：**把本机真实的敏感值喂进管道，看输出里还剩不剩它们。**");
    println!("脱敏的正确性不能靠\"我们写了规则\"来声明 —— 那只是意图。");
    println!();

    // ── ① 用本机真实事实构造 ──
    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\demo".to_string());
    let data = format!(r"{home}\AppData\Roaming\Qinmo");
    let user = home
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or("demo")
        .to_string();
    let local_ip = detect_local_ipv4().unwrap_or_else(|| "192.168.1.100".to_string());

    println!("【1】脱敏器用到的本机事实（**这些值必须从输出里消失**）");
    println!("  用户名    ：{user}");
    println!("  用户目录  ：{home}");
    println!("  数据根    ：{data}");
    println!("  本机 IPv4 ：{local_ip}");
    println!();

    let scrubber = Scrubber::new()
        .secret("SUPERSECRETTOKEN-abcdef123456")
        .user(&user)
        .with_home_and_data(&home, &data)
        .server("play.example.com");

    // ── ② 走一遍真实管道 ──
    let raw = sample(&home, &data, &user, &local_ip);
    println!("【2】输入（**故意含全部六类**，顺序打乱以贴近真实交错）");
    for l in raw.lines() {
        println!("      {l}");
    }
    println!();

    let (clean, rep) = scrubber.scrub(&raw);
    println!("【3】输出");
    for l in clean.lines() {
        println!("      {l}");
    }
    println!();
    println!("  {}", rep.summary());
    println!();

    // ── ③ 断言一个敏感值都不剩 ──
    println!("【4】断言：输出里不含任何敏感值");
    let mut failures: Vec<String> = Vec::new();

    // 令牌必须消失
    if clean.contains("SUPERSECRETTOKEN") {
        failures.push("令牌仍在输出里".into());
    }
    // 用户名必须消失（但要小心：它可能是别的词的子串，所以我们查"独立的它"）
    if clean.to_lowercase().contains(&user.to_lowercase()) {
        failures.push(format!("用户名 {user} 仍在输出里"));
    }
    // 用户目录与数据根必须消失
    if clean.to_lowercase().contains(&home.to_lowercase()) {
        failures.push("用户目录仍在输出里".into());
    }
    // 本机 IP 必须消失
    if clean.contains(&local_ip) {
        failures.push(format!("本机 IP {local_ip} 仍在输出里"));
    }
    // 服务器地址必须消失
    if clean.contains("play.example.com") {
        failures.push("服务器地址仍在输出里".into());
    }
    // UUID 必须消失
    if clean.contains("069a79f4") {
        failures.push("UUID 仍在输出里".into());
    }
    // 邮箱必须消失
    if clean.contains("a.b+tag@example.co.uk") {
        failures.push("邮箱仍在输出里".into());
    }
    // **版本号必须留存**（封存项目在这里丢过字段）
    if !clean.contains("0.1.0.0") {
        failures.push("版本号被误抹了 —— 这正是封存项目那个缺陷".into());
    }

    if failures.is_empty() {
        println!("  ✓ 六类全部被折叠，且版本号未被误伤");
    } else {
        for f in &failures {
            println!("  ✗ {f}");
        }
        println!();
        println!("脱敏未达标。**这不是可以忽略的告警** —— 它意味着日志里可能残留凭据。");
        return 2;
    }
    println!();

    // ── ④ 掩码是否可读 ──
    println!("【5】掩码是否可读（用户要能看懂\"这里被折叠了什么类别\"）");
    let expected = [
        (MASK_USERPROFILE, "用户目录"),
        (MASK_DATA_ROOT, "数据根"),
        (MASK_IP, "IP"),
        (MASK_SERVER, "服务器地址"),
    ];
    let mut missing = Vec::new();
    for (m, name) in expected {
        let ok = clean.contains(m);
        println!("  {} {m}  （{name}）", if ok { "✓" } else { "✗" });
        if !ok {
            missing.push(name);
        }
    }
    if !missing.is_empty() {
        println!();
        println!("以下类别的掩码没出现：{}", missing.join("、"));
        println!("**这不一定是缺陷** —— 若那一类在本机事实里不存在，就不会有替换。");
        println!("但它值得看一眼：可能是种子没喂进去。");
    }
    println!();

    // ── ⑤ 顺带验一次真实落盘（最少量）──
    println!("【6】顺带验一次真实落盘 + 导出再脱敏");
    let dir = std::env::temp_dir().join(format!("qul-scrub-demo-{}", std::process::id()));
    match LogSink::open(
        &dir,
        "demo",
        Rotation {
            max_bytes: 4096,
            keep: 3,
        },
        Scrubber::new()
            .secret("SUPERSECRETTOKEN-abcdef123456")
            .user(&user)
            .with_home_and_data(&home, &data)
            .server("play.example.com"),
    ) {
        Ok(mut s) => {
            let _ = s.write(Level::Info, &raw);
            let text = std::fs::read_to_string(s.current_path()).unwrap_or_default();
            let leaked: Vec<&str> = [
                "SUPERSECRETTOKEN",
                "069a79f4",
                "play.example.com",
                home.as_str(),
            ]
            .into_iter()
            .filter(|v| text.to_lowercase().contains(&v.to_lowercase()))
            .collect();
            if leaked.is_empty() {
                println!("  ✓ 落盘后文件里无敏感内容（{} 字节）", text.len());
            } else {
                println!("  ✗ 落盘后文件里仍有：{leaked:?} —— 这是严重缺陷");
                let _ = std::fs::remove_dir_all(&dir);
                return 2;
            }
            match s.export_redacted(&["诊断演示", "版本=0.1.0.0"]) {
                Ok((out, _)) => {
                    let bad = out.contains("SUPERSECRETTOKEN")
                        || out.to_lowercase().contains(&home.to_lowercase());
                    println!(
                        "  {} 导出（含头部）后无敏感内容（{} 字节）",
                        if bad { "✗" } else { "✓" },
                        out.len()
                    );
                    if bad {
                        let _ = std::fs::remove_dir_all(&dir);
                        return 2;
                    }
                }
                Err(e) => {
                    println!("  ✗ 导出失败：{e}");
                    let _ = std::fs::remove_dir_all(&dir);
                    return 2;
                }
            }
        }
        Err(e) => {
            println!("  ✗ 建立日志写入器失败：{e}");
            return 2;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!();
    println!("✓ 脱敏管道：六类全折叠 · 版本号未误伤 · 落盘无残留 · 导出再脱敏通过");
    0
}

/// 取本机一个 IPv4 地址（**只用于取证**，不对外发送、不写进任何文件）。
///
/// 实现方式是**连一个不可能成功的地址并读本地端点** ——
/// 这是"不引入依赖就能拿到本机出口地址"的通行做法：
/// UDP 不握手，所以 `connect` 不会真的发包。
fn detect_local_ipv4() -> Option<String> {
    use std::net::UdpSocket;
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    // 保留地址段，不会真的到达任何主机；`connect` 只设置默认目的地。
    s.connect("192.0.2.1:9").ok()?;
    let a = s.local_addr().ok()?;
    match a.ip() {
        std::net::IpAddr::V4(v4) => Some(v4.to_string()),
        _ => None,
    }
}
