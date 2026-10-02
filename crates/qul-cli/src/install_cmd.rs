//! # `qul install`：真的装一个版本（M3 · 五阶段里的前四个）
//!
//! ## 🔴 它装到哪里 —— 一个必须先说清的决定
//!
//! **它装到 `<数据根>/instances/<版本 id>/`，而不是 `%APPDATA%\.minecraft`。**
//!
//! 理由有两条，而第一条是**不可协商**的：
//!
//! 1. **`%APPDATA%\.minecraft` 是用户真实的官方安装。** 往那里写
//!    可能损坏他正在用的东西 —— 而我们做的是启动器，不是"替换官方启动器"。
//! 2. 那也正是**我们自己的布局**（方案 §5.5 的三层目录）：
//!    每个版本一个实例目录，互不干扰，可以整个删掉。
//!
//! 默认数据根是 `%LOCALAPPDATA%\qinmo`。**它不出现在任何用户可见的文案里** ——
//! 那是一个实现细节，而用户看到的是"实例"。
//!
//! ## 它做的四步（对应五阶段的前四个）
//!
//! ```text
//!   ① 解析   读版本详情（**从本机缓存读，或联网取**）
//!   ② 下载   缺什么下什么（已有且 sha1 正确的跳过）
//!   ③ 校验   逐个 SHA-1
//!   ④ 解压   natives → **实例内**（方案 §8 第 6 项）
//! ```
//!
//! 第 ⑤ 步（启动）在 `qul launch` 里 —— **分开是刻意的**：
//! 装与启动是两件事，而"只装不启动"是一个真实需求（M6 的服务器包）。
//!
//! ## ⚠️ 两处必须写下来的取舍
//!
//! ### 资产不计入 by default
//!
//! 实测 `26.3` 有 **5147 个资产对象**（去重后仍约 5000）。
//! 把它们算进盘点会让它从"毫秒级"变成"要算 5000 多个 SHA-1"。
//! 所以默认**不算资产**，而 `--with-assets` 打开它。
//!
//! ### 版本详情的来源：**先本机缓存，再联网**
//!
//! 本机缓存（官方启动器写下的）是**权威且离线可用**的。
//! 联网取只在缓存没有那个版本时才发生 —— 而那时它会**如实说它在联网**。

// ⚠️ `Descriptor` 曾经在这里，而在 `descriptor_for` 被搬进编排层之后
// 这一层不再直接用它 —— 于是它成了未用的 import。
use qul_core::descriptor::{Env, PlatformTarget, VersionManifest};
use qul_core::retry::CancelToken;
use std::path::PathBuf;

/// 官方清单的 URL。
///
/// ⚠️ 与资产主机**不同的理由**：它是**入口** ——
/// 在拿到清单之前，没有任何元数据能告诉我们它在哪。
const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// 默认数据根。
///
/// **它不是用户可见的概念** —— 用户看到的是"实例"。
/// 放在 `%LOCALAPPDATA%` 而不是 `%APPDATA%`：
/// 后者会随域账户漫游，而几十 GB 的游戏文件**不该被同步**。
pub fn default_data_root() -> PathBuf {
    std::path::Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("qinmo")
}

/// 一个实例的目录。
pub fn instance_dir(data_root: &std::path::Path, version_id: &str) -> PathBuf {
    data_root.join("instances").join(version_id)
}

/// 一个会打印进度的 `StageSink`。
struct CliSink {
    last: std::sync::Mutex<String>,
    last_stage: std::sync::Mutex<String>,
    verbose: bool,
}

impl CliSink {
    fn new(verbose: bool) -> Self {
        Self {
            last: std::sync::Mutex::new(String::new()),
            last_stage: std::sync::Mutex::new(String::new()),
            verbose,
        }
    }
}

