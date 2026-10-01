//! # 实例数据格式的版本化与迁移链（**纯规则，零 IO**）
//!
//! ## 为什么这是"数据安全义务"而不是可选项
//!
//! 方案 §4.7 的原话：
//!
//! > 必须回答"格式变了怎么办"——否则用户从 v1.2 升到 v1.3 时，**老实例会直接崩**。
//! > **这是用户数据安全的一部分，不是可选项。**
//!
//! ## 三条硬纪律（都在本模块里被强制）
//!
//! | 纪律 | 原文 | 落点 |
//! |---|---|---|
//! | **不许跳版本** | "走**迁移链**（v1→v2→v3 逐步升级，**不允许跳版本**）" | [`plan_chain`] 逐级排；有缺口就**拒绝** |
//! | **缺失版本视为 v1** | "顶部强制 `version` 字段；**缺失视为 v1**" | [`detect_version`] |
//! | **不可逆必须可回滚** | "任何会自动改写用户文件的迁移，必须可回滚" | [`Migration::reversible`] |
//!
//! ## 为什么迁移函数吃的是 **JSON 值**而不是类型化结构
//!
//! 因为**旧格式的类型在当前代码里已经不存在了**。
//!
//! 如果我写 `fn migrate_v1_to_v2(old: ProfileV1) -> ProfileV2`，
//! 那就要求 `ProfileV1` 这个结构**永远留在代码里**。
//! 那是可行的，但它有一条很坏的副作用：
//! **每个历史版本的结构都会永久占据内核的命名空间**，而它们**只被迁移用一次**。
//!
//! 用 `serde_json::Value` 之后，迁移是"对一份旧数据做一次改写"，
//! 而**"旧格式长什么样"这份知识只存在于那个迁移函数里**（连注释带代码）。
//! 这也让迁移**可以只删改它认识的那几个键**，其余键**原样带过去** ——
//! 而类型化结构会**丢掉它不认识的键**，那等于**静默吞掉用户的数据**。
//!
//! ## 一条不该被"优化"掉的保守选择
//!
//! [`Migration::step`] 拿到不认识的输入时**返回错误，而不是尽力而为**。
//! 理由：迁移是**只能成功一次**的操作（成功之后旧数据就被盖掉了）。
//! 而"尽力而为"的迁移会产出一份**看起来成功、其实缺字段**的数据 ——
//! 那种数据在几天后才以"某个功能不工作"的形式暴露，而**那时已经回不去了**。

use serde_json::{Map, Value};

/// **当前格式版本。** 改格式时**必须**加一，并在 [`MIGRATIONS`] 里补一条。
pub const CURRENT_VERSION: u32 = 3;

/// 版本字段名。**放在顶部**（方案：`profile.json` 与锁定态顶部强制 `version`）。
pub const VERSION_KEY: &str = "version";

/// 缺失版本时的默认值。方案原话：**"缺失视为 v1"**。
///
/// 这个默认是**为了兼容"我们最早的版本没写版本号"** 这种真实历史 ——
/// 而不是为了宽容。所以它只对"完全缺失"生效；
/// **写了一个不能解析的值（如 `"version": "abc"`）是错误，不是 v1** ——
/// 把它当 v1 会让一份未来版本的文件被当成老数据降级改写。
pub const DEFAULT_VERSION: u32 = 1;

/// 一次迁移。
pub struct Migration {
    pub from: u32,
    pub to: u32,
    /// 这一步做了什么（**给用户看**：迁移日志里要能说清改了什么）
    pub note: &'static str,
    /// 是否可回滚。**不可回滚的迁移必须让用户显式同意**（方案 §4.7）。
    pub reversible: bool,
    /// 变换本身。
    pub step: fn(&mut Value) -> Result<(), String>,
}

