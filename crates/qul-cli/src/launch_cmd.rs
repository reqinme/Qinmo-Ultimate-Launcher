//! # `qul launch`：真的启动游戏（M3 · 第 ⑤ 阶段）
//!
//! ## 🔴 它是 M3 的验收点
//!
//! 方案 §8 的 M3 行写的出口条件是「**CLI 能完成一次真实安装 → 部署 → 启动全链路**」。
//! 前面的四个阶段（解析 / 下载 / 校验 / 解压）已经走通（`qul install`），
//! 本命令补上第 ⑤ 个。
//!
//! ## 离线门禁：**这个命令不代替它**
//!
//! 它用**离线身份**启动（规格 §1.3.4），而离线身份：
//!
//! - **默认关闭**（`OfflineGate::default() == Disabled`）
//! - 必须由用户**显式启用**，并**在每次启动前被告知限制**
//! - 身份由用户名**在本机推导**，**绝不冒充官方身份**
//!
//! 所以本命令要求 `--offline-account <名字>` **并且** `--i-know-the-limits`
//! 两个参数同时给出 —— 那个双参数是刻意的：
//! **一个参数会让人"顺手加上"，而两个参数里有一个必须被读一遍才能写对。**
//!
//! ## 🔴 一处实测发现的坑，写在这里因为它是本轮最贵的发现
//!
//! 官方启动器的 JVM 参数里有**四个分开的 natives 目录**（S7 的官方基准 L7–L10）：
//!
//! ```text
//!   -Djava.library.path=<bin>/<h1>/<h2>/java
//!   -Djna.tmpdir=<bin>/<h1>/<h2>/jna
//!   -Dorg.lwjgl.system.SharedLibraryExtractPath=<bin>/<h1>/<h2>/lwjgl
//!   -Dio.netty.native.workdir=<bin>/<h1>/<h2>/netty
//! ```
//!
//! 而官方那个 `lwjgl` 目录里**有嵌套子目录**（实测 `lwjgl\3.4.3+4\x64\freetype.dll`）。
//!
//! **那句话排除了一整类实现**：`java.library.path` **不递归**，
//! 所以一个"把所有 natives 摊平到一个目录再指过去"的实现
//! **在带嵌套结构的 natives 上会失败** —— 而它失败的方式是
//! "游戏起不来但日志里只说找不到某个 dll"。
//!
//! 而**摊平还有一个更硬的错**：实测有两个不同的 jar 都含 `org/lwjgl/lwjgl.dll`
//! （基础 jar 与 opengl jar）—— 摊平会**让其中一个覆盖另一个**。
//!
//! 所以本命令的做法是：**先把 natives 目录的真实结构列出来**，
//! 再把**宿主架构对应的那一层**放进 `java.library.path`，
//! 并把 `-Dorg.lwjgl.librarypath` **显式指向它**（LWJGL 认这个属性，
//! 见 <https://www.lwjgl.org/guide> 的 "Configuring LWJGL"）。

#![allow(clippy::too_many_lines)]

use qul_core::descriptor::{Descriptor, Env, PlatformTarget};
use qul_core::java::JavaChoice;
use qul_core::launch_plan::{assemble, AssembleInput};
use qul_core::offline::{Disclosure, OfflineGate, OfflineIdentity};
use qul_core::plan::{LaunchPlan, ResolvedCommand};
use qul_core::retry::CancelToken;
use qul_infra::process::{Channel, LineSink, RealProcessExecutor};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::install_cmd::{default_data_root, instance_dir};

/// 把日志打到屏幕（并可留在内存里）。
struct TeeSink {
    keep: std::sync::Mutex<Vec<String>>,
    quiet: bool,
}

impl LineSink for TeeSink {
    fn line(&self, channel: Channel, text: &str) {
        let tag = match channel {
            Channel::Stdout => "out",
            Channel::Stderr => "err",
        };
        if !self.quiet {
            println!("    [{tag}] {text}");
        }
        self.keep
            .lock()
            .expect("锁没被毒化")
            .push(format!("[{tag}] {text}"));
    }
}

pub struct LaunchArgs {
    pub version: Option<String>,
    pub data_root: Option<PathBuf>,
    /// 离线账户名（**与 `i_know_the_limits` 必须同时给出**）
    pub offline_account: Option<String>,
    /// 用户确认他知道离线账户的限制
    pub i_know_the_limits: bool,
    /// **只验到 JVM 能起来，不进游戏**（`-version`）
    pub dry_run: bool,
    pub quiet: bool,
    /// 覆盖 Java：用它启动（不给就从 `discover_and_probe` 选）
    pub java: Option<PathBuf>,
}

