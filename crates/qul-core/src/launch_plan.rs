//! # 从版本详情**组装启动参数**（M3 · **纯规则，零 IO**）
//!
//! ## 它补的是哪一环
//!
//! | 层 | 知道什么 |
//! |---|---|
//! | [`crate::descriptor`] | **格式**：这个版本用哪套参数形态、哪些库要装 |
//! | [`crate::plan`] | **骨架与占位符**：`{NAME}` 待填 |
//! | **本模块** | **把前者变成后者** —— 也就是"这个版本要跑起来，命令行长什么样" |
//!
//! ## 🔴 两套形态，而它们**在同一个函数里被抹平**
//!
//! 实测的那两个断代点（`spikes/m2-metadata-shapes/结论.md`）在这里汇合：
//!
//! | 形态 | 判据 | 怎么变成参数 |
//! |---|---|---|
//! | **旧** | 有 `minecraftArguments` | **按空白切分**成一个列表 |
//! | **新** | 有 `arguments.jvm` / `arguments.game` | 数组元素：字符串直接用，**条件对象先求 `rules`** |
//!
//! ## ⚠️ 而"条件对象"这一支只在**新**形态里有
//!
//! 实测：旧形态的字符串里**没有**条件参数 —— 一台机器上能跑的参数是固定的。
//! 所以旧形态不需要任何求值，而新形态必须先过 `rules`。
//!
//! ## 占位符：**两个语法，刻意不统一**
//!
//! | 语法 | 谁写 | 含义 |
//! |---|---|---|
//! | `${NAME}` | Mojang 的版本 JSON | **产品的模板变量**（官方启动器填） |
//! | `{NAME}` | 我们的 [`crate::plan`] | **我们骨架的占位符**（调用方填） |
//!
//! 本模块把 `${...}` **原样保留** —— 因为它属于格式。
//!
//! ### 🔴 而这里有一个必须说清的后果
//!
//! [`crate::plan::LaunchPlan::resolve`] **只替换它自己那份占位符表里的 `{NAME}`**，
//! 而 `${NAME}` **不匹配那个语法**（它是 `{` 后面跟 `$`），所以**会原样留下**。
//!
//! 那是**刻意的、也是正确的**：`${...}` 的值（`natives_directory`、`classpath`、
//! `auth_uuid`…）**必须在** [`resolve`][crate::plan::LaunchPlan::resolve]
//! **之前**由本模块的调用方填进 `placeholders`。
//!
//! **反过来说：一份 `resolve()` 成功但里面还有 `${...}` 的命令，是一个 bug。**
//! 所以本模块提供一个 [`launch_plan::leftover_template_vars`] 让那件事**能被检查** ——
//! 见下面那条测试 `解析后不该留下官方模板变量`。

use crate::descriptor::{ArgItem, ArgumentForm, Descriptor, Env, LibraryPlan};
use std::collections::BTreeMap;

/// 官方模板变量的前缀与括号（**格式的一部分，不是我们的选择**）。
pub const TEMPLATE_OPEN: &str = "${";
pub const TEMPLATE_CLOSE: char = '}';

/// **官方模板变量**这个名字本身是 `template`，而不是"占位符" ——
/// 因为"占位符"在本项目里已经指 [`crate::plan`] 的 `{NAME}`，
/// 而两者**语法不同、归属不同**，共用一个名词会让讨论变得含混。
///
/// 扫出一个字符串里出现的全部 `${NAME}`。
///
/// 嵌套与不闭合的花括号**不报错**，只是扫不到 —— 因为：
/// ① 官方模板里没有嵌套；② 一个不闭合的 `${` 是**格式有了变化**，
/// 而那应当由"扫出来的数量与预期不符"暴露，而不是由这里猜。
pub fn template_vars(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let open = TEMPLATE_OPEN.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        // ⚠️ **用字节切片比较，不要 `&s[i..i+2]`。**
        //
        // 第一版写的是字符串切片，而 `i += 1` 会让它落在多字节字符**中间** ——
        // 于是 `&s[i..i+2]` 在字符边界上 panic。
        // 测试 `扫描处理多字节字符不切坏` 抓到了它，而那个场景是真实的：
        // **用户的目录名可能带中文**。
        //
        // 用字节比较是安全的：`$` 与 `{` 都是 ASCII，而 UTF-8 的续字节
        // 高两位固定是 `10`，永远不会等于任何一个 ASCII 字节 ——
        // 所以 ASCII 序列只可能出现在字符边界上。
        if bytes.len() - i >= open.len() && bytes[i..i + open.len()] == *open {
            let start = i + open.len();
            let mut j = start;
            while j < bytes.len() && bytes[j] != TEMPLATE_CLOSE as u8 {
                // 遇到另一个 `{` 说明这不是一个规整的变量 —— 放弃这一处
                if bytes[j] == b'{' {
                    break;
                }
                j += 1;
            }
            if j < bytes.len() && bytes[j] == TEMPLATE_CLOSE as u8 && j > start {
                // 名字本身也必须是合法 UTF-8 才能成为一个 `String`。
                // 不合法就跳过（而不是 panic）—— 那意味着格式里出现了别的编码。
                if let Ok(name) = std::str::from_utf8(&bytes[start..j]) {
                    out.push(name.to_string());
                    i = j + 1;
                    continue;
                }
            }
        }
        // 按**字符**推进，而不是按字节
        let ch = s[i..].chars().next().expect("i 一定在字符边界上");
        i += ch.len_utf8();
    }
    out
}

