//! # 方案 §4.7 的**验收本身**
//!
//! 方案原文：
//!
//! > **这条的验收方式**：造 3 个不同历史版本的样本实例，跑一次新版启动器，
//! > 验证**全部迁移成功且备份存在**。
//! > **M1 出口条件包含此三项**（迁移链 + 迁移前备份 + 失败不阻断）。
//!
//! ## 为什么它必须是一个独立文件，而不是那 12 条单测里的一条
//!
//! 因为**验收的判据与实现的判据不是一回事**：
//!
//! | | 单测 | 本文件 |
//! |---|---|---|
//! | 问的问题 | "这个函数在给定输入下对吗" | **"出口条件的三项都成立吗"** |
//! | 样本来源 | 现场构造的 JSON | **`tests/fixtures/instances/` 里的冻结样本** |
//! | 谁来读 | 改这段代码的人 | **过里程碑评审的人** |
//!
//! 分开还有一个很实际的好处：**这三份样本是只读的"用户老数据"，
//! 而单测里的 JSON 是为了某个断言现场构造的。** 两者的"冻结程度"不同，
//! 放在一起会让人误以为两者都可以随手改。
//!
//! ## 它同时钉住"样本表与目录必须一致"
//!
//! 方案 §4.7 的操作定义要求：每次改格式就冻结一份新样本、且保留旧的。
//! 而"加了样本却忘了加进验收"是一件**很容易发生、且没有任何症状**的事 ——
//! 所以这里有一条测试直接比对**目录内容与样本表**。

use qul_core::migrate::{self, CURRENT_VERSION};
use qul_core::scrub::Scrubber;
use qul_infra::instance::{
    migrate_instance, rollback, InstanceState, MigrationOutcome, BACKUPS, BUILD, DATA_LOCK,
    PATCHES, PERSIST, PROFILE,
};
use std::path::{Path, PathBuf};

/// fixture 的根目录（**住在 `qul-core` 下**：什么格式是内核的知识）。
fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ 下应当有父目录")
        .join("qul-core")
        .join("tests")
        .join("fixtures")
        .join("instances")
}

/// **样本表。** 加一份新样本时**必须**在这里加一行 —— 见下面那条"表与目录一致"的测试。
///
/// 第三列是**预期起始版本**（`None` = 那份样本故意没有版本号，应当被当成 v1）。
const SAMPLES: &[(&str, &str, Option<u32>)] = &[
    ("v1", "无 version 字段的极老数据", None),
    ("v2", "已有 runtime 段的中间版本", Some(2)),
    ("v3", "当前版本", Some(CURRENT_VERSION)),
];

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let p = e.path();
        let t = dst.join(e.file_name());
        if p.is_dir() {
            copy_tree(&p, &t)?;
        } else {
            std::fs::copy(&p, &t)?;
        }
    }
    Ok(())
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "qul-accept-{tag}-{}-{:?}-{}",
        std::process::id(),
        std::thread::current().id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|x| x.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 把一份冻结样本复制成可写的实例目录。
fn instance(sample: &str, tag: &str) -> PathBuf {
    let dst = tmpdir(tag);
    copy_tree(&fixtures_root().join(sample), &dst).unwrap();
    dst
}

// ─────────────────────── 出口条件第一项：迁移链 ───────────────────────