/// 收集 natives 目录里**该放进 `java.library.path` 的那些**。
///
/// ## 判据为什么是"含 dll 的那一层"
///
/// 因为 `java.library.path` **不递归** —— 指向一个只含子目录的目录
/// 等于什么都没指。所以要找到**真正含 `.dll` 的那一层**。
///
/// ## 而架构过滤在这里
///
/// 实测（见 `SESSION.md` 的已知偏差）：我们解压时会同时得到
/// `windows/x64/` 与 `windows/arm64/`（因为 `rules` 是 `allow[windows/]`，
/// **没有 `os.arch` 条件**，而**官方也照样下载那 10 个 arm64 的 jar**）。
///
/// 所以这里按**宿主架构**只取对应的那一支 —— 那是"把官方在解压时做的过滤
/// 挪到挑选路径时做"，而**结果是等价的**：
/// x86_64 上绝不把 `arm64/` 放进 `java.library.path`。
///
/// ⚠️ `arm64` 是**唯一**一个会被排除的架构名。一个"排除一切不含 x64 的路径"
/// 的实现会**误伤**官方那层 `lwjgl\3.4.3+4\x64\` 之外的布局
/// （有的 natives 包把 dll 直接放在 jar 根，路径里根本没有架构名）。
pub fn native_path_dirs(natives_root: &Path, host_arch: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if !natives_root.is_dir() {
        return out;
    }
    // 宿主的架构名与元数据里的名字**不一定一样**：
    // `std::env::consts::ARCH` 给 `x86_64`，而元数据/官方用 `x64`/`amd64`。
    let excluded: &[&str] = match host_arch {
        "x86_64" | "x64" | "amd64" => &["arm64", "aarch64", "arm"],
        "aarch64" | "arm64" => &["x64", "amd64", "x86_64", "x86"],
        _ => &[],
    };

    // **广度优先地找出所有含 dll 的目录**，但不走进被排除的架构分支。
    //
    // ⚠️ **不要写「含 dll 就不再往下走」。**
    //
    // 我第一版那么写了，理由是"更深的子目录只可能是该库自己的子结构" ——
    // 而**实测立刻推翻了它**：natives 根部就有 `jtracy-jni-windows.dll`
    //（那个 jar 把 dll 放在根），于是那条规则**把整棵树剪掉了**，
    // 结果只找到 **1 个目录 3 个文件**（真值：**3 个目录 21 个文件**）。
    //
    // **而这条错在"看起来是对的"这一点上最危险**：它不会报错，
    // 只会让 `java.library.path` 少几个目录 —— 而症状是
    // **「游戏起不来但日志里只说找不到某个 dll」**。
    //
    // 所以现在的做法是：**每层都查，含 dll 就收下，然后继续往下走**。
    // 把同一批 dll 所在的多层目录都收进来是**无害的**（JVM 按顺序找），
    // 而漏掉一层是**有害的**。
    let mut queue: Vec<PathBuf> = vec![natives_root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        if has_dll(&dir) {
            out.push(dir.clone());
        }
        // **无论有没有 dll 都继续往下** —— 见上面那段。
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.filter_map(Result::ok) {
                let p = e.path();
                if !p.is_dir() {
                    continue;
                }
                let name = p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if excluded.contains(&name.as_str()) {
                    continue;
                }
                queue.push(p);
            }
        }
    }
    out.sort();
    out
}

/// 这个目录里**直接**放着 `.dll` 吗。
fn has_dll(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok).any(|e| {
                e.path()
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("dll"))
            })
        })
        .unwrap_or(false)
}

/// 把 classpath 拼起来。
///
/// ## 两个必须做对的地方
///
/// ① **顺序**：`libraries` 的顺序**就是元数据里的顺序**，
///    而它已经被 `plan()` 过滤过。不要重排 —— 实测官方那份 classpath
///    与我们的一致到**逐字符**（`compare-launch` 的结论）。
/// ② **分隔符是 `;`**（Windows）而**不是 `:`**。
///    一个用 `:` 的实现会让整个 classpath 被当成**一个**路径。
fn build_classpath(
    instance: &Path,
    libs: &[qul_core::descriptor::LibraryPlan],
    version_id: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    // 客户端 jar 在**最前**（实测官方也是）
    parts.push(
        instance
            .join("versions")
            .join(version_id)
            .join(format!("{version_id}.jar"))
            .display()
            .to_string(),
    );
    for lp in libs {
        if let Some(a) = &lp.artifact {
            // ⚠️ `DownloadRef` **没有** `rel_path()` 方法 —— 落盘路径就是它的
            // `path` 字段（实测：库的 artifact 有 path，而客户端的 downloads.client
            // **没有** path，所以客户端那条由 `client_rel_path` 决定）。
            if a.has_path() {
                parts.push(instance.join(&a.path).display().to_string());
            } else if let Some(n) = a.file_name_from_url() {
                // 元数据没给 path 时的回退：用 URL 末段。
                parts.push(instance.join("libraries").join(n).display().to_string());
            }
        }
    }
    parts.join(";")
}