/// 一份字符串里**还剩下**的官方模板变量（用于"解析后不该有"的检查）。
pub fn leftover_template_vars(args: &[String]) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (i, a) in args.iter().enumerate() {
        for v in template_vars(a) {
            out.push((i, v, a.clone()));
        }
    }
    out
}

/// 组装的输入：**除参数之外的、组装时需要知道的那些值**。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssembleInput {
    /// 可执行文件（**可以含我们自己的 `{NAME}` 占位符**）
    pub program: String,
    /// 主类（从详情读，**不硬编码**）
    pub main_class: String,
    /// 把官方模板变量映射成我们的占位符名。
    ///
    /// ## 为什么是一张表而不是一堆字段
    ///
    /// 因为那张表**就是"官方模板词汇表"与"我们的事实键"之间的对照**，
    /// 而把它写成一堆字段会让"官方将来加一个变量"需要改结构体。
    ///
    /// **键是官方名**（`natives_directory`），**值是我们的占位符名**（`NATIVES_DIR`）。
    /// 不在表里的官方变量**原样保留** —— 于是"我们还没支持某个变量"
    /// 会表现为一条 `leftover_template_vars`，而不是一个静默的空串。
    pub template_map: BTreeMap<String, String>,
}

/// 组装结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    /// 骨架：JVM 参数 + 主类 + 游戏参数
    pub args: Vec<String>,
    /// 这次组装**用到**的官方模板变量（去重、有序）
    pub templates_used: Vec<String>,
    /// **用到了但我们没有映射**的官方变量 —— 调用方必须处理它
    pub templates_unmapped: Vec<String>,
    /// 这个版本用的是哪套形态（`legacy` / `modern`）—— 它要进日志
    pub form: &'static str,
    /// 对当前环境**有效**的库（已过滤、已归一出 natives）
    pub libraries: Vec<LibraryPlan>,
    /// 要解压出 natives 的那些包
    pub natives: Vec<LibraryPlan>,
}

