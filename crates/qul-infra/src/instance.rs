//! # 实例迁移的落地（M1 · 方案 §4.7）
//!
//! ## 这一层做什么，不做什么
//!
//! | 层 | 做什么 |
//! |---|---|
//! | `qul_core::migrate` | **规则**：迁移链、逐级不跳、版本探测（纯函数，可穷举测试） |
//! | **本模块** | **落地**：备份、原子替换、成败轨迹、实例状态标记 |
//!
//! ## 三条纪律在代码里的落点
//!
//! | 纪律（方案 §4.7 原文） | 落点 |
//! |---|---|
//! | "**强制备份整个实例元数据层**（`profile.json` + 锁定态 + `patches/`，**不含 `build/`**）到实例的备份区" | [`backup_metadata`] |
//! | "迁移失败**不阻断启动器启动**：把该实例标为「需手动处理」，界面给出**可执行说明**（含备份位置与回滚方式）" | [`InstanceState`] / [`MigrationOutcome`] |
//! | "任何会自动改写用户文件的迁移，**必须可回滚**" | [`rollback`] |
//!
//! ## ⚠️ 一条顺序上的硬要求：**先备份，再改写**
//!
//! 这一条看似显然，但它有一个容易搞反的变体：
//! **"先探测需要迁移、备份、再迁移"** 与 **"边迁移边备份"** 看起来差不多，
//! 而后者在**迁移中途失败**时，备份里存的是**半迁移状态** ——
//! 也就是说**回滚回去的仍然是坏数据**。
//!
//! 所以本模块的结构是：
//!
//! ```text
//!     ① 读 profile.json           （不改任何东西）
//!     ② 探测版本 → 排链            （纯计算）
//!     ③ 在内存里跑完整条链         （任何一步失败 ⇒ 磁盘一个字都没变）
//!     ④ 备份元数据层               （此时才动磁盘，且只动 backups/）
//!     ⑤ 原子替换 profile.json      （ADR-0013 的约定）
//! ```
//!
//! **第 ③ 步必须在第 ④ 步之前**，而它的收益是：**备份里存的永远是"迁移前的原始数据"**。

use qul_core::migrate::{self, CURRENT_VERSION};
use qul_core::scrub::Scrubber;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// 被迁移改写的文件（**元数据层**）。
pub const PROFILE: &str = "profile.json";
/// 部署锁定态（**元数据层**）
pub const DATA_LOCK: &str = "data.lock.json";
/// 外部部署声明目录（**元数据层**）
pub const PATCHES: &str = "patches";
/// 运行目录。**明确不在备份范围内** —— 它可能几十 GB，而且可以由前两层重建。
pub const BUILD: &str = "build";
/// 备份区。**固定名字**，因为回滚说明要能告诉用户"去哪个目录找"。
pub const BACKUPS: &str = "backups";
/// 用户保留数据。**迁移永不触碰**（方案 §5.5 的纪律）。
pub const PERSIST: &str = "persist";

/// 迁移的结局。
///
/// **三态而不是 `Result`**，因为"没迁移"是一种**正常**结局（多数实例都已是最新），
/// 而把它塞进 `Ok(())` 会让日志里分不清"迁移了 0 步"与"不需要迁移"。
/// 同理"失败"要带上**可执行信息**，而不是一个错误串。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// 已是最新，什么都没做
    UpToDate { version: u32 },
    /// 迁移成功
    Migrated {
        from: u32,
        to: u32,
        steps: usize,
        /// **备份目录**（回滚说明要用它）
        backup: PathBuf,
        /// 每一步做了什么（给用户看的迁移日志）
        notes: Vec<&'static str>,
    },
    /// 迁移失败 —— **不阻断启动器**，但该实例要标成"需手动处理"
    Failed {
        reason: String,
        /// 备份目录（**可能为 `None`**：失败若发生在备份之前，就没有备份可指）
        backup: Option<PathBuf>,
        /// 给用户的**可执行说明**
        guidance: String,
    },
}

impl MigrationOutcome {
    /// 这个结局要不要让用户看到。
    pub const fn is_user_visible(&self) -> bool {
        matches!(self, MigrationOutcome::Failed { .. })
    }

    /// 实例该被标成什么状态。
    pub const fn state(&self) -> InstanceState {
        match self {
            MigrationOutcome::UpToDate { .. } => InstanceState::Ok,
            MigrationOutcome::Migrated { .. } => InstanceState::Ok,
            MigrationOutcome::Failed { .. } => InstanceState::NeedsManualHandling,
        }
    }

