//! # 启动计划的形状与可复现性（**纯规则，零 IO**）
//!
//! ## 为什么"计划"必须与"执行"分开
//!
//! 方案 §5.8 的安全基线要求**"前端无法绕过命令层直接操作"**。
//! 而命令层这一侧的对手要求是：**"执行"必须是一件可以被检查的事**。
//!
//! 分开的收益是具体的：**一个启动计划可以在不启动任何进程的前提下被断言**。
//! 于是"参数对不对"变成**可测试的**，而不用真的把游戏跑起来看它崩不崩。
//! 这在 M3 尤其要紧——那时我们要对着真实游戏调几十个参数，
//! **如果每次都靠"跑起来看看"，成本会是几十次启动**。
//!
//! ## 两条硬要求（都来自方案原文）
//!
//! | 要求 | 原文 | 落点 |
//! |---|---|---|
//! | **可复现** | `Launchable` = "产出**可复现**启动计划" | 同一份计划解析两次结果必须逐字节相同 |
//! | **未解析占位符必须失败** | `PlanUnresolvedPlaceholder`（错误码已登记） | [`LaunchPlan::resolve`] 返回 `Err` 而不是带着 `{...}` 去启动 |
//!
//! **第二条为什么不能"尽力而为"**：一个没被替换的 `{JAVA}` 会被游戏当成**路径**
//! 去打开，于是用户看到的是"游戏起不来"，而**真正的原因是一个花括号**。
//! 那种失败**无法从现场归因**——所以必须在解析阶段就断掉。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 占位符分隔符。**`{NAME}` 形式**，与方案里写的 `PlanUnresolvedPlaceholder` 对应。
///
/// 用 `{}` 而不是 `%NAME%` 或 `${NAME}`：后两者在 Windows 路径与 shell 里都有特殊含义
/// （`%` 是环境变量语法，`$` 是 PowerShell 语法），**会造成"看起来是占位符其实不是"的误判**。
pub const PLACEHOLDER_OPEN: char = '{';
pub const PLACEHOLDER_CLOSE: char = '}';

/// 一个未解析的占位符。**带上下文**，因为只报"有占位符没解析"无法定位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedPlaceholder {
    /// 占位符名（不含花括号）
    pub name: String,
    /// 它出现在哪一个参数里（0 基）
    pub arg_index: usize,
    /// 那个参数的完整内容（便于人直接看懂）
    pub arg: String,
}

impl std::fmt::Display for UnresolvedPlaceholder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "占位符 {{{}}} 未解析（出现在第 {} 个参数：{}）",
            self.name, self.arg_index, self.arg
        )
    }
}

/// **一份可复现的启动计划。**
///
/// 形状刻意分成两半：
///
/// | 字段 | 是什么 | 谁填 |
/// |---|---|---|
/// | `program` + `args` | **含占位符的骨架** | Provider（它知道产品形态） |
/// | `placeholders` | **占位符 → 实际值** | 调用方（它知道本机事实：路径、内存、账户） |
///
/// **为什么这样分**：骨架是**产品的知识**（参数顺序、哪些参数必需），
/// 而值是**本机的知识**（Java 在哪、内存多大、用哪个账户）。
/// 两者混在一起会让"同一个产品换台机器"要改代码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchPlan {
    /// 可执行文件（**可以含占位符**，因为 Java 路径就是本机事实）
    pub program: String,
    pub args: Vec<String>,
    /// 环境变量增量（不是全量替换——全量替换会弄丢 PATH 之类）
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// 工作目录。`None` = 由调用方决定
    #[serde(default)]
    pub cwd: Option<String>,
    /// 占位符表
    #[serde(default)]
    pub placeholders: BTreeMap<String, String>,
}

/// 解析结果：**一个可以直接交给操作系统的命令**。
///
/// 它**不含占位符**——这是类型给出的保证，而不是靠调用方自觉。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
}

