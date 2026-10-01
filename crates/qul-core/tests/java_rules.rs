//! # S6 的规则层测试：Java 版本要求与选择
//!
//! ## 为什么这些能写成单测，而探测不能
//!
//! "该选哪个 Java"是**可以穷举的规则**；"本机装了哪些 Java"**只能实测**。
//! 所以规则部分的验证成本近乎为零，而它恰好是 S6 里最贵的一类错误
//! （选错 Java 是"游戏起不来"的头号原因，且用户看不懂报错）。
//!
//! ## 本文件里的三条纪律
//!
//! 1. **边界逐个测**：1.16.5→Java 8 与 1.17→Java 16 之间只差一个版本号，
//!    而那正是官方换 Java 要求的地方。
//! 2. **负面场景与正面同权**：缺 Java 时的提示必须是可操作的，
//!    所以它有自己的断言，不是"顺手看一下"。
//! 3. **选择必须确定**：同输入同输出。否则同一台机器两次跑出不同结果，
//!    用户与我们都无法复现问题。

use qul_core::java::{
    choose_java, is_launcher_stub, missing_java_message, GameVersion, JavaCandidate, JavaChoice,
    JavaRequirement,
};

fn java(major: u32, version: &str, bits: u32, path: &str) -> JavaCandidate {
    JavaCandidate {
        path: path.to_string(),
        major,
        version: version.to_string(),
        bits,
        vendor: "测试".into(),
    }
}

// ───────────────────────── 版本号解析与要求 ─────────────────────────

#[test]
fn 版本号解析接受正式版与预发布尾缀() {
    for (s, minor, patch) in [
        ("1.8.9", 8u32, 9u32),
        ("1.12.2", 12, 2),
        ("1.16.5", 16, 5),
        ("1.20.5", 20, 5),
        ("1.20.5-pre1", 20, 5),
        ("1.21.4", 21, 4),
        ("1.7.10", 7, 10),
        ("1.21", 21, 0),
    ] {
        let gv = GameVersion::parse(s).unwrap_or_else(|| panic!("{s} 应当能解析"));
        assert_eq!((gv.minor, gv.patch), (minor, patch), "{s}");
    }
}

#[test]
fn 版本号解析对垃圾输入返回_none_而不是猜() {
    // 猜错会让用户得到一个静默的错选择。所以"解析不了"必须是显式事实。
    for s in ["", "abc", "1", "x.y", "1.x"] {
        assert!(GameVersion::parse(s).is_none(), "{s} 不该被解析成功");
    }
}

#[test]
fn java_要求的边界逐个正确() {
    let cases = [
        ("1.7.10", JavaRequirement::Java8),
        ("1.8.9", JavaRequirement::Java8),
        ("1.12.2", JavaRequirement::Java8),
        ("1.16.5", JavaRequirement::Java8), // ← 上界
        ("1.17", JavaRequirement::Java16),  // ← 换档点
        ("1.17.1", JavaRequirement::Java16),
        ("1.18", JavaRequirement::Java17), // ← 又一档
        ("1.20.1", JavaRequirement::Java17),
        ("1.20.4", JavaRequirement::Java17), // ← 上界
        ("1.20.5", JavaRequirement::Java21), // ← 换档点
        ("1.21.4", JavaRequirement::Java21),
    ];
    for (v, want) in cases {
        let got = GameVersion::parse(v).unwrap().requirement();
        assert_eq!(got, want, "{v} 应要求 {want:?}，实际 {got:?}");
    }
}

#[test]
fn 一七与一八分开是有意的() {
    // 把 1.17 归到 Java 17 会让"只有 Java 16"的机器被误判为缺 Java，
    // 而 1.17 其实能用 16。**宁可少要一个版本，也不要把能跑的组合判成不能跑。**
    assert_eq!(
        GameVersion::parse("1.17").unwrap().requirement(),
        JavaRequirement::Java16
    );
    let only16 = vec![java(16, "16.0.2", 64, "C:\\jdk16\\bin\\java.exe")];
    let choice = choose_java(&only16, JavaRequirement::Java16);
    assert!(
        choice.selected().is_some(),
        "只有 Java 16 时应当能启动 1.17"
    );
}

// ───────────────────────── 选择 ─────────────────────────

#[test]
fn 一个_java_都没有时给出可操作提示() {
    let choice = choose_java(&[], JavaRequirement::Java21);
    assert!(matches!(choice, JavaChoice::NoJavaAtAll { .. }));
    let msg = missing_java_message(&choice).expect("缺 Java 必须有提示");
    // 提示必须同时说清"需要什么"与"下一步做什么"
    assert!(msg.contains("Java 21"), "要说清需要什么：{msg}");
    assert!(
        msg.contains("安装") || msg.contains("手动指定"),
        "要给出下一步：{msg}"
    );
}