/// 把官方模板变量按表换成我们的占位符。
fn rewrite_templates(s: &str, map: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if i + TEMPLATE_OPEN.len() <= bytes.len() && &s[i..i + TEMPLATE_OPEN.len()] == TEMPLATE_OPEN
        {
            let start = i + TEMPLATE_OPEN.len();
            let mut j = start;
            while j < bytes.len() && bytes[j] != TEMPLATE_CLOSE as u8 && bytes[j] != b'{' {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == TEMPLATE_CLOSE as u8 && j > start {
                let name = &s[start..j];
                match map.get(name) {
                    Some(ours) => {
                        out.push('{');
                        out.push_str(ours);
                        out.push('}');
                    }
                    // **没有映射就原样保留** —— 见 `AssembleInput::template_map`。
                    None => out.push_str(&s[i..j + 1]),
                }
                i = j + 1;
                continue;
            }
        }
        // 按字符推进入，避免在多字节 UTF-8 中间切断
        let ch = s[i..].chars().next().expect("i 在边界上");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// 把一段 `argument`（字符串或值的数组）变成参数列表。
fn value_to_args(v: &crate::descriptor::ArgValue) -> Vec<String> {
    v.as_slice().into_iter().map(|s| s.to_string()).collect()
}

/// **组装启动参数。**
///
/// 顺序（与官方一致，而它有意义）：**JVM 参数 → `-cp` … → 主类 → 游戏参数**。
///
/// ⚠️ **`-cp` 与 classpath 的内容由调用方给**（`classpath_arg`）——
/// 因为"库放在哪、客户端 jar 在哪"是**布局决定**，不是格式决定。
/// 本模块只负责**位置**（在 JVM 参数之后、主类之前）。
pub fn assemble(
    d: &Descriptor,
    env: &Env,
    input: &AssembleInput,
    classpath: Option<&str>,
) -> Result<Assembled, AssembleError> {
    let main_class = d.main_class()?;
    let form = d.argument_form()?;

    let mut args: Vec<String> = Vec::new();
    let mut templates_used: Vec<String> = Vec::new();

    match &form {
        ArgumentForm::Modern { jvm, game } => {
            // ① JVM 参数（**过 rules**）
            for item in jvm {
                for a in expand_item(item, env) {
                    templates_used.extend(template_vars(&a));
                    args.push(a);
                }
            }
            // ② classpath：**位置在这儿**，内容由调用方给
            if let Some(cp) = classpath {
                args.push("-cp".to_string());
                args.push(cp.to_string());
            }
            // ③ 主类
            if !main_class.is_empty() {
                args.push(main_class.to_string());
            }
            // ④ 游戏参数（**同样过 rules**）
            for item in game {
                for a in expand_item(item, env) {
                    templates_used.extend(template_vars(&a));
                    args.push(a);
                }
            }
        }
        ArgumentForm::Legacy { legacy_arguments } => {
            // 旧形态：一个字符串，按空白切分。**没有条件参数** —— 见模块文档。
            if let Some(cp) = classpath {
                args.push("-cp".to_string());
                args.push(cp.to_string());
            }
            if !main_class.is_empty() {
                args.push(main_class.to_string());
            }
            for a in crate::descriptor::split_legacy_arguments(legacy_arguments) {
                templates_used.extend(template_vars(&a));
                args.push(a);
            }
        }
    }

    // 用我们的占位符名替换官方模板变量
    let args: Vec<String> = args
        .iter()
        .map(|a| rewrite_templates(a, &input.template_map))
        .collect();

    templates_used.sort();
    templates_used.dedup();

    // **用到了但没映射的** —— 调用方必须知道，否则它会在启动时才炸。
    let templates_unmapped: Vec<String> = templates_used
        .iter()
        .filter(|n| !input.template_map.contains_key(*n))
        .cloned()
        .collect();

    // 库：按环境过滤，并分出 natives
    let libraries = d.library_plans(env);
    let natives = libraries
        .iter()
        .filter(|p| p.natives.is_some())
        .cloned()
        .collect();

    Ok(Assembled {
        args,
        templates_used,
        templates_unmapped,
        form: form.which(),
        libraries,
        natives,
    })
}

/// 展开一个参数条目（**过 `rules`**）。
fn expand_item(item: &ArgItem, env: &Env) -> Vec<String> {
    match item {
        ArgItem::Plain(s) => vec![s.clone()],
        ArgItem::Conditional { rules, value } => {
            if crate::descriptor::rules_allow(rules, env) {
                value_to_args(value)
            } else {
                Vec::new()
            }
        }
    }
}

/// 组装失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssembleError {
    /// 详情里没有 `mainClass`
    NoMainClass,
    /// 详情里既没有 `arguments` 也没有旧形态的参数字符串
    NoArguments,
}

impl std::fmt::Display for AssembleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssembleError::NoMainClass => write!(
                f,
                "版本详情里没有 mainClass —— 它变过三次（见 descriptor 模块），必须从 JSON 读"
            ),
            AssembleError::NoArguments => write!(f, "版本详情里两套参数形态都没有，无法组装参数"),
        }
    }
}

impl std::error::Error for AssembleError {}