    /// 一行摘要（迁移日志与界面提示都用它）。
    pub fn summary(&self) -> String {
        match self {
            MigrationOutcome::UpToDate { version } => {
                format!("已是最新格式（v{version}）")
            }
            MigrationOutcome::Migrated {
                from,
                to,
                steps,
                backup,
                ..
            } => format!(
                "已从 v{from} 迁移到 v{to}（{steps} 步）；备份在 {}",
                backup.display()
            ),
            MigrationOutcome::Failed { reason, .. } => format!("迁移失败：{reason}"),
        }
    }
}

/// 实例状态（**方案 §4.7 的"需手动处理"**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceState {
    /// 正常
    Ok,
    /// **需手动处理**：迁移失败。界面给出可执行说明（含备份位置与回滚方式）。
    NeedsManualHandling,
}

/// 迁移失败的**可执行说明**。
///
/// 方案 §4.7：*"界面给出**可执行说明**（含备份位置与回滚方式）"*。
///
/// **"可执行"三个字是重点**：只写"迁移失败"是**把问题丢回给用户**。
/// 所以这段文字必须做三件事，且缺一不可：
///
/// | 要素 | 为什么 |
/// |---|---|
/// | **发生了什么** | 用户要先知道严重程度 |
/// | **数据还在哪** | 不知道备份在哪，用户不敢动手 |
/// | **怎么回滚** | 只说"有备份"等于让用户自己猜 |
fn guidance(reason: &str, backup: Option<&Path>) -> String {
    let mut s = String::new();
    s.push_str(&format!("这个实例的格式升级没有成功：{reason}\n"));
    s.push_str("**你的数据没有被破坏** —— 迁移前会先把元数据备份，且失败时不会改写原文件。\n");
    match backup {
        Some(b) => {
            s.push_str(&format!("备份位置：{}\n", b.display()));
            s.push_str(&format!(
                "回滚方式：把上面那个目录里的文件复制回实例目录（覆盖 {PROFILE}）。\
                 若你不确定，请先整份复制一份实例目录再操作。\n"
            ));
        }
        None => {
            s.push_str(
                "本次失败发生在备份之前，所以没有产生备份 —— \
                 也正因为如此，原文件一个字都没有被改动。\n",
            );
        }
    }
    s.push_str("该实例已标为「需手动处理」；**其余实例不受影响**，启动器仍可正常使用。");
    s
}

/// 把**元数据层**备份到实例的备份区。
///
/// ## 为什么明确排除 `build/`
///
/// 三个理由，每条都具体：
/// 1. **它可能几十 GB** —— 每次升级都复制一份，用户的磁盘会被悄悄吃满；
/// 2. **它可由前两层重建** —— 备份它的收益极低（`import/` + `patches/` 就是它的来源）；
/// 3. **它里面有"不可替代的用户数据"**（存档/截图）——
///    但那些数据**不该靠"迁移备份"来保护**，而该靠 §5.5 的"提升到 `persist/`"机制。
///    把它们混进来会让"备份"这个词的含义变模糊。
///
/// 返回备份目录路径。
pub fn backup_metadata(instance_dir: &Path, stamp: &str) -> Result<PathBuf, String> {
    let dest = instance_dir.join(BACKUPS).join(stamp);
    if dest.exists() {
        return Err(format!("备份目录已存在：{}", dest.display()));
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.kind().to_string())?;

    let mut copied = 0usize;
    // ① profile.json
    let p = instance_dir.join(PROFILE);
    if p.is_file() {
        std::fs::copy(&p, dest.join(PROFILE)).map_err(|e| e.kind().to_string())?;
        copied += 1;
    }
    // ② 锁定态
    let l = instance_dir.join(DATA_LOCK);
    if l.is_file() {
        std::fs::copy(&l, dest.join(DATA_LOCK)).map_err(|e| e.kind().to_string())?;
        copied += 1;
    }
    // ③ patches/ 整棵（外部部署声明）
    let patches = instance_dir.join(PATCHES);
    if patches.is_dir() {
        copied += copy_tree(&patches, &dest.join(PATCHES))?;
    }

    // **`build/` 与 `persist/` 都不复制**，而这一条要有痕迹：
    // "备份里为什么没有我的存档"是一个用户一定会问的问题。
    let note = format!(
        "本备份只含元数据层（{PROFILE} / {DATA_LOCK} / {PATCHES}/），共 {copied} 个文件。\n\
         不含 {BUILD}/（可由元数据层重建，且可能极大）与 {PERSIST}/（用户保留数据，迁移不触碰）。\n\
         回滚就是把本目录里的内容复制回实例目录。\n"
    );
    std::fs::write(dest.join("README-备份说明.txt"), note).map_err(|e| e.kind().to_string())?;

    Ok(dest)
}

