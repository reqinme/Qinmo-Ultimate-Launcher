//! # 启动链路的取证（M3 · 进程管理）
//!
//! ## 它证明的是"这条链通到哪一步"
//!
//! ```text
//!   ① 读版本详情            ← M2
//!   ② 组装参数骨架           ← M3（上一轮）
//!   ③ 求值 Java 需求 + 选 Java ← M2
//!   ④ 解析全部占位符          ← M1 的 plan
//!   ⑤ 真的拉起进程并回收       ← M3（本轮）
//! ```
//!
//! ## 🔴 它**不启动游戏**
//!
//! 它启动的是一个**本机的无害命令**，用来证明第 ⑤ 步真的能跑通。
//!
//! **理由有两条，而第二条是合规上的**：
//!
//! 1. 第 ①–④ 步的产物（参数骨架）应当能被**逐项检查**，
//!    而"真的启动一次游戏"会让检查变成"看它起没起来"；
//! 2. **本命令不接受也不读取任何账号令牌** —— 离线身份在
//!    [`qul_core::offline`] 里已经有模型，而把它接到这条链上需要
//!    **用户显式开启离线门禁**（规格 §1.3.4）。本命令**不代替那个开启**。
//!
//! 所以本命令的边界写在这里：**它是链路取证，不是启动器**。
//! 真正启动游戏的那条路是 `qul launch`（M3 的验收点），而它要门禁。

use qul_core::descriptor::{Descriptor, Env, PlatformTarget};
use qul_core::java::{choose_java, JavaChoice};
use qul_core::launch_plan::{assemble, AssembleInput};
use qul_core::plan::ResolvedCommand;
use qul_core::retry::CancelToken;
use qul_infra::process::{Channel, LineSink, RealProcessExecutor};
use std::collections::BTreeMap;
use std::sync::Arc;

/// 把日志同时打到屏幕与留在内存里。
struct TeeSink {
    keep: std::sync::Mutex<Vec<(Channel, String)>>,
    quiet: bool,
}

impl TeeSink {
    fn new(quiet: bool) -> Self {
        Self {
            keep: std::sync::Mutex::new(Vec::new()),
            quiet,
        }
    }
    fn count(&self) -> usize {
        self.keep.lock().expect("锁没被毒化").len()
    }
}

impl LineSink for TeeSink {
    fn line(&self, channel: Channel, text: &str) {
        if !self.quiet {
            let tag = match channel {
                Channel::Stdout => "out",
                Channel::Stderr => "err",
            };
            println!("    [{tag}] {}", text.chars().take(120).collect::<String>());
        }
        self.keep
            .lock()
            .expect("锁没被毒化")
            .push((channel, text.to_string()));
    }
}