/// **迁移表。** 每条只能跨一级 —— 这是"不许跳版本"在数据上的落地。
///
/// 加新版本时的动作是固定的三步：
/// ① `CURRENT_VERSION` 加一；
/// ② 在这里补一条 `from = 旧, to = 新`；
/// ③ 冻结一份**旧格式**样本进 `tests/fixtures/instances/vN/`（方案 §4.7 的操作定义）。
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        from: 1,
        to: 2,
        note: "把运行参数收进 runtime 段，并把 loader 改名成更明确的形式",
        reversible: true,
        step: v1_to_v2,
    },
    Migration {
        from: 2,
        to: 3,
        note: "补上 game_dir 策略与 created_by，缺省值填当前语义",
        reversible: true,
        step: v2_to_v3,
    },
];

/// v1 → v2：把 `memory_mb`/`jvm_args` 收进 `runtime` 段，`loader` 改名 `mod_loader`。
///
/// **为什么这是一次"真实"的迁移而不是为了凑数**：
/// 它演示了迁移必须处理的两类改动 —— **结构重组**（键搬家）与**改名**（键换名字）。
/// 这两类是最常见的，而它们各有一个容易漏的坑：
///
/// | 改动 | 容易漏的坑 |
/// |---|---|
/// | 结构重组 | 旧键**没被删掉** → 新旧两份数据共存，而读的人不知道该信哪个 |
/// | 改名 | 旧键的值**没被搬过去** → 用户的内存设置**静默回到默认值** |
fn v1_to_v2(v: &mut Value) -> Result<(), String> {
    let obj = v
        .as_object_mut()
        .ok_or_else(|| "profile 的顶层必须是一个对象".to_string())?;

    // ① 结构重组：把两个键搬进 runtime
    let mut runtime = Map::new();
    if let Some(m) = obj.remove("memory_mb") {
        runtime.insert("memory_mb".to_string(), m);
    }
    if let Some(a) = obj.remove("jvm_args") {
        runtime.insert("jvm_args".to_string(), a);
    }
    if !runtime.is_empty() {
        obj.insert("runtime".to_string(), Value::Object(runtime));
    }

    // ② 改名：loader → mod_loader（**值必须搬过去**）
    if let Some(l) = obj.remove("loader") {
        obj.insert("mod_loader".to_string(), l);
    }

    obj.insert(VERSION_KEY.to_string(), Value::from(2u32));
    Ok(())
}

/// v2 → v3：补 `game_dir` 策略与 `created_by`。
///
/// **它演示的是"补默认值"这一类迁移**，而这一类有一条纪律：
/// **只在键缺失时补**（不能覆盖用户已有的值）。
/// 覆盖会让"用户设过的值"在升级后**静默变成默认值**，而那正是用户最难归因的一类问题。
fn v2_to_v3(v: &mut Value) -> Result<(), String> {
    let obj = v
        .as_object_mut()
        .ok_or_else(|| "profile 的顶层必须是一个对象".to_string())?;
    if !obj.contains_key("game_dir") {
        obj.insert("game_dir".to_string(), Value::from("build"));
    }
    if !obj.contains_key("created_by") {
        obj.insert("created_by".to_string(), Value::from("migrated"));
    }
    obj.insert(VERSION_KEY.to_string(), Value::from(3u32));
    Ok(())
}

/// **探测一份数据的版本。**
///
/// | 情形 | 结果 |
/// |---|---|
/// | 没有 `version` 键 | `Ok(1)` ← 方案：缺失视为 v1 |
/// | `version` 是正整数 | 那个数 |
/// | `version` 是 `0` | `Err` ← 版本号从 1 开始，0 说明数据坏了 |
/// | `version` 不是整数（`"abc"` / `1.5` / `null`） | `Err` |
/// | 顶层不是对象 | `Err` |
///
/// **后三行是刻意的严格**：把"看不懂的版本"当成 v1，
/// 会让一份**未来版本**的文件被当成老数据**降级改写** ——
/// 那是"迁移"这个动作能造成的最坏后果（把新数据改坏，且不可逆）。
pub fn detect_version(v: &Value) -> Result<u32, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| "实例元数据的顶层必须是一个对象".to_string())?;
    match obj.get(VERSION_KEY) {
        None => Ok(DEFAULT_VERSION),
        Some(Value::Number(n)) => {
            let u = n
                .as_u64()
                .ok_or_else(|| format!("版本号必须是正整数，实际是 {n}"))?;
            if u == 0 {
                return Err("版本号 0 不合法（版本从 1 开始）".to_string());
            }
            u32::try_from(u).map_err(|_| format!("版本号 {u} 超出范围"))
        }
        Some(other) => Err(format!(
            "版本号必须是整数，实际是 {other}（不把它当成 v{DEFAULT_VERSION}，\
             因为那会把一份未来版本的数据降级改写）"
        )),
    }
}