impl From<crate::descriptor::MetaError> for AssembleError {
    fn from(e: crate::descriptor::MetaError) -> Self {
        match e {
            crate::descriptor::MetaError::NoMainClass => AssembleError::NoMainClass,
            crate::descriptor::MetaError::NoArguments => AssembleError::NoArguments,
            // 其余错误（坏 JSON / 缺 id）在更早的阶段就该被拦住。
            _ => AssembleError::NoArguments,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{Descriptor, Env, PlatformTarget};

    fn win() -> Env {
        Env::new(PlatformTarget::windows("10.0.26200", "x86_64"))
    }

    fn input() -> AssembleInput {
        let mut template_map = BTreeMap::new();
        template_map.insert("natives_directory".to_string(), "NATIVES_DIR".to_string());
        template_map.insert("classpath".to_string(), "CLASSPATH".to_string());
        template_map.insert("auth_player_name".to_string(), "PLAYER".to_string());
        template_map.insert("version_name".to_string(), "VERSION".to_string());
        template_map.insert("launcher_name".to_string(), "LAUNCHER_NAME".to_string());
        template_map.insert(
            "launcher_version".to_string(),
            "LAUNCHER_VERSION".to_string(),
        );
        AssembleInput {
            program: "{JAVA}".into(),
            main_class: String::new(),
            template_map,
        }
    }

    // ───────────────── 模板变量扫描 ─────────────────

    #[test]
    fn 能扫出模板变量() {
        assert_eq!(template_vars("${A}"), vec!["A"]);
        assert_eq!(template_vars("-Dx=${A}/y -cp ${B}"), vec!["A", "B"]);
        assert_eq!(template_vars("没有变量"), Vec::<String>::new());
        assert_eq!(template_vars("${}"), Vec::<String>::new(), "空名不算");
        assert_eq!(template_vars("${A"), Vec::<String>::new(), "不闭合不算");
    }

    #[test]
    fn 扫描不会把我们的占位符当成模板变量() {
        // ⚠️ **两个语法必须能区分。** `{NAME}` 是我们的，`${NAME}` 是格式的。
        assert_eq!(template_vars("{OURS}"), Vec::<String>::new());
        assert_eq!(template_vars("${THEIRS}"), vec!["THEIRS"]);
        // 混在一行里也要对
        assert_eq!(template_vars("{OURS}/${THEIRS}"), vec!["THEIRS"]);
    }

    #[test]
    fn 扫描处理多字节字符不切坏() {
        // 中文路径是真实场景（用户的目录名可能带中文）
        let s = "${A}/中文/${B}目录";
        assert_eq!(template_vars(s), vec!["A", "B"]);
    }

    // ───────────────── 模板改写 ─────────────────

    #[test]
    fn 模板按表改写() {
        let m: BTreeMap<String, String> = [("A".to_string(), "OURS_A".to_string())]
            .into_iter()
            .collect();
        assert_eq!(rewrite_templates("${A}", &m), "{OURS_A}");
        assert_eq!(rewrite_templates("-x=${A}/y", &m), "-x={OURS_A}/y");
    }

    #[test]
    fn 没有映射的模板原样保留_而不是变成空串() {
        // ⚠️ **这是安全的失败方向。**
        // 一个"没映射就当空串"的实现会把 `-Djava.library.path=${natives_directory}`
        // 变成 `-Djava.library.path=` —— 那是一个**看起来正常、但会让游戏起不来**
        // 的参数。而原样保留会让它出现在 `templates_unmapped` 里，被调用方看见。
        let m: BTreeMap<String, String> = BTreeMap::new();
        assert_eq!(rewrite_templates("${UNKNOWN}", &m), "${UNKNOWN}");
    }

    #[test]
    fn 改写不动我们的占位符() {
        // `{OURS}` 不含 `$`，所以它必须原样通过 —— 那是 plan 层的语法。
        let m: BTreeMap<String, String> = BTreeMap::new();
        assert_eq!(rewrite_templates("{OURS}", &m), "{OURS}");
        assert_eq!(
            rewrite_templates("{OURS}/${THEIRS}", &m),
            "{OURS}/${THEIRS}"
        );
    }

    // ───────────────── 旧形态组装 ─────────────────

    #[test]
    fn 旧形态按空白切分并接在_cp_之后() {
        let json = r#"{
            "id":"old","mainClass":"com.example.Main",
            "minecraftArguments":"${auth_player_name} --gameDir ${game_directory}"
        }"#;
        let d = Descriptor::parse(json).unwrap();
        let a = assemble(&d, &win(), &input(), Some("cp.jar")).unwrap();
        assert_eq!(a.form, "legacy");
        assert_eq!(
            a.args,
            vec![
                "-cp",
                "cp.jar",
                "com.example.Main",
                "{PLAYER}",
                "--gameDir",
                "${game_directory}"
            ]
        );
        // `game_directory` 没在表里 → 原样保留且被列出来
        assert_eq!(a.templates_used, vec!["auth_player_name", "game_directory"]);
        assert_eq!(a.templates_unmapped, vec!["game_directory"]);
    }

    #[test]
    fn 旧形态没有条件参数_所以规则不影响它() {
        // 实测：旧形态的字符串里没有条件参数。这条测试把这个事实写下来 ——
        // 若哪天有人给旧形态加 rules 求值，它会发现这里没有 rules 可求。
        let json = r#"{"id":"old","mainClass":"M","minecraftArguments":"--x 1"}"#;
        let d = Descriptor::parse(json).unwrap();
        let osx = Env::new(PlatformTarget {
            name: "osx".into(),
            version: "14".into(),
            arch: "arm64".into(),
        });
        let a = assemble(&d, &osx, &input(), None).unwrap();
        let b = assemble(&d, &win(), &input(), None).unwrap();
        assert_eq!(a.args, b.args, "旧形态在两个平台上产出同一组参数");
    }

    // ───────────────── 新形态组装 ─────────────────

    fn modern_json() -> String {
        r#"{
            "id":"new",
            "mainClass":"com.example.Main",
            "arguments": {
                "jvm": [
                    "-Xss1M",
                    {"rules":[{"action":"allow","os":{"name":"windows"}}], "value":"-Dwin=1"},
                    {"rules":[{"action":"allow","os":{"name":"osx"}}], "value":"-XstartOnFirstThread"},
                    {"rules":[{"action":"allow","os":{"name":"osx"}}], "value":["-Da","-Db"]},
                    "-Djava.library.path=${natives_directory}"
                ],
                "game": [
                    "--username", "${auth_player_name}",
                    "--version", "${version_name}"
                ]
            }
        }"#.to_string()
    }