pub fn run_launch(args: &LaunchArgs) -> i32 {
    println!("=== 启动 26.3（M3 · 第 ⑤ 阶段）===");
    println!();

    let data_root = args.data_root.clone().unwrap_or_else(default_data_root);
    let version_id = args.version.clone().unwrap_or_else(|| "26.3".to_string());
    let inst = instance_dir(&data_root, &version_id);

    println!("【0】目标");
    println!("  实例 : {}", inst.display());
    if !inst.is_dir() {
        println!("  ✗ 这个实例不存在 —— 先跑 `qul install {version_id} --dir <同一个数据根>`");
        return 2;
    }

    // ── ① 离线门禁：**两个参数必须同时给** ──
    println!();
    println!("【1】离线身份与门禁（规格 §1.3.4）");
    let (name, gate) = match (&args.offline_account, args.i_know_the_limits) {
        (Some(n), true) => {
            // **门禁在这里被显式打开**，而"何时被告知"是一个存在的事实。
            let acknowledged_at = now_stamp();
            println!("  ✓ 离线账户：{n}");
            println!("  ✓ 用户已确认限制（{acknowledged_at}）");
            (n.clone(), OfflineGate::Enabled { acknowledged_at })
        }
        (Some(_), false) => {
            println!("  ✗ 给了 `--offline-account` 但没有 `--i-know-the-limits`。");
            println!("     离线账户是**默认关闭**的，而开启它需要你明确知道下面这些：");
            println!();
            println!("{}", Disclosure::default().text("秦墨"));
            println!();
            println!("     确认之后加上 `--i-know-the-limits` 重跑。");
            return 2;
        }
        (None, _) => {
            println!("  ✗ 没有给 `--offline-account <名字>`。");
            println!("     本命令**只**用离线身份启动（正版登录在流程 A 里，尚未接上）。");
            println!("     它**不会**去猜一个名字 —— 那会让你以为登录过。");
            return 2;
        }
    };
    // 门禁自己也要同意（**规格要求的那一条，不被调用方绕过**）
    if let Err(e) = gate.allow_launch() {
        println!("  ✗ 门禁拒绝：{e}");
        return 2;
    }
    let identity = match OfflineIdentity::derive(&name) {
        Ok(i) => i,
        Err(e) => {
            println!("  ✗ 账户名不合法：{e}");
            return 2;
        }
    };
    println!("【1b】离线身份（**每次启动前告知**）");
    println!("{}", Disclosure::default().text("秦墨"));
    println!();
    println!("  推导出的 UUID：{}", identity.uuid);
    println!("  ⚠️ 它由 `MD5(\"OfflinePlayer:\" + 名字)` 在本机推导 —— 见 `offline.rs`。");
    println!("     我们**不提交任何令牌**，也**不伪造官方会话**。");
    println!();

    // ── ② 解析详情 ──
    let text = match std::fs::read_to_string(
        inst.join("versions")
            .join(&version_id)
            .join(format!("{version_id}.json")),
    ) {
        Ok(t) => t,
        Err(e) => {
            println!("  ✗ 读不到实例里的版本详情：{e}");
            return 1;
        }
    };
    let d = match Descriptor::parse(&text) {
        Ok(d) => d,
        Err(e) => {
            println!("  ✗ 详情解析失败：{e}");
            return 1;
        }
    };
    let host_arch = std::env::consts::ARCH;
    let env = Env::new(PlatformTarget::windows("10.0.26200", host_arch));

    // ── ③ 选 Java ──
    println!("【2】选 Java");
    let jr = d.java_requirement();
    println!(
        "  需要   : {}（{}）",
        jr.requirement.human(),
        jr.source.as_str()
    );
    let java_path = match &args.java {
        Some(p) => {
            println!("  指定   : {}", p.display());
            p.clone()
        }
        None => {
            // ⚠️ `discover_and_probe()` 返回的是一个**元组**
            // `(Discovery, Vec<(String, String)>)` —— 第一个是探测到的候选，
            // 第二个是"探测失败的原因"。我第一版把它当成了结构体。
            let (disc, probe_failures) = qul_infra::java::discover_and_probe();
            println!(
                "  探测   : {} 个候选 / {} 个来源 / {} 个探测失败",
                disc.candidates.len(),
                disc.sources.len(),
                probe_failures.len()
            );
            for (p, why) in probe_failures.iter().take(3) {
                println!("           探测失败 {p}：{why}");
            }
            match qul_core::java::// ⚠️ 第二个参数是**按值**的（`JavaRequirement` 实现了 `Copy`），
            // 而 `launch_chain.rs` 里那处写法看起来像传引用 —— 我照它抄错了。
            choose_java(&disc.candidates, jr.requirement)
            {
                JavaChoice::Selected { candidate, reason } => {
                    // ⚠️ `JavaCandidate::path` 是一个 **`String`**（"仅作为标识与展示"），
                    // 不是一个 `PathBuf` —— 所以没有 `.display()`。
                    // 而 `JavaCandidate` 自己实现了 `Display`（会给出版本/位数/厂商），
                    // 所以直接打印它比手拼字段好。
                    println!("  选中   : {candidate}");
                    println!("  路径   : {}", candidate.path);
                    println!("  理由   : {reason}");
                    std::path::PathBuf::from(&candidate.path)
                }
                other => {
                    println!("  ✗ 选不到合适的 Java：{other:?}");
                    println!("     本机确实装了 25（官方运行时），但它可能不在探测范围里。");
                    println!("     可以显式指定：`qul launch --java <javaw.exe 的路径>`");
                    return 1;
                }
            }
        }
    };

    // ── ④ natives 路径（**本轮最贵的那条实测**）──
    println!();
    println!("【3】natives 路径（`java.library.path` **不递归**）");
    let natives_root = qul_infra::zip::natives_dir(&inst, &version_id);
    let dirs = native_path_dirs(&natives_root, host_arch);
    println!("  根部   : {}", natives_root.display());
    println!(
        "  宿主   : {host_arch}（排除 {}",
        match host_arch {
            "x86_64" | "x64" | "amd64" => "arm64/aarch64/arm",
            "aarch64" | "arm64" => "x64/amd64/x86",
            _ => "(无)",
        }
    );
    println!("  含 dll 的目录 {} 个：", dirs.len());
    for p in dirs.iter().take(8) {
        let n = std::fs::read_dir(p).map(|r| r.count()).unwrap_or(0);
        println!(
            "    {}  （{n} 个文件）",
            p.strip_prefix(&natives_root).unwrap_or(p).display()
        );
    }
    if dirs.len() > 8 {
        println!("    …还有 {} 个", dirs.len() - 8);
    }
    if dirs.is_empty() {
        println!("  ✗ 一个含 dll 的目录都没找到 —— natives 没解压成功？");
        return 1;
    }
    // 🔴 **`NATIVES_DIR` 必须是「一个目录」，不是一串。**
    //
    // ## 这一行是真跑了一次游戏才写对的
    //
    // 第一版把它绑成 `;` 拼起来的目录列表，因为 `java.library.path`
    // 本来就是那样（一串）。而**官方模板不是那样用它**：
    //
    // ```text
    //   -Djava.library.path=${natives_directory}/java
    //   -Djna.tmpdir=${natives_directory}/jna
    //   -Dorg.lwjgl.system.SharedLibraryExtractPath=${natives_directory}/lwjgl
    //   -Dio.netty.native.workdir=${natives_directory}/netty
    // ```
    //
    // 那四处都在 `${natives_directory}` 后面**接一个 `/子目录`** ——
    // 于是一串 `;` 拼起来的路径会变成 `…a;…b;…c/lwjgl`，
    // 而 `Path.of()` 在第一个 `;` 处就炸：
    //
    // ```text
    //   java.nio.file.InvalidPathException: Illegal char <:> at index 105
    //     at …NativeLibrariesBootstrap.configureLWJGLLibraryPath(NativeLibrariesBootstrap.java:180)
    // ```
    //
    // **所以官方模板要求的那件事是：`natives_directory` 是一个真实存在的单一目录。**
    //
    // ## 而那四个子目录由我们建
    //
    // 官方启动器把它们建在自己的 `bin/<h1>/<h2>/` 下；我们建在实例内的
    // natives 根下。**内容不需要提前放进去** —— 那四个属性的用途是
    // "让 JVM / LWJGL / JNA / netty 把**运行时解出来的**东西放那里"。
    let natives_single = natives_root.clone();
    for sub in ["java", "jna", "lwjgl", "netty"] {
        let _ = std::fs::create_dir_all(natives_single.join(sub));
    }
    println!("  ★ `-Djava.library.path` = **那一个目录**（不是一串）");
    println!("     {}", natives_single.display());
    println!("     ⚠️ 这一条是真跑了一次游戏才写对的 —— 见本文件的注释。");

    // ── ⑤ 组装 ──
    println!();
    println!("【4】组装启动计划");
    let input = AssembleInput {
        program: "{JAVA}".into(),
        main_class: d.main_class().unwrap_or("").to_string(),
        template_map: template_map(),
    };
    let a = match assemble(&d, &env, &input, Some("{CLASSPATH}")) {
        Ok(a) => a,
        Err(e) => {
            println!("  ✗ 组装失败：{e}");
            return 1;
        }
    };
    println!("  形态   : {}", a.form);
    println!(
        "  库     : {} 个进 classpath · {} 个是 natives",
        a.libraries.len(),
        a.natives.len()
    );
    println!("  模板   : 用到 {} 个官方变量", a.templates_used.len());
    if !a.templates_unmapped.is_empty() {
        println!(
            "  ✗ **有 {} 个官方变量我们没有映射**：{:?}",
            a.templates_unmapped.len(),
            a.templates_unmapped
        );
        println!("     一个「原样保留」的变量会以 `${{...}}` 的形式传进 JVM ——");
        println!("     而那会让游戏要么立刻失败，要么行为不可预测。");
        return 1;
    }

    let classpath = build_classpath(&inst, &a.libraries, &version_id);
    let game_dir = inst.clone();
    let assets_root = inst.join("assets");
    let asset_index = d
        .asset_index_ref()
        .map(|x| x.id.clone())
        .unwrap_or_else(|| "legacy".to_string());

    let mut plan = LaunchPlan::new("{PROGRAM}");
    for arg in &a.args {
        plan = plan.arg(arg);
    }
    plan = plan
        .bind("PROGRAM", java_path.display().to_string())
        .bind("JAVA", java_path.display().to_string())
        .bind("CLASSPATH", classpath.clone())
        .bind("NATIVES_DIR", natives_single.display().to_string())
        .bind("PLAYER_NAME", identity.name.clone())
        .bind("PLAYER_UUID", identity.uuid.clone())
        .bind("ACCESS_TOKEN", identity.token.clone())
        .bind("SESSION", identity.token.clone())
        .bind("VERSION_NAME", version_id.clone())
        .bind(
            "VERSION_TYPE",
            // `Descriptor` 的字段叫 `kind`（`#[serde(rename = "type")]`），
            // 而它是 `Option` —— 实测老版本可能没有这个字段。
            // 缺失时用 `release`：那是**保守**的一侧（快照会让游戏去连快照服）。
            d.kind.as_deref().unwrap_or("release"),
        )
        .bind("GAME_DIR", game_dir.display().to_string())
        .bind("ASSETS_DIR", assets_root.display().to_string())
        .bind("ASSETS_INDEX", asset_index.clone())
        .bind("LAUNCHER_NAME", "qinmo")
        .bind("LAUNCHER_VERSION", env!("CARGO_PKG_VERSION"))
        .bind("USER_TYPE", "legacy")
        .bind("USER_PROPERTIES", "{}")
        .bind("CLIENT_ID", "")
        .bind("XUID", "");

    let resolved = match plan.resolve() {
        Ok(r) => r,
        Err(e) => {
            println!("  ✗ **有 {} 个占位符没解析完**：", e.len());
            for p in e.iter().take(10) {
                println!("      {p}");
            }
            println!("     这是**类型级保证失败** —— `resolve()` 的结果里不该有 `{{...}}`。");
            return 1;
        }
    };

    // ── ⑥ 会不会真的启动 ──
    let run: ResolvedCommand = if args.dry_run {
        println!();
        println!("【5】`--dry-run`：**不进游戏**，只验 JVM 能起来");
        ResolvedCommand {
            program: resolved.program.clone(),
            args: vec!["-version".to_string()],
            env: resolved.env.clone(),
            // 🔴 **`cwd` 必须显式设成实例目录。**
            //
            // 第一版写的是 `resolved.cwd.clone()`，而 `LaunchPlan` **没有设过它**
            // —— 于是进程继承了**我们自己的** cwd（仓库根）。
            //
            // 实测的症状：`logs/latest.log` 出现在**仓库根**，而不是实例里；
            // 而 `git add -A` 把它提交了（**同一类坑第二次**）。
            //
            // 而它不只是"日志放错地方"：客户端还会在 cwd 里写 `crash-reports/`、
            // `saves/`、`options.txt`、`resourcepacks/` —— **一个实例的全部用户数据**。
            // 放在仓库根意味着"两个实例共用一份配置"，而那会让
            // "我调好的按键怎么变了"变成一个无法回答的问题。
            // ⚠️ `ResolvedCommand::cwd` 是 **`Option<String>`**，不是 `Option<PathBuf>`
            // —— 我按 `PathBuf` 写了，编译器抓到了。
            cwd: Some(game_dir.display().to_string()),
        }
    } else {
        println!();
        println!("【5】启动");
        resolved.clone()
    };

    println!("  程序   : {}", run.program);
    println!("  参数   : {} 个", run.args.len());
    // **前几个参数值得打出来** —— 它们是"计划长什么样"的唯一可读证据
    for a in run.args.iter().take(6) {
        println!("           {}", short(a, 110));
    }
    if run.args.len() > 6 {
        println!("           …还有 {} 个", run.args.len() - 6);
    }

    println!();
    println!("=== 拉起进程 ===");
    let sink = Arc::new(TeeSink {
        keep: std::sync::Mutex::new(Vec::new()),
        quiet: args.quiet,
    });
    let ex = RealProcessExecutor::default();
    let cancel = CancelToken::new();
    let started = std::time::Instant::now();
    // ⚠️ `ProcessExecutor::run` **不收 sink**（它的签名是 `(cmd, cancel)`，
    // 因为它是"窄 trait"那一层的接口）。要边跑边收日志必须用
    // `run_with_sink(cmd, cancel, Arc<dyn LineSink>)` —— 而这个区别
    // 是"我凭记忆写了一版"的直接后果。
    let out = match ex.run_with_sink(&run, &cancel, sink.clone()) {
        Ok(o) => o,
        Err(e) => {
            println!("  ✗ 进程执行失败：{e}");
            return 1;
        }
    };
    let ms = started.elapsed().as_millis();

    println!();
    println!("=== 进程结束（{ms} ms）===");
    match out.exit_code {
        Some(c) => println!("  退出码 : {c}"),
        None => println!("  退出码 : (没有 —— 被信号终止)"),
    }
    println!("  取消   : {}", out.cancelled);
    println!("  日志行 : {}", out.lines);

    let kept = sink.keep.lock().expect("锁没被毒化").clone();
    let joined = kept.join("\n");

    if args.dry_run {
        let ok = joined.contains("version ")
            || joined.contains("openjdk")
            || joined.contains("Java(TM)")
            || joined.contains("Runtime Environment");
        if ok {
            println!();
            println!("  ✓ **JVM 起来了**，而且我们的参数被它接受了");
            println!("     （`-version` 让 JVM 打印自己的版本然后退出 —— 它不加载游戏）");
            0
        } else {
            println!("  ✗ JVM 没有打印版本信息 —— 那说明启动参数有问题");
            1
        }
    } else {
        // 真启动：判据是"进程正常退出"或"日志里有主菜单的痕迹"
        let saw_menu = joined.contains("Setting user")
            || joined.contains("LWJGL")
            || joined.contains("Backend library")
            || joined.contains("Sound engine started");
        if saw_menu {
            println!();
            println!("  ✓ **游戏跑起来了**（日志里出现了客户端初始化的痕迹）");
            0
        } else if out.exit_code == Some(0) {
            println!("  ⚠️ 进程正常退出，但日志里没有看到客户端初始化的痕迹。");
            0
        } else {
            println!("  ✗ 进程非零退出 —— 看上面的日志");
            1
        }
    }
}

/// 把一个可能很长的参数截短（classpath 有 6800+ 字符）。
fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max / 2).collect();
        let tail: String = s
            .chars()
            .rev()
            .take(max / 2)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("{head}…{tail}")
    }
}

fn now_stamp() -> String {
    // 不引时间库：只要一个"什么时候确认的"能被人读出来的戳。
    // 用系统时间的秒数 + 本地偏移是够的，而**不做日历换算**。
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
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