fn template_map() -> BTreeMap<String, String> {
    [
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
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub fn run_launch_chain(version: Option<&str>) -> i32 {
    println!("=== 启动链路取证（M3）===");
    println!();

    // ── ① 读详情 ──
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
    let id = match version {
        Some(v) => v.to_string(),
        None => manifest.latest.release.clone(),
    };
    let detail_path = versions.join(&id).join(format!("{id}.json"));
    let detail = match std::fs::read_to_string(&detail_path) {
        Ok(t) => t,
        Err(e) => {
            println!("✗ 读不到 versions/{id}/{id}.json：{e}");
            return 2;
        }
    };
    println!("【1】版本详情");
    println!("  id   : {id}");
    match manifest.find(&id) {
        Some(e) => println!("  类型 : {}  发布于 {}", e.kind, e.release_time),
        None => println!("  ⚠️ 清单里没有这个 id"),
    }

    let d = match Descriptor::parse(&detail) {
        Ok(d) => d,
        Err(e) => {
            println!("✗ 详情解析失败：{e}");
            return 2;
        }
    };
    let env = Env::new(PlatformTarget::windows("10.0.26200", "x86_64"));
    println!("  主类 : {}", d.main_class().unwrap_or("(没有)"));
    println!();

    // ── ② 组装参数骨架 ──
    println!("【2】组装参数骨架");
    let input = AssembleInput {
        program: "{JAVA}".into(),
        main_class: String::new(),
        template_map: template_map(),
    };
    let a = match assemble(&d, &env, &input, Some("{CLASSPATH}")) {
        Ok(x) => x,
        Err(e) => {
            println!("✗ 组装失败：{e}");
            return 2;
        }
    };
    println!("  形态 : {}", a.form);
    println!("  参数 : {} 条", a.args.len());
    println!(
        "  有效库 {} 个（其中 natives {} 个）",
        a.libraries.len(),
        a.natives.len()
    );
    if !a.templates_unmapped.is_empty() {
        println!("  ✗ **未映射的官方模板变量：{:?}**", a.templates_unmapped);
        println!("     它们会被原样传给 Java —— 必须补上映射才能启动。");
        return 1;
    }
    println!("  ✓ 全部官方模板变量都已映射");
    println!();

    // ── ③ Java 需求与选择 ──
    println!("【3】Java 需求与选择");
    let req = d.java_requirement();
    println!(
        "  需求 : {} （{}）",
        req.requirement.human(),
        req.source.as_str()
    );
    let (discovery, _failed) = qul_infra::java::discover_and_probe();
    let choice = choose_java(&discovery.candidates, req.requirement);
    let java_path = match &choice {
        JavaChoice::Selected { candidate, reason } => {
            println!("  选中 : {}", candidate.path);
            println!("  理由 : {reason}");
            candidate.path.clone()
        }
        JavaChoice::NoJavaAtAll { requirement } => {
            println!("  ✗ 本机没有任何 Java，而需要 {}", requirement.human());
            return 1;
        }
        JavaChoice::NoneSatisfies {
            requirement,
            available,
        } => {
            println!(
                "  ✗ 本机 {} 个候选都不满足 {}",
                available.len(),
                requirement.human()
            );
            return 1;
        }
    };
    println!();

    // ── ④ 解析占位符 ──
    println!("【4】解析占位符（**`plan.resolve()` 的类型级保证：结果里没有任何 `{{...}}`**）");
    let mut plan = qul_core::plan::LaunchPlan::new("{PROGRAM}");
    for arg in &a.args {
        plan = plan.arg(arg.clone());
    }
    // 只填**解析骨架所必需**的那几个。**刻意不填任何账号相关的值。**
    plan = plan
        .bind("PROGRAM", &java_path)
        .bind("CLASSPATH", "(本例不真的启动游戏，classpath 省略)")
        .bind("NATIVES_DIR", mc.join("bin").display().to_string())
        .bind("VERSION_NAME", &id)
        .bind("GAME_DIR", mc.display().to_string())
        .bind("ASSETS_DIR", mc.join("assets").display().to_string())
        .bind(
            "ASSETS_INDEX",
            d.asset_index_ref()
                .map(|x| x.id.clone())
                .unwrap_or_default(),
        )
        .bind("LAUNCHER_NAME", "qinmo")
        .bind("LAUNCHER_VERSION", env!("CARGO_PKG_VERSION"))
        .bind("PLAYER_NAME", "(未填)")
        .bind("PLAYER_UUID", "(未填)")
        .bind("ACCESS_TOKEN", "(未填)")
        .bind("SESSION", "(未填)")
        .bind("VERSION_TYPE", "release")
        .bind("USER_TYPE", "legacy")
        .bind("USER_PROPERTIES", "{}")
        .bind("CLIENT_ID", "(未填)")
        .bind("XUID", "(未填)");
    let resolved = match plan.resolve() {
        Ok(r) => r,
        Err(un) => {
            println!("  ✗ 有未解析的占位符：");
            for u in &un {
                println!("     {u}");
            }
            return 1;
        }
    };
    println!(
        "  ✓ 解析成功：{} 个参数，**没有任何占位符残留**",
        resolved.args.len()
    );
    println!();

    // ── ⑤ 真的拉起一个进程 ──
    println!("【5】真的拉起一个进程（**不是游戏**，见本命令的文档）");
    println!("  为什么要跑这一步：它证明「链路的最后一环」真的通 ——");
    println!("  一个只打印参数、不真的启动进程的取证，验不到「进程管理」。");
    println!();
    // 用本机无害命令：证明 spawn / 日志回收 / 退出码 / 取消四条都对。
    #[cfg(windows)]
    let probe = ResolvedCommand {
        program: "cmd".into(),
        args: vec![
            "/c".into(),
            "echo chain-probe-ok & echo chain-probe-err 1>&2 & exit 7".into(),
        ],
        env: BTreeMap::new(),
        cwd: None,
    };
    #[cfg(not(windows))]
    let probe = ResolvedCommand {
        program: "sh".into(),
        args: vec![
            "-c".into(),
            "echo chain-probe-ok; echo chain-probe-err 1>&2; exit 7".into(),
        ],
        env: BTreeMap::new(),
        cwd: None,
    };
    println!("  探针命令 : {} {}", probe.program, probe.args.join(" "));
    let sink = Arc::new(TeeSink::new(false));
    let ex = RealProcessExecutor {
        poll_ms: 20,
        kill_tree: true,
    };
    let out = match ex.run_with_sink(&probe, &CancelToken::new(), sink.clone()) {
        Ok(o) => o,
        Err(e) => {
            println!("  ✗ 进程执行失败：{e}");
            return 1;
        }
    };
    println!();
    println!("  退出码     : {:?}", out.exit_code);
    println!("  收到行数   : {}", out.lines);
    println!("  耗时       : {} ms", out.elapsed_ms);
    println!("  是否被取消 : {}", out.cancelled);
    if out.exit_code != Some(7) {
        println!("  ✗ **退出码没有如实回收**（期望 7）");
        return 1;
    }
    if sink.count() < 2 {
        println!("  ✗ **日志没有收到两路**（收到 {} 行）", sink.count());
        return 1;
    }
    println!("  ✓ 退出码如实回收（7）· 两路日志都收到 · 进程已回收");
    println!();

    // ── ⑥ 取消 ──
    println!("【6】取消：返回 `None` 而**不是** `Err`");
    #[cfg(windows)]
    let slow = ResolvedCommand {
        program: "cmd".into(),
        args: vec!["/c".into(), "ping -n 30 127.0.0.1 >nul".into()],
        env: BTreeMap::new(),
        cwd: None,
    };
    #[cfg(not(windows))]
    let slow = ResolvedCommand {
        program: "sleep".into(),
        args: vec!["30".into()],
        env: BTreeMap::new(),
        cwd: None,
    };
    let cancel = Arc::new(CancelToken::new());
    let c2 = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        c2.cancel();
    });
    let started = std::time::Instant::now();
    match ex.run_with_sink(&slow, &cancel, Arc::new(TeeSink::new(true))) {
        Ok(o) => {
            let took = started.elapsed().as_millis();
            println!(
                "  被取消 : {}   退出码 : {:?}   耗时 : {took} ms",
                o.cancelled, o.exit_code
            );
            if !o.cancelled || o.exit_code.is_some() {
                println!("  ✗ 取消语义不对（该 cancelled=true 且 exit_code=None）");
                return 1;
            }
            if took > 15_000 {
                println!("  ✗ 取消后没有很快返回");
                return 1;
            }
            println!("  ✓ 取消是「用户的意愿」而不是故障 —— 所以是 `None` 而不是 `Err`");
        }
        Err(e) => {
            println!("  ✗ 取消被报成了错误：{e}");
            return 1;
        }
    }
    println!();

    println!("【结论】");
    println!("  ✓ ① 读详情 ② 组装骨架 ③ 选 Java ④ 解析占位符 —— **全部走通**");
    println!("  ✓ ⑤ 进程管理：spawn · 两路日志 · 退出码回收 —— **全部走通**");
    println!("  ✓ ⑥ 取消语义正确（`None` 而非 `Err`）");
    println!();
    println!("  ⚠️ 本命令**不启动游戏**，也不接受任何账号令牌。");
    println!("     真正的启动需要：离线门禁被用户显式开启（规格 §1.3.4），");
    println!("     以及 natives 解压、资源校验等前置 —— 那些是 M3 剩下的部分。");
    0
}