impl qul_infra::install::StageSink for CliSink {
    fn stage(&self, stage: qul_infra::install::Stage, message: &str) {
        let mut s = self.last_stage.lock().expect("锁没被毒化");
        let line = format!("  [{}] {message}", stage.key());
        if *s != line || self.verbose {
            println!("{line}");
        }
        *s = line;
    }
    fn files(&self, done: usize, total: usize, current: &str) {
        // **只在整百分比变化时打印** —— 一个每文件都打印的实现在
        // 5000 个资产上会刷出 5000 行，而那让日志不可读。
        //
        // ⚠️ 用 `checked_div` 而不是 `if total == 0 { 0 } else { … }`：
        // clippy 的 `manual_checked_ops` 会拦后者，而它是对的 ——
        // 手写的那版**在 `done * 100` 上还可能溢出**，而 `checked_div`
        // 把"除数非零"这件事表达成一个类型上的事实。
        let pct = (done * 100).checked_div(total).unwrap_or(0);
        let line = format!("  [file] {pct}%  ({done}/{total})  {current}");
        let mut l = self.last.lock().expect("锁没被毒化");
        let prev_pct = l
            .split('%')
            .next()
            .and_then(|s| s.rsplit(' ').next())
            .and_then(|s| s.parse::<usize>().ok());
        if prev_pct != Some(pct) || done == total {
            println!("{line}");
            *l = line;
        }
    }
}

/// **它现在是 `qul_app::install_plan::descriptor_for` 的薄调用。**
///
/// ⚠️ 这里**曾经有一份 55 行的"先缓存再联网"实现** —— 而它被搬进了
/// 编排层，因为**界面也要它**。详见 `crates/qul-app/src/install_plan.rs` 的模块文档。
///
/// 那件事的证据是：这一层现在只提供"本机的 .minecraft 在哪"与
/// "清单 URL 是什么"，而**判断完全在编排层**。
fn descriptor_for(
    version_id: &str,
    transport: &dyn qul_core::http::Transport,
    allow_network: bool,
) -> Result<(qul_core::descriptor::Descriptor, String), String> {
    // ⚠️ 这两样是**调用方该给的**：本机缓存的位置是环境事实，
    // 而清单 URL 是方案 §11.5 那条纪律的**唯一例外**（它是入口）——
    // 而那个例外**由 CLI 持有**，不由编排层硬编码。
    let cache_root =
        std::path::Path::new(&std::env::var("APPDATA").unwrap_or_default()).join(".minecraft");
    qul_app::install_plan::descriptor_for(
        version_id,
        &cache_root,
        transport,
        allow_network,
        MANIFEST_URL,
    )
}

pub struct InstallArgs {
    pub version: Option<String>,
    pub data_root: Option<PathBuf>,
    pub offline: bool,
    pub with_assets: bool,
    /// **从一份已有的安装里迁移**（官方那份 `.minecraft` 是最常见的来源）
    pub migrate_from: Option<PathBuf>,
    pub verbose: bool,
}