#[test]
fn 验收_三份历史样本全部迁移成功() {
    let mut migrated = 0usize;
    for (name, desc, expect_from) in SAMPLES {
        let d = instance(name, name);
        let before = std::fs::read_to_string(d.join(PROFILE)).unwrap();
        let before_v = serde_json::from_str::<serde_json::Value>(&before).unwrap();
        let detected = migrate::detect_version(&before_v).unwrap();

        // **起始版本必须与样本表一致** —— 否则"验收过了"是假的：
        // 一份本该是 v1 的样本若被误认成 v3，那它根本不会走迁移链。
        match expect_from {
            None => assert_eq!(
                detected, 1,
                "{name}：无版本号应当被视为 v1（方案 §4.7），实际 {detected}"
            ),
            Some(v) => assert_eq!(detected, *v, "{name}：样本表写的起始版本不对"),
        }

        let out = migrate_instance(&d, CURRENT_VERSION, "acceptance", &Scrubber::new());
        match &out {
            MigrationOutcome::Migrated {
                from,
                to,
                steps,
                backup,
                ..
            } => {
                assert_eq!(*from, detected, "{name}：报告的起始版本与探测不符");
                assert_eq!(*to, CURRENT_VERSION, "{name}");
                assert!(backup.is_dir(), "{name}：**备份必须存在**");
                // 步数必须等于版本差 —— 这就是"不许跳版本"的可见证据
                assert_eq!(
                    *steps as u32,
                    CURRENT_VERSION - detected,
                    "{name}：步数应当等于版本差（逐级升级）"
                );
                migrated += 1;
            }
            MigrationOutcome::UpToDate { version } => {
                // 只有"本来就是当前版本"的样本才能走这条
                assert_eq!(
                    *version, CURRENT_VERSION,
                    "{name}：只有最新样本才允许 UpToDate，实际 v{version}"
                );
            }
            MigrationOutcome::Failed {
                reason, guidance, ..
            } => {
                panic!("{name}（{desc}）迁移失败：{reason}\n说明：{guidance}")
            }
        }

        // 迁移后必须是当前版本
        let after = serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(d.join(PROFILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            migrate::detect_version(&after).unwrap(),
            CURRENT_VERSION,
            "{name}"
        );
        assert_eq!(out.state(), InstanceState::Ok, "{name}");
        let _ = std::fs::remove_dir_all(&d);
    }
    assert!(
        migrated >= 2,
        "至少两份样本应当真的走了迁移链（v1 与 v2），实际 {migrated}"
    );
}

// ─────────────────────── 出口条件第二项：迁移前备份 ───────────────────────

#[test]
fn 验收_每份样本都产生了迁移前备份且备份是原始形态() {
    for (name, _desc, expect_from) in SAMPLES {
        // 只有真的需要迁移的样本才该有备份
        let needs_migration = match expect_from {
            None => true,
            Some(v) => *v < CURRENT_VERSION,
        };
        let d = instance(name, name);
        let out = migrate_instance(&d, CURRENT_VERSION, "acceptance", &Scrubber::new());

        if !needs_migration {
            assert!(
                !d.join(BACKUPS).exists(),
                "{name}：不需要迁移就不该产生备份（否则备份区会被无意义地填满）"
            );
            let _ = std::fs::remove_dir_all(&d);
            continue;
        }

        let MigrationOutcome::Migrated { backup, .. } = &out else {
            panic!("{name}：应当迁移成功，实际 {out:?}");
        };

        // **备份里必须是迁移前的形态** —— 这是"先备份再改写"的可见证据。
        // 若顺序搞反（边迁移边备份），这里会读到新形态。
        let backed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(backup.join(PROFILE)).unwrap()).unwrap();
        let backed_v = migrate::detect_version(&backed).unwrap();
        assert!(
            backed_v < CURRENT_VERSION,
            "{name}：备份里应当是**迁移前**的版本，实际 v{backed_v}（顺序搞反了？）"
        );

        // 元数据层三样都在
        assert!(backup.join(DATA_LOCK).is_file(), "{name}：锁定态该在备份里");
        // **`build/` 与 `persist/` 一个都不许进备份**
        assert!(
            !backup.join(BUILD).exists(),
            "{name}：备份不该含 {BUILD}/（可能几十 GB，且可由元数据层重建）"
        );
        assert!(
            !backup.join(PERSIST).exists(),
            "{name}：备份不该含 {PERSIST}/（用户保留数据，迁移不触碰）"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[test]
fn 验收_元数据层的相对结构在备份里被保留() {
    // `patches/import/patch-01.json` 必须还在那个层级。
    // 把它拍平会让回滚产生一个**结构错误的实例** ——
    // 而那种错误在游戏侧表现为"补丁没生效"，极难归因。
    let d = instance("v1", "structure");
    let out = migrate_instance(&d, CURRENT_VERSION, "acceptance", &Scrubber::new());
    let backup = match out {
        MigrationOutcome::Migrated { backup, .. } => backup,
        other => panic!("{other:?}"),
    };
    assert!(
        backup
            .join(PATCHES)
            .join("import")
            .join("patch-01.json")
            .is_file(),
        "patches/ 的相对结构必须保留"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ─────────────────────── 出口条件第三项：失败不阻断 ───────────────────────

#[test]
fn 验收_损坏的实例迁移失败但不阻断且给出可执行说明() {
    let d = tmpdir("corrupt");
    std::fs::write(d.join(PROFILE), "{ 这不是合法 JSON").unwrap();
    let before = std::fs::read_to_string(d.join(PROFILE)).unwrap();

    let out = migrate_instance(&d, CURRENT_VERSION, "acceptance", &Scrubber::new());
    let MigrationOutcome::Failed {
        reason,
        guidance,
        backup,
    } = &out
    else {
        panic!("损坏的实例应当迁移失败，实际 {out:?}");
    };

    // ① 不阻断：它是**一个结局**，不是 panic / 不是 Err 把调用方炸掉
    assert_eq!(out.state(), InstanceState::NeedsManualHandling);
    assert!(out.is_user_visible(), "失败必须让用户看到");

    // ② 可执行说明：三要素缺一不可
    let full = format!("{reason}\n{guidance}");
    assert!(full.contains("没有被破坏"), "必须说清数据还在：{full}");
    assert!(
        full.contains("其余实例不受影响"),
        "必须说清影响范围（否则用户会以为整个启动器坏了）：{full}"
    );

    // ③ 原文件一个字都没变
    assert_eq!(std::fs::read_to_string(d.join(PROFILE)).unwrap(), before);
    // 失败发生在备份之前 ⇒ 没有备份，而说明里要讲清这一点
    assert!(backup.is_none());
    assert!(
        guidance.contains("没有产生备份") || guidance.contains("备份位置"),
        "必须讲清备份的有无：{guidance}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 验收_失败后的实例可以回滚到一个可用状态() {
    // "可回滚"若只写在文档里就只是一句话 —— 所以它必须被**走一遍**。
    let d = instance("v1", "rollback");
    let original = std::fs::read_to_string(d.join(PROFILE)).unwrap();
    let out = migrate_instance(&d, CURRENT_VERSION, "acceptance", &Scrubber::new());
    let backup = match out {
        MigrationOutcome::Migrated { backup, .. } => backup,
        other => panic!("{other:?}"),
    };
    assert_ne!(std::fs::read_to_string(d.join(PROFILE)).unwrap(), original);
    let n = rollback(&d, &backup).unwrap();
    assert!(n >= 2, "至少要复原 profile 与锁定态");
    assert_eq!(
        std::fs::read_to_string(d.join(PROFILE)).unwrap(),
        original,
        "回滚必须逐字节复原"
    );
    // 回滚不该动 build/ 与 persist/（否则用户会丢存档）
    assert!(d.join(BUILD).join("options.txt").is_file());
    assert!(d.join(PERSIST).join("keep.txt").is_file());
    let _ = std::fs::remove_dir_all(&d);
}

// ─────────────────────── 样本表与目录必须一致 ───────────────────────

#[test]
fn 样本表与目录内容一致() {
    // 方案 §4.7 的操作定义：每改一次格式就冻结一份新样本、且保留旧的。
    // 而"加了样本却忘了加进验收"是一件**很容易发生、且没有任何症状**的事。
    // 这条测试把它变成一件**会红**的事。
    let root = fixtures_root();
    let mut on_disk: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    on_disk.sort();

    let mut in_table: Vec<String> = SAMPLES.iter().map(|(n, _, _)| n.to_string()).collect();
    in_table.sort();

    assert_eq!(
        on_disk, in_table,
        "fixture 目录与样本表不一致。\n\
         磁盘上有：{on_disk:?}\n\
         表里有　：{in_table:?}\n\
         加了一份样本就要在 SAMPLES 里加一行 —— 否则它**永远不会被验收**。"
    );
}

#[test]
fn 每份样本都有_profile_且是合法_json() {
    for (name, _d, _v) in SAMPLES {
        let p = fixtures_root().join(name).join(PROFILE);
        assert!(p.is_file(), "{name}：缺 {PROFILE}");
        let text = std::fs::read_to_string(&p).unwrap();
        serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|e| panic!("{name}：{PROFILE} 不是合法 JSON：{e}"));
    }
}

#[test]
fn 样本一旦提交就不该被测试修改() {
    // 方案 §4.7："fixture 一旦提交**永不修改**——它就是'用户的老数据'的替身。"
    // 而"测试顺手改了样本"是一种**会污染所有后续验收**的事故 ——
    // 因为它会让下一个跑测试的人看到一份**已经被改过的「老数据」**。
    //
    // 做法：跑完全部验收路径后，比对样本目录的**内容摘要**没变。
    let root = fixtures_root();
    let before = snapshot(&root);
    for (name, _d, _v) in SAMPLES {
        let d = instance(name, name);
        let _ = migrate_instance(&d, CURRENT_VERSION, "acceptance", &Scrubber::new());
        let _ = std::fs::remove_dir_all(&d);
    }
    let after = snapshot(&root);
    assert_eq!(
        before, after,
        "样本目录在测试过程中被改动了 —— 这会让所有后续验收都建立在**被改过的「老数据」**上"
    );
}

/// 目录内容的摘要（路径 → 长度 + 首尾各 32 字节的简单指纹）。
///
/// 不去引哈希库：这是尖刺阶段，而"文件被改过"这件事
/// 用长度+首尾就足够发现（真正的改法不会只改中间还保持首尾与长度不变）。
fn snapshot(dir: &Path) -> Vec<(String, u64, String)> {
    let mut out = Vec::new();
    fn walk(base: &Path, p: &Path, out: &mut Vec<(String, u64, String)>) {
        let Ok(entries) = std::fs::read_dir(p) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let bytes = std::fs::read(&path).unwrap_or_default();
                let len = bytes.len() as u64;
                let head: String = String::from_utf8_lossy(&bytes[..bytes.len().min(32)]).into();
                let tail_start = bytes.len().saturating_sub(32);
                let tail: String = String::from_utf8_lossy(&bytes[tail_start..]).into();
                out.push((rel, len, format!("{head}|{tail}")));
            }
        }
    }
    walk(dir, dir, &mut out);
    out.sort();
    out
}