    #[test]
    fn 新形态的_jvm_参数在_cp_之前_主类之前_游戏参数之后() {
        let d = Descriptor::parse(&modern_json()).unwrap();
        let a = assemble(&d, &win(), &input(), Some("cp")).unwrap();
        assert_eq!(a.form, "modern");
        // 顺序：JVM → -cp cp → 主类 → 游戏参数
        let cp_at = a.args.iter().position(|x| x == "-cp").unwrap();
        let mc_at = a.args.iter().position(|x| x == "com.example.Main").unwrap();
        let user_at = a.args.iter().position(|x| x == "--username").unwrap();
        assert!(cp_at < mc_at, "-cp 必须在主类之前");
        assert!(mc_at < user_at, "主类必须在游戏参数之前");
        // 而第一条 JVM 参数在最前
        assert_eq!(a.args[0], "-Xss1M");
    }

    #[test]
    fn 新形态的条件参数按平台过滤() {
        let d = Descriptor::parse(&modern_json()).unwrap();
        let a = assemble(&d, &win(), &input(), None).unwrap();
        assert!(
            a.args.iter().any(|x| x == "-Dwin=1"),
            "windows 专属参数该在"
        );
        assert!(
            !a.args.iter().any(|x| x == "-XstartOnFirstThread"),
            "osx 专属参数不该在"
        );
        assert!(!a.args.iter().any(|x| x == "-Da"));

        let osx = Env::new(PlatformTarget {
            name: "osx".into(),
            version: "14".into(),
            arch: "arm64".into(),
        });
        let b = assemble(&d, &osx, &input(), None).unwrap();
        assert!(!b.args.iter().any(|x| x == "-Dwin=1"));
        assert!(b.args.iter().any(|x| x == "-XstartOnFirstThread"));
        // 而**数组形态的 value 要展开成两个参数**
        assert!(b.args.iter().any(|x| x == "-Da"));
        assert!(b.args.iter().any(|x| x == "-Db"));
    }

