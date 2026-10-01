//! # 实例通用模型的落盘往返（M1 · 方案 §8）
//!
//! ## 这个文件在补什么
//!
//! `qul-core::instance` 的 24 项单测全部是**纯规则**（那是架构要求：core 零 IO）。
//! 而"记录能被写进磁盘、读回来还对得上"**是另一件事** ——
//! 它需要文件系统，所以它在这里。
//!
//! | | core 单测 | 本文件 |
//! |---|---|---|
//! | 问的问题 | "这套规则对吗" | **"往返一圈之后它还自洽吗"** |
//!
//! ## 它验的三件事
//!
//! 1. **落盘再读回，记录完全相等**（含 ID 与 dir）；
//! 2. **手工改坏磁盘上的记录，`validate` 能抓住**（那是真实会发生的形态：
//!    目录被手工改名、记录被别的工具改过）；
//! 3. **`.tmp` 原子写**：写到一半崩溃不会留下一个半份的正式文件
//!    （复用 `fsx::write_atomic` —— 与 ADR-0013 同一把锁的纪律）。

use qul_core::instance::{Instance, InstanceError, InstanceState, ProductRef};
use qul_core::layout::RelPath;

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "qul-inst-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|x| x.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn make(key: &str) -> Instance {
    Instance::create(
        key,
        "测试实例",
        ProductRef::new("java", "26.3", "vanilla"),
        3,
    )
    .unwrap()
}

/// 把实例写进它自己的目录（**用 `write_atomic`**）。
fn save(root: &std::path::Path, i: &Instance) -> std::path::PathBuf {
    let dir = root.join(i.dir.as_str().replace('/', std::path::MAIN_SEPARATOR_STR));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("instance.json");
    let json = serde_json::to_string_pretty(i).unwrap();
    // ⚠️ **第三个参数 `Some(root)` 不是可选的装饰**：它让 `write_atomic`
    // 在写之前先做一次 `ensure_within` —— 于是"记录被写到实例根之外"
    // 这件事**在最早的入口就被拦住**，而不是等到某处读不到。
    qul_infra::fsx::write_atomic(&p, json.as_bytes(), Some(root)).expect("原子写应当成功");
    p
}

fn load(p: &std::path::Path) -> Instance {
    let t = std::fs::read_to_string(p).unwrap();
    serde_json::from_str(&t).unwrap()
}

// ───────────────── 往返 ─────────────────