pub fn run_install(args: &InstallArgs) -> i32 {
    println!("=== 安装一个版本（M3 · 五阶段的前四个）===");
    println!();

    let data_root = args.data_root.clone().unwrap_or_else(default_data_root);
    println!("【0】目标");
    println!("  数据根 : {}", data_root.display());
    println!("  ⚠️ **不写 `%APPDATA%\\.minecraft`** —— 那是用户真实的官方安装。");
    println!("     往那里写可能损坏他正在用的东西，而我们是启动器，不是替换品。");
    println!();

    let transport: Box<dyn qul_core::http::Transport> = if args.offline {
        Box::new(qul_infra::http::NoNetwork)
    } else {
        #[cfg(windows)]
        {
            Box::new(qul_infra::winhttp::WinHttpTransport::new())
        }
        #[cfg(not(windows))]
        {
            Box::new(qul_infra::http::PlainHttpTransport::new())
        }
    };
    println!(
        "  传输   : {}",
        if args.offline {
            "**不联网**（NoNetwork）"
        } else {
            "WinHTTP（系统 TLS 栈，零新依赖）"
        }
    );

    // ── 决定装哪个版本 ──
    let version_id = match &args.version {
        Some(v) => v.clone(),
        None => {
            // 从本机清单取最新正式版（**从指针读，不是常量**）
            let mc = std::path::Path::new(&std::env::var("APPDATA").unwrap_or_default())
                .join(".minecraft");
            let mp = mc.join("versions").join("version_manifest_v2.json");
            match std::fs::read_to_string(&mp)
                .ok()
                .and_then(|t| VersionManifest::parse(&t).ok())
            {
                Some(m) => {
                    let r = m.latest.release.clone();
                    println!("  版本   : {r}（本机清单里的最新正式版）");
                    r
                }
                None => {
                    println!("  ✗ 没指定版本，而本机也没有可读的版本清单。");
                    println!("     用法：qul install <版本 id>");
                    return 2;
                }
            }
        }
    };
    let inst = instance_dir(&data_root, &version_id);
    println!("  实例   : {}", inst.display());
    println!();

    // ── ① 解析 ──
    println!("【1】解析版本详情");
    let (d, src) = match descriptor_for(&version_id, transport.as_ref(), !args.offline) {
        Ok(x) => x,
        Err(e) => {
            println!("  ✗ {e}");
            return 1;
        }
    };
    println!("  来源   : {src}");
    println!("  主类   : {}", d.main_class().unwrap_or("(没有)"));
    let jr = d.java_requirement();
    println!(
        "  Java   : {}  （{}）",
        jr.requirement.human(),
        jr.source.as_str()
    );
    if !d.assumptions().is_empty() {
        println!("  假设   :");
        for a in d.assumptions() {
            println!("           {} = {}（{}）", a.field, a.assumed, a.why);
        }
    }
    let env = Env::new(PlatformTarget::windows("10.0.26200", "x86_64"));
    println!();

    // ── 资产索引：**要先拿到那个文件才能解析它** ──
    // ⚠️ 它**只用于打印**。`install()` 会自己从实例目录再读一次同一份文件 ——
    // 把解析结果传进去会让我们有两个真相来源，而它们可以不一致。
    let mut asset_objects: Option<(usize, usize, u64)> = None;
    // 它只为了在**包装完之后**报一次去重比例 —— 见下面的打印。

    if args.with_assets {
        println!("【1b】取资产索引（`--with-assets`）");
        match d.asset_index_ref() {
            Some(ai) => {
                let dest = inst
                    .join("assets")
                    .join("indexes")
                    .join(format!("{}.json", ai.id));
                println!("  索引   : {}  ← {}", ai.id, ai.url);
                if dest.is_file() {
                    println!("  ✓ 本机已有");
                } else if args.offline {
                    println!("  ✗ 本机没有而 `--offline` 不许联网 —— 跳过资产");
                } else {
                    let v = match qul_infra::check::Sha1Verifier::new(&ai.sha1) {
                        Some(v) => v,
                        None => {
                            println!("  ✗ 索引的 sha1 形态不对");
                            return 1;
                        }
                    };
                    let cancel = CancelToken::new();
                    match qul_infra::download::download(
                        transport.as_ref(),
                        &ai.url,
                        &dest,
                        &v,
                        &qul_infra::download::DownloadConfig::default(),
                        &cancel,
                        None,
                    ) {
                        qul_infra::download::DownloadOutcome::Done { bytes, .. } => {
                            println!("  ✓ 下了 {bytes} 字节并校验通过")
                        }
                        other => {
                            println!("  ✗ 取索引失败：{other:?}");
                            return 1;
                        }
                    }
                }
                // 解析它
                match std::fs::read_to_string(&dest)
                    .map_err(|e| e.to_string())
                    .and_then(|t| qul_core::assets::AssetIndex::parse(&t))
                {
                    Ok(idx) => {
                        // ⚠️ **这三个数必须都打印。** 它们是"逻辑名数 ≠ 文件数"
                        // 这个实测事实的直接证据 —— 而只打印第一个数会让
                        // 去重看起来像没有发生。
                        println!(
                            "  ✓ {} 个逻辑名 → **{} 个不同哈希**（去重掉 {} 个，{} 字节）",
                            idx.objects.len(),
                            idx.unique_hashes().len(),
                            idx.objects.len() - idx.unique_hashes().len(),
                            idx.unique_bytes()
                        );
                        asset_objects = Some((
                            idx.objects.len(),
                            idx.unique_hashes().len(),
                            idx.unique_bytes(),
                        ));
                    }
                    Err(e) => {
                        println!("  ✗ 索引解析失败：{e}");
                        return 1;
                    }
                }
            }
            None => println!("  这个版本没有资产索引（老版本用 `assets` 字符串）"),
        }
        // 去重比例 —— **它是"逻辑名数 ≠ 文件数"这个实测事实的直接证据**。
        if let Some((names, hashes, bytes)) = asset_objects {
            println!(
                "  去重   : {names} 个逻辑名 → {hashes} 个文件（少下 {} 个，{bytes} 字节）",
                names.saturating_sub(hashes)
            );
        }
        println!();
    } else {
        println!("【1b】资产：**默认不算**（`--with-assets` 打开）");
        println!("  理由：实测 `26.3` 有 5147 个资产对象 —— 算进来会让盘点");
        println!("        从「毫秒级」变成「要算 5000 多个 SHA-1」。");
        println!("        而**资产是可选内容**：缺了游戏能起，只是没声音没语言。");
        println!();
    }

    // ── ②③④ 五阶段流水线 ──
    println!("【2-4】下载 · 校验 · 解压");
    let cfg = qul_infra::install::InstallConfig {
        offline_only: args.offline,
        include_assets: args.with_assets,
        migrate_from: args.migrate_from.clone(),
        ..Default::default()
    };
    let sink = CliSink::new(args.verbose);
    let cancel = CancelToken::new();
    let started = std::time::Instant::now();
    let out = match qul_infra::install::install(
        &d,
        &env,
        &version_id,
        &inst,
        &cfg,
        transport.as_ref(),
        &cancel,
        &sink,
    ) {
        Ok(o) => o,
        Err(e) => {
            println!();
            println!("  ✗ 安装失败：{e}");
            if let qul_infra::install::InstallError::Checksum { want, got, .. } = &e {
                println!(
                    "     期望 {}…  实际 {}…",
                    &want[..8.min(want.len())],
                    &got[..8.min(got.len())]
                );
                println!("     这通常是下载被截断或中间有代理改写了内容。");
            }
            return 1;
        }
    };
    let ms = started.elapsed().as_millis();
    println!();

    // ── 结果 ──
    println!("【结果】（{ms} ms）");
    let inv = &out.inventory;
    println!("  需要   : {} 个文件", inv.needs.len());
    println!("  已有   : {} 个（**sha1 全部核对过**）", inv.present);
    println!("  新下   : {} 个", out.downloaded);
    if out.migrated > 0 {
        println!(
            "  **迁移** : {} 个（从已有安装搬的，**每一个都过了 SHA-1**）",
            out.migrated
        );
    }
    println!("  解压   : {} 个 natives 文件", out.extracted);
    println!(
        "  缺口   : {} 个文件 / {} 字节",
        inv.missing.len(),
        inv.bytes_to_fetch()
    );
    let by_kind = inv.by_kind();
    let mut kinds: Vec<_> = by_kind.iter().collect();
    kinds.sort();
    for (k, n) in kinds {
        println!("    {k:<14} {n}");
    }
    println!();
    println!("  阶段耗时 :");
    for (k, v) in &out.stage_ms {
        println!("    {k:<10} {v} ms");
    }
    println!();

    if inv.is_complete() {
        println!("  ✓ **这个实例已经完整**（零缺口）");
        println!("     下一步：`qul launch {version_id}`");
        0
    } else if args.offline {
        println!(
            "  ⚠️ 还缺 {} 个文件 —— 而 `--offline` 不许联网。",
            inv.missing.len()
        );
        println!("     去掉 `--offline` 再跑一次就会补齐它们。");
        println!("     **这个状态是正常的，不是失败** —— 它正是离线盘点的用途。");
        0
    } else {
        println!("  ✗ 联网装完之后仍有缺口 —— 那说明有文件没能下下来。");
        for m in inv.missing.iter().take(10) {
            println!("    {} ({})", m.rel, m.kind.key());
        }
        1
    }
}