/// 递归复制一棵目录树，返回复制的文件数。
///
/// **保留相对结构**：`patches/import/a.json` 必须还在 `patches/import/a.json`。
/// 把它拍平会让回滚产生一个**结构错误的实例** —— 而那种错误在游戏侧表现为"补丁没生效"，
/// 极难归因。
fn copy_tree(src: &Path, dst: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(dst).map_err(|e| e.kind().to_string())?;
    let mut n = 0usize;
    let entries = std::fs::read_dir(src).map_err(|e| e.kind().to_string())?;
    for e in entries {
        let e = e.map_err(|x| x.kind().to_string())?;
        let p = e.path();
        let target = dst.join(e.file_name());
        if p.is_dir() {
            n += copy_tree(&p, &target)?;
        } else if p.is_file() {
            std::fs::copy(&p, &target).map_err(|x| x.kind().to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

/// **迁移一个实例。** 顺序见模块文档（内存里跑完 → 才备份 → 才替换）。
///
/// `stamp` 是备份目录名（调用方给，便于测试确定；生产用时间戳）。
pub fn migrate_instance(
    instance_dir: &Path,
    to: u32,
    stamp: &str,
    scrubber: &Scrubber,
) -> MigrationOutcome {
    let profile_path = instance_dir.join(PROFILE);

    // ① 读
    let raw = match std::fs::read_to_string(&profile_path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // **没有 profile.json 不是"迁移失败"** —— 那是一个还没初始化的实例。
            // 把它报成失败会让界面提示一个不存在的问题。
            return MigrationOutcome::UpToDate {
                version: CURRENT_VERSION,
            };
        }
        Err(e) => {
            return MigrationOutcome::Failed {
                reason: format!("读不到 {}：{}", PROFILE, e.kind()),
                backup: None,
                guidance: guidance(&format!("读不到 {}", PROFILE), None),
            };
        }
    };

    let value: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            // **解析失败也不阻断**，但要说清是"文件坏了"而不是"迁移坏了"。
            return MigrationOutcome::Failed {
                reason: format!("{} 不是合法 JSON：{e}", PROFILE),
                backup: None,
                guidance: guidance(&format!("{} 不是合法 JSON", PROFILE), None),
            };
        }
    };

    // ② 探测 + 排链
    let from = match migrate::detect_version(&value) {
        Ok(v) => v,
        Err(e) => {
            return MigrationOutcome::Failed {
                reason: e.clone(),
                backup: None,
                guidance: guidance(&e, None),
            };
        }
    };
    let chain = match migrate::plan_chain(from, to) {
        Ok(c) => c,
        Err(e) => {
            return MigrationOutcome::Failed {
                reason: e.clone(),
                backup: None,
                guidance: guidance(&e, None),
            };
        }
    };
    if chain.is_empty() {
        return MigrationOutcome::UpToDate { version: from };
    }
    let notes = migrate::describe_chain(&chain);

    // ③ **在内存里跑完整条链** —— 任何一步失败 ⇒ 磁盘一个字都没变
    let (migrated, steps) = match migrate::migrate(value, to) {
        Ok(x) => x,
        Err(e) => {
            return MigrationOutcome::Failed {
                reason: e.clone(),
                backup: None,
                guidance: guidance(&e, None),
            };
        }
    };

    // ③′ **校验产出的版本号确实是目标版本。**
    // 少这一条的话，"迁移函数忘了写版本号"会产出一个**版本号仍是旧值**的文件，
    // 于是**每次启动都会再迁一遍** —— 而重复迁移的结果取决于迁移自己。
    match migrate::detect_version(&migrated) {
        Ok(v) if v == to => {}
        Ok(v) => {
            return MigrationOutcome::Failed {
                reason: format!("迁移后版本号是 v{v}，应当是 v{to}"),
                backup: None,
                guidance: guidance("迁移产出的版本号不对", None),
            };
        }
        Err(e) => {
            return MigrationOutcome::Failed {
                reason: e.clone(),
                backup: None,
                guidance: guidance(&e, None),
            };
        }
    }

    // ④ 备份（此时才动磁盘，且只动 backups/）
    let backup = match backup_metadata(instance_dir, stamp) {
        Ok(b) => b,
        Err(e) => {
            // **备份失败必须中止迁移。** 这一条不能宽容：
            // 没有备份就改写，等于把"可回滚"这个承诺取消掉 ——
            // 而方案 §4.7 把它写成硬要求。
            let reason = format!("备份元数据失败，已中止迁移：{e}");
            return MigrationOutcome::Failed {
                reason: reason.clone(),
                backup: None,
                guidance: guidance(&reason, None),
            };
        }
    };

    // ⑤ 原子替换（ADR-0013 的约定：读、写、恢复共享同一把锁）
    let text = match serde_json::to_string_pretty(&migrated) {
        Ok(t) => t,
        Err(e) => {
            return MigrationOutcome::Failed {
                reason: format!("迁移结果无法序列化：{e}"),
                backup: Some(backup),
                guidance: guidance("迁移结果无法序列化", None),
            };
        }
    };
    // 迁移产出的文本**也要过脱敏**：`last_played` 与将来的字段可能含路径或用户名，
    // 而它会出现在诊断包里。
    let (clean, _) = scrubber.scrub(&text);
    if let Err(e) = crate::fsx::write_atomic(&profile_path, clean.as_bytes(), None) {
        return MigrationOutcome::Failed {
            reason: format!("写入迁移结果失败：{e}"),
            backup: Some(backup.clone()),
            guidance: guidance(&format!("写入迁移结果失败：{e}"), Some(backup.as_path())),
        };
    }

    MigrationOutcome::Migrated {
        from,
        to,
        steps,
        backup,
        notes,
    }
}

/// **回滚**：把备份目录里的元数据层复制回实例目录。
///
/// 方案 §4.7 要求"必须可回滚"，而"可回滚"若只写在文档里就只是一句话 ——
/// 所以它必须是一个**能被测试调用的函数**。
///
/// 它**不动 `build/` 与 `persist/`**：备份里本来就没有它们，
/// 而"回滚时顺手删掉运行目录"会让用户丢掉存档。
pub fn rollback(instance_dir: &Path, backup_dir: &Path) -> Result<usize, String> {
    if !backup_dir.is_dir() {
        return Err(format!("备份目录不存在：{}", backup_dir.display()));
    }
    let mut restored = 0usize;
    for name in [PROFILE, DATA_LOCK] {
        let src = backup_dir.join(name);
        if src.is_file() {
            std::fs::copy(&src, instance_dir.join(name)).map_err(|e| e.kind().to_string())?;
            restored += 1;
        }
    }
    let ps = backup_dir.join(PATCHES);
    if ps.is_dir() {
        restored += copy_tree(&ps, &instance_dir.join(PATCHES))?;
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let d = std::env::temp_dir().join(format!("qul-mig-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 把一个 fixture 复制成可写的实例目录。
    ///
    /// ⚠️ **fixture 住在 `qul-core/tests/fixtures/`，不是本 crate 的目录。**
    /// 第一版用了 `CARGO_MANIFEST_DIR`（= `crates/qul-infra`），于是全部 9 条
    /// 依赖 fixture 的测试一起失败 —— 而 `CARGO_MANIFEST_DIR` 的语义
    /// 在"跨 crate 用同一份 fixture"时**不指向你以为的那个 crate**。
    ///
    /// 放在 `qul-core` 下的理由：**"什么格式"是内核的知识**，
    /// 而 fixture 就是那个知识的历史快照；迁移的实现方（本 crate）只是消费者。
    fn instance_from_fixture(v: &str, tag: &str) -> PathBuf {
        let core_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ 下应当有父目录")
            .join("qul-core");
        let src = core_dir
            .join("tests")
            .join("fixtures")
            .join("instances")
            .join(v);
        assert!(
            src.is_dir(),
            "找不到 fixture：{} —— fixture 住在 qul-core/tests/fixtures/instances/",
            src.display()
        );
        let dst = tmpdir(tag);
        copy_tree(&src, &dst).unwrap();
        dst
    }

    fn read_profile(dir: &Path) -> Value {
        let t = std::fs::read_to_string(dir.join(PROFILE)).unwrap();
        serde_json::from_str(&t).unwrap()
    }

    // ───────────────── 三条纪律的验收 ─────────────────

    #[test]
    fn v1_fixture_能迁移到当前版本且备份存在() {
        // 这就是方案 §4.7 的**验收方式**：造历史版本样本，跑一次，
        // 验证"全部迁移成功**且备份存在**"。
        let d = instance_from_fixture("v1", "accept-v1");
        let before = read_profile(&d);
        assert_eq!(migrate::detect_version(&before).unwrap(), 1, "v1 无版本号");

        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        match &out {
            MigrationOutcome::Migrated {
                from,
                to,
                steps,
                backup,
                ..
            } => {
                assert_eq!((*from, *to, *steps), (1, CURRENT_VERSION, 2));
                assert!(backup.is_dir(), "备份目录必须存在：{}", backup.display());
                // **备份里必须是迁移前的原始数据**
                let backed = serde_json::from_str::<Value>(
                    &std::fs::read_to_string(backup.join(PROFILE)).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    migrate::detect_version(&backed).unwrap(),
                    1,
                    "备份里应当是**迁移前**的版本"
                );
                assert_eq!(backed["memory_mb"], 2048, "备份里应当是老字段形态");
            }
            other => panic!("应当迁移成功，实际 {other:?}"),
        }

        // 迁移后的文件
        let after = read_profile(&d);
        assert_eq!(migrate::detect_version(&after).unwrap(), CURRENT_VERSION);
        assert_eq!(after["runtime"]["memory_mb"], 2048, "用户的内存值必须保住");
        assert!(after.get("memory_mb").is_none(), "旧键必须被删掉：{after}");

        // **`build/` 与 `persist/` 一个字都没被碰**
        assert!(d.join(BUILD).join("options.txt").is_file());
        assert!(d.join(BUILD).join("saves/world/level.dat").is_file());
        assert!(d.join(PERSIST).join("keep.txt").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 备份不含_build_也不含_persist_但要说清为什么() {
        let d = instance_from_fixture("v1", "backup-scope");
        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        let backup = match out {
            MigrationOutcome::Migrated { backup, .. } => backup,
            other => panic!("{other:?}"),
        };
        assert!(!backup.join(BUILD).exists(), "备份不该含 build/");
        assert!(!backup.join(PERSIST).exists(), "备份不该含 persist/");
        // patches/ 整棵都要在
        assert!(backup.join(PATCHES).join("import/patch-01.json").is_file());
        assert!(backup.join(DATA_LOCK).is_file());
        // **"为什么没有我的存档"是一个用户一定会问的问题** → 备份里要有说明
        let note = std::fs::read_to_string(backup.join("README-备份说明.txt")).unwrap();
        assert!(note.contains(BUILD), "{note}");
        assert!(note.contains("回滚"), "说明必须讲怎么回滚：{note}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 失败不阻断且给出可执行说明() {
        let d = tmpdir("fail");
        std::fs::write(d.join(PROFILE), "这不是 JSON").unwrap();
        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        let MigrationOutcome::Failed {
            guidance, backup, ..
        } = &out
        else {
            panic!("应当是失败，实际 {out:?}");
        };
        assert_eq!(out.state(), InstanceState::NeedsManualHandling);
        assert!(out.is_user_visible());
        assert!(backup.is_none(), "失败发生在备份之前，不该有备份");
        // "可执行"三个字是重点：必须说清数据还在、失败发生在哪一步
        assert!(guidance.contains("没有被破坏"), "{guidance}");
        assert!(guidance.contains("其余实例不受影响"), "{guidance}");
        // **原文件一个字都没变**
        assert_eq!(
            std::fs::read_to_string(d.join(PROFILE)).unwrap(),
            "这不是 JSON"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 回滚能把实例复原() {
        // 方案要求"必须可回滚"，而"可回滚"若只写在文档里就只是一句话。
        let d = instance_from_fixture("v1", "rollback");
        let original = std::fs::read_to_string(d.join(PROFILE)).unwrap();

        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        let backup = match out {
            MigrationOutcome::Migrated { backup, .. } => backup,
            other => panic!("{other:?}"),
        };
        assert_ne!(
            std::fs::read_to_string(d.join(PROFILE)).unwrap(),
            original,
            "迁移后应当变了"
        );

        let n = rollback(&d, &backup).unwrap();
        assert!(n >= 2, "至少要复原 profile 与锁定态，实际 {n}");
        assert_eq!(
            std::fs::read_to_string(d.join(PROFILE)).unwrap(),
            original,
            "回滚必须逐字节复原"
        );
        // 回滚不该动 build/ 与 persist/
        assert!(d.join(BUILD).join("options.txt").is_file());
        assert!(d.join(PERSIST).join("keep.txt").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    // ───────────────── 其余行为 ─────────────────

    #[test]
    fn 已是最新时什么都不做() {
        let d = instance_from_fixture("v3", "uptodate");
        let before = std::fs::read_to_string(d.join(PROFILE)).unwrap();
        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        assert!(matches!(out, MigrationOutcome::UpToDate { .. }), "{out:?}");
        assert_eq!(out.state(), InstanceState::Ok);
        assert!(!out.is_user_visible(), "正常状态不该打扰用户");
        assert_eq!(std::fs::read_to_string(d.join(PROFILE)).unwrap(), before);
        assert!(!d.join(BACKUPS).exists(), "没迁移就不该产生备份");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 没有_profile_的实例不算失败() {
        // 一个还没初始化的实例**不是**"迁移失败" ——
        // 把它报成失败会让界面提示一个不存在的问题。
        let d = tmpdir("empty");
        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        assert!(matches!(out, MigrationOutcome::UpToDate { .. }), "{out:?}");
        assert_eq!(out.state(), InstanceState::Ok);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn v2_fixture_走一步就到位() {
        let d = instance_from_fixture("v2", "v2");
        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        match out {
            MigrationOutcome::Migrated {
                from, to, steps, ..
            } => {
                assert_eq!((from, to, steps), (2, CURRENT_VERSION, 1));
            }
            other => panic!("{other:?}"),
        }
        let after = read_profile(&d);
        assert_eq!(after["runtime"]["memory_mb"], 4096, "用户的值要保住");
        assert_eq!(after["mod_loader"]["kind"], "fabric");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 未来版本的实例被拒绝而不是降级改写() {
        let d = tmpdir("future");
        std::fs::write(
            d.join(PROFILE),
            format!("{{\"version\": {}}}", CURRENT_VERSION + 3),
        )
        .unwrap();
        let before = std::fs::read_to_string(d.join(PROFILE)).unwrap();
        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        assert!(matches!(out, MigrationOutcome::Failed { .. }), "{out:?}");
        assert_eq!(
            std::fs::read_to_string(d.join(PROFILE)).unwrap(),
            before,
            "**绝不能降级改写新数据**"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 备份目录已存在时中止而不是覆盖() {
        // 覆盖一份已有备份等于**销毁用户唯一的一份旧数据**。
        let d = instance_from_fixture("v1", "dup-backup");
        std::fs::create_dir_all(d.join(BACKUPS).join("auto-1")).unwrap();
        let before = std::fs::read_to_string(d.join(PROFILE)).unwrap();
        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        let MigrationOutcome::Failed { guidance, .. } = &out else {
            panic!("应当中止，实际 {out:?}");
        };
        assert!(guidance.contains("没有被破坏"), "{guidance}");
        assert_eq!(
            std::fs::read_to_string(d.join(PROFILE)).unwrap(),
            before,
            "中止时不该改写 profile"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 迁移产出的文本会过脱敏管道() {
        // `last_played` 与将来的字段可能含路径或用户名，而迁移结果会进诊断包。
        let d = tmpdir("scrub");
        let home = r"C:\Users\someone";
        std::fs::write(
            d.join(PROFILE),
            "{\"version\":1,\"note\":\"C:\\\\Users\\\\someone\\\\x\"}",
        )
        .unwrap();
        let sb = Scrubber::new().path(home, qul_core::scrub::MASK_USERPROFILE);
        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &sb);
        assert!(matches!(out, MigrationOutcome::Migrated { .. }), "{out:?}");
        let text = std::fs::read_to_string(d.join(PROFILE)).unwrap();
        assert!(!text.to_lowercase().contains("someone"), "{text}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 结局摘要能说清发生了什么() {
        let d = instance_from_fixture("v1", "summary");
        let out = migrate_instance(&d, CURRENT_VERSION, "stamp-x", &Scrubber::new());
        let s = out.summary();
        assert!(s.contains("v1"), "{s}");
        assert!(s.contains("备份"), "{s}");
        assert!(s.contains("stamp-x"), "摘要要指出备份位置：{s}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 每一步的说明会随结局带出来() {
        let d = instance_from_fixture("v1", "notes");
        let out = migrate_instance(&d, CURRENT_VERSION, "auto-1", &Scrubber::new());
        match out {
            MigrationOutcome::Migrated { notes, .. } => {
                assert_eq!(notes.len(), 2);
                assert!(notes[0].contains("runtime"), "{notes:?}");
            }
            other => panic!("{other:?}"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