#[test]
fn 有_java_但都不满足时提示里要列出已找到的版本() {
    // S6 第 4 条要求的形式："需要 Java 21，未找到；已找到 Java 8 与 17"
    let cands = vec![
        java(8, "1.8.0_503", 64, "C:\\jre8\\bin\\java.exe"),
        java(17, "17.0.20", 64, "C:\\jdk17\\bin\\java.exe"),
    ];
    let choice = choose_java(&cands, JavaRequirement::Java21);
    assert!(matches!(choice, JavaChoice::NoneSatisfies { .. }));
    let msg = missing_java_message(&choice).expect("必须有提示");
    assert!(msg.contains("Java 21"), "{msg}");
    assert!(msg.contains("Java 8"), "要列出已找到的：{msg}");
    assert!(msg.contains("Java 17"), "要列出已找到的：{msg}");
}

#[test]
fn 选中项是满足要求里主版本最低的() {
    // 能用 17 跑就别上 21：更高版本有时会让老模组出问题，而用户看不出原因。
    let cands = vec![
        java(21, "21.0.12.1", 64, "C:\\jdk21\\bin\\java.exe"),
        java(17, "17.0.20", 64, "C:\\jdk17\\bin\\java.exe"),
        java(25, "25.0.4.1", 64, "C:\\jdk25\\bin\\java.exe"),
    ];
    let choice = choose_java(&cands, JavaRequirement::Java17);
    let c = choice.selected().expect("应当选出");
    assert_eq!(c.major, 17, "应当选 17 而不是更高的");
}

#[test]
fn 六十四位优先于三十二位() {
    let cands = vec![
        java(17, "17.0.20", 32, "C:\\a32\\bin\\java.exe"),
        java(17, "17.0.20", 64, "C:\\b64\\bin\\java.exe"),
    ];
    let choice = choose_java(&cands, JavaRequirement::Java17);
    assert_eq!(choice.selected().unwrap().bits, 64);
}

#[test]
fn 只有三十二位时退回并说明风险() {
    let cands = vec![java(17, "17.0.20", 32, "C:\\a32\\bin\\java.exe")];
    let choice = choose_java(&cands, JavaRequirement::Java17);
    match choice {
        JavaChoice::Selected { candidate, reason } => {
            assert_eq!(candidate.bits, 32);
            assert!(
                reason.contains("32 位") || reason.contains("退回"),
                "退回 32 位必须说明：{reason}"
            );
        }
        other => panic!("应当退回 32 位而不是失败，实际 {other:?}"),
    }
}

#[test]
fn 避开_oracle_启动器存根() {
    // 存根按注册表转发到某个 JRE：用户在别处改了注册表，我们选的 Java 就变了。
    // 那正是 S6 要消灭的那类"用户看不懂的失败"。
    let stub = "C:\\Program Files (x86)\\Common Files\\Oracle\\Java\\java8path\\java.exe";
    let real = "C:\\Program Files\\Java\\jre1.8.0_503\\bin\\java.exe";
    assert!(is_launcher_stub(stub), "应当识别为存根");
    assert!(!is_launcher_stub(real), "真实 JRE 不该被判成存根");

    let cands = vec![
        java(8, "1.8.0_503", 64, stub),
        java(8, "1.8.0_503", 64, real),
    ];
    let choice = choose_java(&cands, JavaRequirement::Java8);
    assert_eq!(choice.selected().unwrap().path, real, "应当避开存根");
}

#[test]
fn 全是存根时仍然可用但要说清() {
    // 有总比没有强：只有存根的机器上，退回存根比"报缺 Java"更有用。
    let stub = "C:\\Common Files\\Oracle\\Java\\javapath\\java.exe";
    let cands = vec![java(25, "25.0.4.1", 64, stub)];
    let choice = choose_java(&cands, JavaRequirement::Java21);
    match choice {
        JavaChoice::Selected { reason, .. } => {
            assert!(reason.contains("存根"), "退回存根必须说明：{reason}");
        }
        other => panic!("应当退回存根，实际 {other:?}"),
    }
}

#[test]
fn 选择是确定的() {
    // 同输入必须同输出：否则同一台机器两次跑出不同结果，问题无法复现。
    let cands = vec![
        java(17, "17.0.20", 64, "C:\\z\\bin\\java.exe"),
        java(17, "17.0.20", 64, "C:\\a\\bin\\java.exe"),
        java(21, "21.0.12.1", 64, "C:\\m\\bin\\java.exe"),
    ];
    let first = choose_java(&cands, JavaRequirement::Java17);
    for _ in 0..8 {
        assert_eq!(choose_java(&cands, JavaRequirement::Java17), first);
    }
}