    #[test]
    fn 新形态的模板变量被换成了我们的占位符() {
        let d = Descriptor::parse(&modern_json()).unwrap();
        let a = assemble(&d, &win(), &input(), None).unwrap();
        assert!(
            a.args
                .iter()
                .any(|x| x == "-Djava.library.path={NATIVES_DIR}"),
            "该换成我们的占位符：{:?}",
            a.args
        );
        assert!(a.args.iter().any(|x| x == "{PLAYER}"));
        assert!(a.args.iter().any(|x| x == "{VERSION}"));
        // 这几个都在表里 → 没有未映射的
        assert!(
            a.templates_unmapped.is_empty(),
            "{:?}",
            a.templates_unmapped
        );
    }

    // ───────────────── 库与 natives ─────────────────

    #[test]
    fn 组装顺带给出有效的库与_natives() {
        let json = r#"{
            "id":"x","mainClass":"M",
            "arguments":{"jvm":[],"game":[]},
            "libraries":[
                {"name":"a:b:1","downloads":{"artifact":{"path":"a.jar","sha1":"s","size":1,"url":"u"}}},
                {"name":"osx:only:1","rules":[{"action":"allow","os":{"name":"osx"}}],
                 "downloads":{"artifact":{"path":"o.jar","sha1":"s","size":1,"url":"u"}}},
                {"name":"x:y:1","natives":{"windows":"natives-windows"},
                 "downloads":{"classifiers":{"natives-windows":{"path":"n.jar","sha1":"s","size":1,"url":"u"}}}}
            ]
        }"#;
        let d = Descriptor::parse(json).unwrap();
        let a = assemble(&d, &win(), &input(), None).unwrap();
        // osx-only 被过滤掉
        assert_eq!(a.libraries.len(), 2, "{:?}", a.libraries);
        // 而有一个是 natives
        assert_eq!(a.natives.len(), 1);
        assert_eq!(a.natives[0].natives.as_ref().unwrap().path, "n.jar");
    }

    // ───────────────── 错误路径 ─────────────────

    #[test]
    fn 缺_main_class_时是明确的错误() {
        let json = r#"{"id":"x","minecraftArguments":"a"}"#;
        let d = Descriptor::parse(json).unwrap();
        let e = assemble(&d, &win(), &input(), None).unwrap_err();
        assert_eq!(e, AssembleError::NoMainClass);
        assert!(e.to_string().contains("变过三次"));
    }

    #[test]
    fn 两套形态都没有时是明确的错误() {
        let json = r#"{"id":"x","mainClass":"M"}"#;
        let d = Descriptor::parse(json).unwrap();
        assert_eq!(
            assemble(&d, &win(), &input(), None).unwrap_err(),
            AssembleError::NoArguments
        );
    }

    // ───────────────── 那条最重要的检查 ─────────────────

    #[test]
    fn 解析后不该留下官方模板变量() {
        // ⚠️ **这条测试钉的是模块文档里那句警告。**
        //
        // `plan.resolve()` 只替换 `{NAME}`，而 `${NAME}` **不匹配那个语法**
        // —— 所以一份"`resolve()` 成功但里面还有 `${...}`"的命令是一个 bug，
        // 而它会**静默地**把一个字面的 `${natives_directory}` 传给 Java。
        //
        // 所以组装之后必须检查 `templates_unmapped` 为空。
        let d = Descriptor::parse(&modern_json()).unwrap();
        let a = assemble(&d, &win(), &input(), Some("cp")).unwrap();
        assert!(
            a.templates_unmapped.is_empty(),
            "**还有官方模板变量没被映射：{:?}** —— \
             它们会被原样传给 Java 进程，而游戏会以一个难查的方式失败",
            a.templates_unmapped
        );
        let left = leftover_template_vars(&a.args);
        assert!(left.is_empty(), "解析后不该留下 `${{...}}`：{left:?}");
    }

    #[test]
    fn 未映射的模板变量会被列出来而不是静默通过() {
        // 用一个"表里故意缺一项"的输入，断言它**被看见**。
        let json = r#"{
            "id":"x","mainClass":"M",
            "arguments":{"jvm":["-Dx=${we_never_mapped_this}"],"game":[]}
        }"#;
        let d = Descriptor::parse(json).unwrap();
        let a = assemble(&d, &win(), &input(), None).unwrap();
        assert_eq!(a.templates_unmapped, vec!["we_never_mapped_this"]);
        // 而它**仍然原样在参数里**（不是空串）—— 那是安全的失败方向
        assert!(a.args.iter().any(|x| x == "-Dx=${we_never_mapped_this}"));
    }
}