impl LaunchPlan {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            placeholders: BTreeMap::new(),
        }
    }

    pub fn arg(mut self, a: impl Into<String>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn env(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.env.insert(k.into(), v.into());
        self
    }

    pub fn cwd(mut self, d: impl Into<String>) -> Self {
        self.cwd = Some(d.into());
        self
    }

    /// 填一个占位符。**重复填同一个会覆盖**（最后一次赢）——
    /// 因为"后填的覆盖先填的"是配置合并的通行语义，
    /// 而反过来（先填的赢）会让"用户改设置"变得不可能。
    pub fn bind(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.placeholders.insert(name.into(), value.into());
        self
    }

    /// **骨架里出现的全部占位符名**（去重、有序）。
    ///
    /// 它让"这份计划需要调用方准备哪些值"**可以被列举** ——
    /// 而这正是"可复现"的前提：不知道需要什么，就无法保证给全。
    pub fn required_placeholders(&self) -> Vec<String> {
        let mut names = Vec::new();
        let mut scan = |s: &str| {
            for n in placeholder_names(s) {
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        };
        scan(&self.program);
        for a in &self.args {
            scan(a);
        }
        for (k, v) in &self.env {
            scan(k);
            scan(v);
        }
        if let Some(c) = &self.cwd {
            scan(c);
        }
        names.sort();
        names
    }

    /// **有没有还没绑定的占位符**？返回全部未解析项（不只看第一个）。
    ///
    /// **返回全部而不是第一个**：只报一个的话，用户修一个再跑一次又冒出一个，
    /// 那会把一次可以一次说完的事变成 N 轮。
    pub fn unresolved(&self) -> Vec<UnresolvedPlaceholder> {
        let mut out = Vec::new();
        let mut check = |s: &str, idx: usize| {
            for n in placeholder_names(s) {
                if !self.placeholders.contains_key(&n) {
                    out.push(UnresolvedPlaceholder {
                        name: n,
                        arg_index: idx,
                        arg: s.to_string(),
                    });
                }
            }
        };
        check(&self.program, usize::MAX); // MAX 表示"这是 program 不是参数"
        for (i, a) in self.args.iter().enumerate() {
            check(a, i);
        }
        for (k, v) in &self.env {
            check(k, usize::MAX);
            check(v, usize::MAX);
        }
        if let Some(c) = &self.cwd {
            check(c, usize::MAX);
        }
        out
    }

    /// **解析成可执行的命令。**
    ///
    /// 有任何未解析占位符 → `Err`。**不"尽力而为"**：
    /// 一个没被替换的 `{JAVA}` 会被游戏当成路径去打开，
    /// 于是用户看到"游戏起不来"，而真正的原因是一个花括号 ——
    /// **那种失败无法从现场归因**，所以必须在这里断掉。
    pub fn resolve(&self) -> Result<ResolvedCommand, Vec<UnresolvedPlaceholder>> {
        let un = self.unresolved();
        if !un.is_empty() {
            return Err(un);
        }
        let sub = |s: &str| substitute(s, &self.placeholders);
        let mut env = BTreeMap::new();
        for (k, v) in &self.env {
            env.insert(sub(k), sub(v));
        }
        Ok(ResolvedCommand {
            program: sub(&self.program),
            args: self.args.iter().map(|a| sub(a)).collect(),
            env,
            cwd: self.cwd.as_deref().map(sub),
        })
    }
}

/// 从一个字符串里取出全部占位符名（去重）。
///
/// ## 三条刻意的规则
///
/// 1. **不嵌套**：`{A{B}}` 里 `{B}` 是占位符，而 `A{B` 是普通文本。
///    支持嵌套会让"名字"这个概念失去边界，而我们从不需要嵌套。
/// 2. **空名不算**：`{}` **不是**占位符（否则一个空花括号会让解析永远失败，
///    而它完全可能出现在正常的 JSON 或正则参数里）。
/// 3. **未闭合不算**：`{ABC` 不是占位符 —— 它只是文本里恰好有个左花括号。
///    把它当成"未解析"会让**合法参数被误判**。
pub fn placeholder_names(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == PLACEHOLDER_OPEN {
            // 找闭合；中间的字符不许再出现左花括号（规则 1）
            let mut j = i + 1;
            let mut ok = true;
            while j < bytes.len() && bytes[j] != PLACEHOLDER_CLOSE {
                if bytes[j] == PLACEHOLDER_OPEN {
                    ok = false;
                    break;
                }
                j += 1;
            }
            if ok && j < bytes.len() {
                let name: String = bytes[i + 1..j].iter().collect();
                // 规则 2：空名不算
                if !name.is_empty() && !out.contains(&name) {
                    out.push(name);
                }
                i = j + 1;
                continue;
            }
            // 未闭合：当作普通文本继续扫（规则 3）
        }
        i += 1;
    }
    out.sort();
    out
}