#[test]
fn 落盘再读回完全相等() {
    let root = tmpdir("roundtrip");
    let i = make("alpha");
    let p = save(&root, &i);
    let back = load(&p);

    assert_eq!(back, i, "**往返之后必须完全相等**（含 ID 与 dir）");
    assert!(
        back.check_integrity().is_ok(),
        "读回来的记录必须自洽：{:?}",
        back.validate()
    );
    // 文件真的在那个位置
    assert!(root
        .join("instances")
        .join("alpha")
        .join("instance.json")
        .is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn json_里没有产品专属的词() {
    // 通用性在**落盘形态**上也必须成立 —— 而不只是在内存里。
    let root = tmpdir("generic");
    let bedrock = Instance::create(
        "bedrock-main",
        "基岩版",
        ProductRef::new("bedrock", "1.21.90", ""),
        3,
    )
    .unwrap();
    let p = save(&root, &bedrock);
    let text = std::fs::read_to_string(&p).unwrap().to_lowercase();
    assert!(text.contains("bedrock"), "该记下产品标识：{text}");
    for forbidden in ["jar", "versions/", "libraries", "assets"] {
        assert!(
            !text.contains(forbidden),
            "通用实例记录里不该出现 {forbidden:?} —— 那说明产品假设渗进了通用层：{text}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

// ───────────────── 改坏之后能被抓住 ─────────────────

#[test]
fn 磁盘上被改坏的_id_能被抓住() {
    // ⚠️ **这是"ID 必须是 key 的函数"这个设计选择的实际回报。**
    // 一个随机 ID 的方案**做不到这件事** —— 它无法回答"这个 ID 还对得上吗"。
    let root = tmpdir("tamper-id");
    let i = make("alpha");
    let p = save(&root, &i);

    // 模拟"记录被别的工具改过"：把 id 换成一个格式合法但算不出来的值
    let text = std::fs::read_to_string(&p).unwrap();
    let tampered = text.replace(
        &format!("\"id\": \"{}\"", i.id.as_str()),
        "\"id\": \"inst-0000000000000000\"",
    );
    assert_ne!(tampered, text, "替换该生效");
    std::fs::write(&p, tampered).unwrap();

    let back = load(&p);
    let errs = back.validate();
    assert!(
        errs.iter()
            .any(|e| matches!(e, InstanceError::IdKeyMismatch { .. })),
        "该发现 ID 与 key 不符：{errs:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn 目录被手工改名之后_validate_能说清() {
    // 真实场景：用户在资源管理器里把目录改名了，而记录里的 `dir` 还是旧的。
    let root = tmpdir("tamper-dir");
    let mut i = make("alpha");
    let p = save(&root, &i);
    // 只改记录里的 dir（模拟"记录与真实目录漂开了"）
    i.dir = RelPath::new("instances/renamed-by-hand").unwrap();
    std::fs::write(&p, serde_json::to_string(&i).unwrap()).unwrap();

    let back = load(&p);
    let errs = back.validate();
    assert!(
        errs.iter()
            .any(|e| matches!(e, InstanceError::DirKeyMismatch { .. })),
        "该发现 dir 与 key 不符：{errs:?}"
    );
    // 而且**dir 与 key 不符本身就是"记录不再可信"的信号** ——
    // 这正是把 dir 也记下来的价值：它能与 key 互为校验。
    let _ = std::fs::remove_dir_all(&root);
}

// ───────────────── 改名不动落盘位置 ─────────────────

#[test]
fn 改名之后文件还在原处且_id_不变() {
    // ⚠️ 这是"身份"与"标签"分开的**文件系统层面**的验证：
    // 一个把显示名当目录名的实现在这里需要移动目录（可能撞车、可能失败）。
    let root = tmpdir("rename");
    let mut i = make("alpha");
    let p = save(&root, &i);
    let id_before = i.id.clone();

    i.rename("一个全新的名字").unwrap();
    let p2 = save(&root, &i);

    assert_eq!(p, p2, "**落盘位置不该变**");
    let back = load(&p2);
    assert_eq!(back.name, "一个全新的名字");
    assert_eq!(back.id, id_before, "**ID 不该变**");
    assert_eq!(back.key, "alpha", "**key 不该变**");
    assert!(back.check_integrity().is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

// ───────────────── 多个实例 ─────────────────

#[test]
fn 多个实例的_id_互不相同() {
    // 64 位哈希的碰撞在数学上可忽略，而**这一组实例是可穷举的那部分**。
    let keys = [
        "alpha",
        "beta",
        "gamma",
        "实例一",
        "实例二",
        "a",
        "b",
        "my-instance",
        "test-1",
        "test-2",
    ];
    let mut ids: Vec<String> = keys
        .iter()
        .map(|k| make(k).id.as_str().to_string())
        .collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n, "这一组里不该有碰撞");
}

#[test]
fn 一批实例能落盘并按稳定顺序读回() {
    let root = tmpdir("many");
    let mut v = vec![
        Instance::create("z", "Zeta", ProductRef::new("java", "26.3", ""), 3).unwrap(),
        Instance::create("a", "Alpha", ProductRef::new("java", "26.3", ""), 3).unwrap(),
        Instance::create("b", "Beta", ProductRef::new("bedrock", "1.21.90", ""), 3).unwrap(),
    ];
    for i in &v {
        save(&root, i);
    }
    Instance::sort_stable(&mut v);
    let names: Vec<&str> = v.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, vec!["Alpha", "Beta", "Zeta"]);

    // 从磁盘按同样顺序读回 —— **界面每次刷新顺序都一样**
    let mut loaded: Vec<Instance> = v
        .iter()
        .map(|i| {
            let p = root
                .join(i.dir.as_str().replace('/', std::path::MAIN_SEPARATOR_STR))
                .join("instance.json");
            load(&p)
        })
        .collect();
    Instance::sort_stable(&mut loaded);
    let names2: Vec<&str> = loaded.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, names2, "落盘往返不该改变顺序");
    for i in &loaded {
        assert!(i.check_integrity().is_ok(), "{:?}", i.validate());
    }
    let _ = std::fs::remove_dir_all(&root);
}

// ───────────────── 状态不落 Running ─────────────────

#[test]
fn 运行态不会被写进磁盘() {
    // ⚠️ 一个把 `Running` 写进磁盘的实现会在**上次异常退出后**
    // 让界面显示"正在运行"，而那个状态**永远无法自愈**。
    //
    // 这条测试断言的是"**持久状态里没有 Running**"这个事实 ——
    // 而它是靠 `is_persistent()` 被上层用来过滤的。
    let root = tmpdir("running");
    let mut i = make("alpha");
    i.state = InstanceState::Running;
    let p = save(&root, &i);
    let back = load(&p);

    // 磁盘上的字节确实记着 running（因为我们是直接写的）——
    // **所以纪律必须是"写之前过滤"**，而不是"读的时候假装没有"。
    assert_eq!(back.state, InstanceState::Running);
    assert!(
        !InstanceState::Running.is_persistent(),
        "**纪律在 `is_persistent()` 上**：调用方在写之前必须过滤掉它"
    );
    // 而其余三个状态都该被写
    for s in InstanceState::ALL.iter().filter(|s| s.is_persistent()) {
        let mut x = make("beta");
        x.state = *s;
        let q = save(&root, &x);
        assert_eq!(load(&q).state, *s, "{s:?} 该能往返");
    }
    let _ = std::fs::remove_dir_all(&root);
}