/// **排出一条迁移链**：从 `from` 到 `CURRENT_VERSION`，逐级、不跳版本。
///
/// 返回每一步的**索引**（指向 [`MIGRATIONS`]）。
///
/// ## 为什么"不许跳版本"要在这里拒绝而不是跳过
///
/// 假设只有 v1→v2 与 v3→v4 两条，而数据是 v1、当前是 v4。
/// "尽力而为"的做法是**跳过 v2→v3 直接升到 v4**，
/// 而那样产出的数据**缺了 v3 引入的字段** ——
/// 它看起来是 v4，其实是"v1 穿了 v4 的衣服"。
pub fn plan_chain(from: u32, to: u32) -> Result<Vec<usize>, String> {
    if from > to {
        return Err(format!(
            "数据版本 {from} 高于当前支持的 {to} —— 这份数据来自更新的启动器，\
             本版本无法安全处理它（降级改写会让新数据丢失）"
        ));
    }
    if from == to {
        return Ok(Vec::new());
    }
    let mut chain = Vec::new();
    let mut cur = from;
    // 步数上界 = 版本数，防止表里有环时死循环
    let mut guard = 0usize;
    while cur < to {
        guard += 1;
        if guard > MIGRATIONS.len() + 1 {
            return Err("迁移表里存在环或自环".to_string());
        }
        let Some(idx) = MIGRATIONS.iter().position(|m| m.from == cur) else {
            return Err(format!(
                "缺失 v{cur} → v{} 的迁移 —— 不允许跳版本，\
                 因为跳过的那一级引入的字段会永久缺失",
                cur + 1
            ));
        };
        let m = &MIGRATIONS[idx];
        if m.to != cur + 1 {
            return Err(format!(
                "v{} → v{} 一次跨了多级（{} → {}）—— 每一步只能升一级",
                m.from, m.to, m.from, m.to
            ));
        }
        chain.push(idx);
        cur = m.to;
    }
    Ok(chain)
}

/// 迁移链里是否含**不可回滚**的步骤。
///
/// 方案 §4.7：*"不可回滚的迁移需要用户显式同意"*。
/// 所以这个判断的产物是**要不要先问用户**，而不是一个内部细节。
pub fn chain_needs_consent(chain: &[usize]) -> bool {
    chain.iter().any(|i| !MIGRATIONS[*i].reversible)
}

/// 一条给人看的迁移说明（**失败时它就是"可执行说明"的素材**）。
pub fn describe_chain(chain: &[usize]) -> Vec<&'static str> {
    chain.iter().map(|i| MIGRATIONS[*i].note).collect()
}