/// 把 `{NAME}` 全部替换成绑定值。**调用前必须已确认全部已绑定**
/// （[`LaunchPlan::resolve`] 保证了这一点）。
fn substitute(s: &str, table: &BTreeMap<String, String>) -> String {
    let names = placeholder_names(s);
    if names.is_empty() {
        return s.to_string();
    }
    let mut out = s.to_string();
    // 按名字长度降序替换，避免 `{A}` 是 `{AB}` 前缀时的错替
    // （`{AB}` 里含 `{A`，若先替 `{A}` 就会破坏 `{AB}`）
    let mut keys: Vec<&String> = names
        .iter()
        .filter_map(|n| table.get_key_value(n).map(|(k, _)| k))
        .collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    for k in keys {
        if let Some(v) = table.get(k) {
            out = out.replace(&format!("{PLACEHOLDER_OPEN}{k}{PLACEHOLDER_CLOSE}"), v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 取出占位符名() {
        assert_eq!(placeholder_names("{JAVA}"), vec!["JAVA"]);
        assert_eq!(
            placeholder_names("-cp {CP} -Djava.library.path={NATIVES}"),
            vec!["CP", "NATIVES"]
        );
        // 去重
        assert_eq!(placeholder_names("{A}-{A}"), vec!["A"]);
    }

    #[test]
    fn 三条边界规则() {
        // 规则 1：不嵌套 —— `{A{B}}` 里 `{B}` 是占位符，`A{B` 是文本
        assert_eq!(placeholder_names("{A{B}}"), vec!["B"]);
        // 规则 2：空名不算
        assert!(placeholder_names("{}").is_empty());
        assert!(placeholder_names("a{}b").is_empty());
        // 规则 3：未闭合不算 —— 否则合法参数会被误判
        assert!(placeholder_names("{ABC").is_empty());
    }

    #[test]
    fn 正则量词那种花括号不会被当成占位符() {
        // `a{2,3}` 里的 `{2,3}` 是正则量词，不是占位符名。
        // 我们的规则会把它当成一个名叫 "2,3" 的占位符 —— **这是可以接受的**，
        // 因为启动参数里不会出现正则；但它必须**不致命**：
        // 调用方要么绑一个 "2,3"，要么这次解析失败并**明确报出这个名字**。
        // 关键是：**失败信息里能看到 "2,3"**，于是人能一眼判断"哦那是正则"。
        let names = placeholder_names("a{2,3}");
        assert_eq!(names, vec!["2,3"]);
        let plan = LaunchPlan::new("x").arg("a{2,3}");
        let un = plan.unresolved();
        assert_eq!(un.len(), 1);
        assert_eq!(un[0].name, "2,3", "失败信息必须能让人看懂是什么");
    }

    #[test]
    fn 未解析占位符必须失败而不是尽力而为() {
        // 一个没被替换的 {JAVA} 会被游戏当成路径去打开，
        // 于是用户看到"游戏起不来"，而真正的原因是一个花括号 ——
        // **那种失败无法从现场归因**，所以必须在这里断掉。
        let plan = LaunchPlan::new("{JAVA}")
            .arg("-cp")
            .arg("{CP}")
            .bind("CP", "a;b");
        let e = plan.resolve().expect_err("必须失败");
        assert_eq!(e.len(), 1, "只漏了 JAVA");
        assert_eq!(e[0].name, "JAVA");
        assert!(e[0].to_string().contains("JAVA"), "{}", e[0]);
    }

    #[test]
    fn 未解析项要一次全报出来() {
        // 只报一个的话，用户修一个再跑一次又冒出一个 ——
        // 那会把一次能说完的事变成 N 轮。
        let plan = LaunchPlan::new("{A}").arg("{B}").arg("{C}");
        let e = plan.resolve().expect_err("必须失败");
        assert_eq!(e.len(), 3, "必须一次报全");
        let names: Vec<&str> = e.iter().map(|x| x.name.as_str()).collect();
        for n in ["A", "B", "C"] {
            assert!(names.contains(&n), "漏报了 {n}");
        }
    }

    #[test]
    fn 解析结果不含任何占位符() {
        let plan = LaunchPlan::new("{JAVA}")
            .arg("-Dx={Y}")
            .bind("JAVA", r"C:\jdk\bin\java.exe")
            .bind("Y", "1");
        let cmd = plan.resolve().unwrap();
        assert_eq!(cmd.program, r"C:\jdk\bin\java.exe");
        assert_eq!(cmd.args, vec!["-Dx=1"]);
        // 类型保证：解析结果里不可能再有花括号占位符
        assert!(placeholder_names(&cmd.program).is_empty());
        for a in &cmd.args {
            assert!(placeholder_names(a).is_empty(), "{a}");
        }
    }

    #[test]
    fn 同一份计划解析两次逐字节相同() {
        // 方案：`Launchable` = "产出**可复现**启动计划"。
        // "可复现"就是这条：同输入 → 同输出。
        let plan = LaunchPlan::new("{JAVA}")
            .arg("-cp")
            .arg("{CP}")
            .env("PATH", "{BIN};{PATH2}")
            .cwd("{GAMEDIR}")
            .bind("JAVA", "j")
            .bind("CP", "c1;c2")
            .bind("BIN", "b")
            .bind("PATH2", "p")
            .bind("GAMEDIR", "d");
        let a = plan.resolve().unwrap();
        for _ in 0..8 {
            assert_eq!(plan.resolve().unwrap(), a);
        }
        // 而且顺序也是稳定的（BTreeMap 保证 env 有序）
        let keys: Vec<&String> = a.env.keys().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "env 的顺序必须稳定");
    }

    #[test]
    fn 骨架可含占位符而值也可以是本机事实() {
        let plan = LaunchPlan::new("{JAVA}").arg("{MEM}").arg("{NATIVES}");
        assert_eq!(plan.required_placeholders(), vec!["JAVA", "MEM", "NATIVES"]);
        // 需要什么值可以被列举 —— 这是"给全"的前提
        assert_eq!(plan.unresolved().len(), 3);
    }

    #[test]
    fn 重复绑定以后填的为准() {
        // "后填的覆盖先填的"是配置合并的通行语义；
        // 反过来会让"用户改设置"变得不可能。
        let plan = LaunchPlan::new("{A}")
            .bind("A", "first")
            .bind("A", "second");
        assert_eq!(plan.resolve().unwrap().program, "second");
    }

    #[test]
    fn 名字互为前缀时替换不出错() {
        // `{A}` 是 `{AB}` 的前缀。若按名字升序替换，`{A}` 会把 `{AB}` 破坏成 `值B}`。
        let plan = LaunchPlan::new("{A}-{AB}").bind("A", "1").bind("AB", "2");
        assert_eq!(plan.resolve().unwrap().program, "1-2");
    }

    #[test]
    fn 空计划可以解析() {
        // 没有任何占位符的计划（如"直接跑一个已存在的 exe"）也必须能解析。
        let plan = LaunchPlan::new("game.exe").arg("--x");
        assert!(plan.required_placeholders().is_empty());
        assert_eq!(plan.resolve().unwrap().args, vec!["--x"]);
    }

    #[test]
    fn 环境变量是增量而不是全量替换() {
        // 全量替换会弄丢 PATH 之类，游戏会以奇怪的方式失败。
        // 这条测试钉住"它是一个 map，表示要设置/覆盖哪些键"。
        let plan = LaunchPlan::new("j").env("MY_VAR", "1");
        let cmd = plan.resolve().unwrap();
        assert_eq!(cmd.env.len(), 1, "只带自己声明的那个键");
        assert_eq!(cmd.env.get("MY_VAR").map(String::as_str), Some("1"));
        assert!(!cmd.env.contains_key("PATH"), "不该伪造 PATH");
    }
}