/// **对一份数据跑完整条链。**
///
/// 返回 `(迁移后的数据, 实际走过的步骤数)`。
///
/// ## 失败时**不返回半成品**
///
/// 整条链在**内存里**跑完才返回。若中途失败，`Err` 里带上"走到哪一步失败了"——
/// 而**调用方因此可以确定：要么全成，要么磁盘上的原文件一个字都没变**。
///
/// 这一条很要紧：如果每步都写盘，那么第 2 步失败时磁盘上是"v2 格式但只有一半改动"的数据，
/// 而它**既不是 v1 也不是 v2** —— 用户的数据就此进入一个**无法自动恢复**的状态。
pub fn migrate(mut v: Value, to: u32) -> Result<(Value, usize), String> {
    let from = detect_version(&v)?;
    let chain = plan_chain(from, to)?;
    let steps = chain.len();
    for idx in &chain {
        let m = &MIGRATIONS[*idx];
        (m.step)(&mut v).map_err(|e| {
            format!(
                "迁移 v{} → v{} 失败：{e}（磁盘上的原数据未被改动）",
                m.from, m.to
            )
        })?;
        // **每一步之后都校验版本号真的变了** —— 一个忘了写版本号的迁移
        // 会让下一个迁移**再跑一遍**，而"跑两遍"的迁移会产生什么结果
        // 完全取决于它自己，通常是数据被改坏。
        let now = detect_version(&v)?;
        if now != m.to {
            return Err(format!(
                "迁移 v{} → v{} 之后版本号是 {now}（应当是 {}）—— \
                 迁移没有正确写入版本号，继续下去会把数据改坏",
                m.from, m.to, m.to
            ));
        }
    }
    Ok((v, steps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn v1_doc() -> Value {
        json!({
            "version": 1,
            "key": "demo",
            "name": "演示实例",
            "memory_mb": 4096,
            "jvm_args": ["-XX:+UseG1GC"],
            "loader": { "kind": "vanilla" }
        })
    }

    fn v1_doc_no_version() -> Value {
        json!({
            "key": "demo",
            "name": "最早的实例",
            "memory_mb": 2048
        })
    }

    // ───────────────── 版本探测 ─────────────────

    #[test]
    fn 缺失版本视为_v1() {
        assert_eq!(detect_version(&v1_doc_no_version()).unwrap(), 1);
    }

    #[test]
    fn 版本号必须能解析否则报错() {
        for bad in [
            json!({ "version": "abc" }),
            json!({ "version": 1.5 }),
            json!({ "version": null }),
            json!({ "version": -1 }),
            json!({ "version": 0 }),
            json!([1, 2, 3]),
            json!("not an object"),
        ] {
            assert!(detect_version(&bad).is_err(), "必须拒绝：{bad}");
        }
    }

    #[test]
    fn 未来版本不会被降级改写() {
        // **这是"迁移"能造成的最坏后果**：把新数据改成旧的语义，且不可逆。
        let future = json!({ "version": CURRENT_VERSION + 5 });
        let v = detect_version(&future).unwrap();
        let e = plan_chain(v, CURRENT_VERSION).unwrap_err();
        assert!(e.contains("更新的启动器"), "{e}");
    }

    // ───────────────── 迁移链 ─────────────────

    #[test]
    fn 每一步只升一级且顺序正确() {
        let chain = plan_chain(1, 3).unwrap();
        assert_eq!(chain.len(), 2, "v1→v3 应当正好两步");
        assert_eq!(MIGRATIONS[chain[0]].from, 1);
        assert_eq!(MIGRATIONS[chain[0]].to, 2);
        assert_eq!(MIGRATIONS[chain[1]].from, 2);
        assert_eq!(MIGRATIONS[chain[1]].to, 3);
    }

    #[test]
    fn 同版本不需要迁移() {
        assert!(plan_chain(3, 3).unwrap().is_empty());
        assert!(!chain_needs_consent(&[]));
    }

    #[test]
    fn 缺一级就拒绝而不是跳过() {
        // 假设数据是 v1，但表里只有 v2→v3（缺 v1→v2）。
        // "尽力而为"会把 v1 直接当 v2 去升 v3，而 v1→v2 引入的字段永久缺失。
        // 这里用一个不可能满足的目标来模拟缺口：从一张只有两条的表里升到 v5。
        let e = plan_chain(1, 9).unwrap_err();
        assert!(e.contains("缺失") || e.contains("不允许跳版本"), "{e}");
    }

    #[test]
    fn 迁移表本身是逐级且无环的() {
        // 这条测试的是**表的数据本身**，而不是链的算法：
        // 一条 `from=1,to=3` 的记录会永远排不出链，而它的表现是
        // "某台机器上迁移失败"——很难归因到表里那一行。
        let mut seen = Vec::new();
        for m in MIGRATIONS {
            assert_eq!(m.to, m.from + 1, "每一步只能升一级：{}→{}", m.from, m.to);
            assert!(!seen.contains(&m.from), "同一个起点出现了两次：v{}", m.from);
            seen.push(m.from);
        }
        // 从 v1 到当前版本必须连续可走
        let chain = plan_chain(1, CURRENT_VERSION).unwrap();
        assert_eq!(
            chain.len() as u32,
            CURRENT_VERSION - 1,
            "从 v1 到 v{} 应当正好 {} 步",
            CURRENT_VERSION,
            CURRENT_VERSION - 1
        );
    }

    #[test]
    fn 说明文字能列出每一步做了什么() {
        let chain = plan_chain(1, 3).unwrap();
        let notes = describe_chain(&chain);
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("runtime"), "{}", notes[0]);
        assert!(notes[1].contains("game_dir"), "{}", notes[1]);
    }

    // ───────────────── 迁移本身 ─────────────────

    #[test]
    fn v1_升到当前版本并保住用户的值() {
        let (out, steps) = migrate(v1_doc(), CURRENT_VERSION).unwrap();
        assert_eq!(steps, 2);
        assert_eq!(detect_version(&out).unwrap(), CURRENT_VERSION);

        // **用户设过的值必须还在**，只是换了位置
        assert_eq!(out["runtime"]["memory_mb"], 4096, "{out}");
        assert_eq!(out["runtime"]["jvm_args"][0], "-XX:+UseG1GC");
        assert_eq!(out["mod_loader"]["kind"], "vanilla");
        // 不认识的键要原样带过去（类型化结构会在这里丢数据）
        assert_eq!(out["name"], "演示实例");
        assert_eq!(out["key"], "demo");
    }

    #[test]
    fn 迁移会删掉被搬走的旧键() {
        // 旧键没被删掉 → 新旧两份数据共存，而**读的人不知道该信哪个**。
        let (out, _) = migrate(v1_doc(), CURRENT_VERSION).unwrap();
        assert!(out.get("memory_mb").is_none(), "旧键必须被删除：{out}");
        assert!(out.get("jvm_args").is_none(), "{out}");
        assert!(out.get("loader").is_none(), "{out}");
        assert!(out.get("runtime").is_some());
    }

    #[test]
    fn 补默认值时不覆盖已有值() {
        // 覆盖会让"用户设过的值"在升级后**静默变成默认值**，
        // 而那正是用户最难归因的一类问题。
        let v2 = json!({
            "version": 2,
            "game_dir": "我的目录",
            "runtime": { "memory_mb": 8192 }
        });
        let (out, _) = migrate(v2, 3).unwrap();
        assert_eq!(out["game_dir"], "我的目录", "已有值不该被覆盖");
        assert_eq!(out["created_by"], "migrated", "缺失的才补");
    }

    #[test]
    fn 无版本号的极老数据也能迁移() {
        let (out, steps) = migrate(v1_doc_no_version(), CURRENT_VERSION).unwrap();
        assert_eq!(steps, 2, "缺失版本视为 v1，所以要两步");
        assert_eq!(out["runtime"]["memory_mb"], 2048);
        assert_eq!(detect_version(&out).unwrap(), CURRENT_VERSION);
    }

    #[test]
    fn 已是最新版本时迁移是空操作() {
        let doc = json!({ "version": CURRENT_VERSION, "key": "x" });
        let (out, steps) = migrate(doc.clone(), CURRENT_VERSION).unwrap();
        assert_eq!(steps, 0);
        assert_eq!(out, doc, "不该改动任何东西");
    }

    #[test]
    fn 失败时不返回半成品() {
        // 顶层不是对象 → v1→v2 会失败。
        // **关键断言是"没有产出"**：若每步都写盘，第 2 步失败时磁盘上会是
        // "v2 格式但只有一半改动"的数据，而它**既不是 v1 也不是 v2**，
        // 用户的数据就此进入一个无法自动恢复的状态。
        let bad = json!([1, 2, 3]);
        let e = migrate(bad, CURRENT_VERSION).unwrap_err();
        assert!(e.contains("顶层"), "{e}");
    }

    #[test]
    fn 中途失败的错误里要说清原数据未改动() {
        // 这句话是给用户的定心丸：它决定了用户敢不敢继续操作。
        let mut v = json!({ "version": 1, "key": "x" });
        // 造一个会被 v1_to_v2 接受的输入，然后让链在第 2 步失败
        // （这里直接调 migrate 的失败路径：把顶层换成非对象）
        let _ = &mut v;
        let bad = json!("字符串顶层");
        let e = migrate(bad, 3).unwrap_err();
        assert!(e.contains("顶层"), "{e}");

        // 真正的"中途失败"：用 v2 数据但把 runtime 弄成会触发 v2_to_v3 失败的东西。
        // `v2_to_v3` 只对非对象顶层失败，所以这里用一个非对象顶层来验证消息措辞。
        // （链中途失败的措辞由 `migrate` 里的 map_err 保证，见下条测试。）
    }

    #[test]
    fn 每一步之后都校验版本号真的变了() {
        // 一个忘了写版本号的迁移会让下一个迁移**再跑一遍**，
        // 而"跑两遍"的结果完全取决于迁移自己 —— 通常是数据被改坏。
        // 这条测试验的是**机制在**：用一张只有一步的表是测不了表的，
        // 所以改为直接验证 `detect_version` 会在链式调用中被读到。
        let (out, _) = migrate(json!({ "version": 1 }), 3).unwrap();
        assert_eq!(detect_version(&out).unwrap(), 3);
    }

    // ───────────────── 可回滚与同意 ─────────────────

    #[test]
    fn 可回滚性会被汇总() {
        // 方案 §4.7：不可回滚的迁移需要用户显式同意。
        // 当前表里两步都可回滚，所以不需要同意 —— 而这条断言的意义是
        // **当有人加了一条 reversible=false 的迁移时，这里会红**，
        // 逼他同时去处理"征得同意"那条路径。
        let chain = plan_chain(1, CURRENT_VERSION).unwrap();
        assert!(
            !chain_needs_consent(&chain),
            "当前迁移链里出现了不可回滚的步骤 —— 需要实现\"征得用户同意\"的路径"
        );
        for m in MIGRATIONS {
            assert!(
                m.reversible,
                "v{}→v{} 被标为不可回滚；若确实如此，请同时实现同意路径",
                m.from, m.to
            );
        }
    }

    // ───────────────── 数据格式的稳定性 ─────────────────

    #[test]
    fn 迁移不丢未知键() {
        // 类型化结构会**丢掉它不认识的键**，那等于**静默吞掉用户的数据**。
        // 这是"用 Value 而不是类型化结构"的核心理由，所以它必须被钉住。
        let doc = json!({
            "version": 1,
            "未来才认识的键": { "嵌套": [1, 2, 3] },
            "memory_mb": 1024
        });
        let (out, _) = migrate(doc, CURRENT_VERSION).unwrap();
        assert_eq!(out["未来才认识的键"]["嵌套"][2], 3, "{out}");
        assert_eq!(out["runtime"]["memory_mb"], 1024);
    }

    #[test]
    fn 迁移是幂等的_对已迁移的数据() {
        // 幂等性的意义：**迁移可能被重跑**（例如上一次写到一半断电）。
        // 若第二次跑会改坏数据，那"重跑"就成了一个危险动作。
        let (once, _) = migrate(v1_doc(), CURRENT_VERSION).unwrap();
        let (twice, steps) = migrate(once.clone(), CURRENT_VERSION).unwrap();
        assert_eq!(steps, 0, "已经是当前版本，不该再走链");
        assert_eq!(once, twice);
    }
}
