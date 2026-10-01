//! # qul-infra —— 基础设施
//!
//! **一切"碰外部世界"的东西都在这一层**：文件系统、注册表、子进程、网络。
//!
//! `qul-core` 由架构测试保证不碰这些；本层是它的对面。
//! 判据很简单：**如果一段代码需要真的去问操作系统，它属于这里。**
//!
//! ## 本模块：Java 运行时探测
//!
//! 对应方案 M0 尖刺 **S6**。它只做两件事：
//! 1. **发现**本机有哪些 Java（多路并行，见 [`discover`]）
//! 2. **问出**每个候选的版本 / 位数 / 厂商（[`probe`]）
//!
//! **"该选哪个"不在这里**——那是 `qul_core::java::choose_java` 的事。
//! 分开是为了让规则可被穷举测试，而这一层只能实测。

use qul_core::java::JavaCandidate;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 一个候选是怎么被找到的。**必须记录**——
/// 因为 S6 的通过标准包含"无漏报"，而漏报只有靠来源清单才能发现。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverySource {
    /// 来源类别（`registry` / `env` / `path` / `common-dir` / `manual`）
    pub kind: String,
    /// 该来源给出的路径
    pub path: String,
}

/// 探测结果：候选 + 它们的来源 + 被跳过的路径及原因。
#[derive(Debug, Clone, Default)]
pub struct Discovery {
    pub candidates: Vec<JavaCandidate>,
    pub sources: Vec<DiscoverySource>,
    /// **被跳过的东西及原因。**
    ///
    /// 这一项是刻意保留的：S6 要求"无漏报"，
    /// 而"某个路径被跳过了"与"那个路径不存在"是两件事——
    /// 没有这份记录，漏报就查不出来。
    pub skipped: Vec<(String, String)>,
}

/// `java.exe` / `javaw.exe` 的文件名候选。
fn java_exe_names() -> [&'static str; 2] {
    ["java.exe", "javaw.exe"]
}

/// 判断一个路径是否可执行文件（只做存在性 + 文件名检查，不执行）。
fn looks_like_java(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    match path.file_name().and_then(|s| s.to_str()) {
        Some(n) => java_exe_names().iter().any(|x| n.eq_ignore_ascii_case(x)),
        None => false,
    }
}

/// 从 `JAVA_HOME` / `JDK_HOME` 这类环境变量取候选。
fn from_env(out: &mut Discovery) {
    for var in ["JAVA_HOME", "JDK_HOME", "JRE_HOME"] {
        if let Some(v) = std::env::var_os(var) {
            let base = PathBuf::from(&v);
            let mut hit = false;
            for name in java_exe_names() {
                let p = base.join("bin").join(name);
                if looks_like_java(&p) {
                    out.sources.push(DiscoverySource {
                        kind: format!("env:{var}"),
                        path: p.display().to_string(),
                    });
                    hit = true;
                }
            }
            if !hit {
                out.skipped.push((
                    base.display().to_string(),
                    format!("{var} 指向的目录下没有 bin\\java.exe"),
                ));
            }
        }
    }
}

/// 从 `PATH` 逐段找。
fn from_path(out: &mut Discovery) {
    let Some(path) = std::env::var_os("PATH") else {
        out.skipped.push(("PATH".into(), "环境变量不存在".into()));
        return;
    };
    for dir in std::env::split_paths(&path) {
        for name in java_exe_names() {
            let p = dir.join(name);
            if looks_like_java(&p) {
                out.sources.push(DiscoverySource {
                    kind: "path".into(),
                    path: p.display().to_string(),
                });
            }
        }
    }
}

/// 常见安装根目录（Oracle / Adoptium / Microsoft / Amazon / Zulu / Liberica / 系统）。
///
/// **只扫一层子目录**：这些厂商的习惯是 `<root>/<产品名>-<版本>/bin/java.exe`。
/// 深扫会拖慢启动，而 S6 明确说"扫不到的绿色版"作为已知限制并用手动指定兜底。
fn common_roots() -> Vec<(String, PathBuf)> {
    let mut roots = Vec::new();
    let mut push = |kind: &str, p: PathBuf| roots.push((kind.to_string(), p));

    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Some(v) = std::env::var_os(var) {
            let base = PathBuf::from(v);
            // Deliberately NOT "Microsoft": that matches Microsoft Office,
            // Microsoft SQL Server, Microsoft Visual Studio ... none of which are
            // Java. The first run of this probe scanned all of them. A vendor
            // name is only useful here if it cannot also name a non-Java product.
            for vendor in [
                "Java",
                "Eclipse Adoptium",
                "Eclipse Foundation",
                "Amazon Corretto",
                "Zulu",
                "BellSoft",
                "Semeru",
                "AdoptOpenJDK",
                "Microsoft\\jdk",
            ] {
                push(&format!("common:{vendor}"), base.join(vendor));
            }
        }
    }
    if let Some(v) = std::env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(v);
        for vendor in [
            "Programs\\Eclipse Adoptium",
            "Programs\\Java",
            "Programs\\Zulu",
        ] {
            push("common:localappdata", base.join(vendor));
        }
    }
    roots
}

/// 扫常见根目录（一层深，另加一层处理 `jdk-21.0.5+11` 这类带版本号的子目录）。
fn from_common_dirs(out: &mut Discovery) {
    for (kind, root) in common_roots() {
        if !root.is_dir() {
            continue;
        }
        let mut check = |dir: &Path| {
            for name in java_exe_names() {
                let p = dir.join(name);
                if looks_like_java(&p) {
                    out.sources.push(DiscoverySource {
                        kind: kind.clone(),
                        path: p.display().to_string(),
                    });
                }
            }
        };
        check(&root.join("bin"));
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    check(&p.join("bin"));
                }
            }
        }
    }
}

/// `reg query /s` 输出里的一行，**判定它是哪一类**。
///
/// 见 [`parse_reg_value_line`]。
#[derive(Debug, Clone, PartialEq, Eq)]
enum RegValue {
    /// 不是 `REG_SZ` 的值行（键名行、空行）
    NotAValue,
    /// 是值行，但**值名不是指向安装目录的那两个** ——
    /// 例如 `CurrentVersion` / `NOSTARTMENU`。它们不是候选路径。
    NotAPath,
    /// 指向 Java 安装目录的一行
    Path { name: String, value: String },
}

/// **解析 `reg query /s` 输出里的一行。**
///
/// ## 它为什么是一个独立函数
///
/// 因为它的判据被一个真 bug 修过，而**那个 bug 的成因必须在源码里看得见**：
///
/// `reg query /s` 会输出该键下的**所有**值。第一版对每一行只看
/// `REG_SZ` 后面那一段，于是把 `CurrentVersion  REG_SZ  25.0.4.1`、
/// `NOSTARTMENU  REG_SZ  0`、`Version  REG_SZ  1.8` 全当成候选路径，
/// 然后每一个都报「注册表登记的 JavaHome 下没有 bin\java.exe」。
///
/// 那句诊断**在说谎**：那些值从来不是路径。而后果不只是刷屏 ——
/// 它把 21 条噪声写进了"漏报台账"，而那份台账的作用恰恰是
/// **查我们漏了什么**。噪声进台账，查漏报时就会看错方向。
///
/// ## 判据（实测 `reg query HKLM\SOFTWARE\JavaSoft\JDK /s` 的输出）
///
/// ```text
///     CurrentVersion    REG_SZ    25.0.4.1                              <- 不是路径
///     JavaHome          REG_SZ    C:\Program Files\Java\jdk-21.0.12.1  <- 路径
///     NOSTARTMENU       REG_SZ    0                                     <- 不是路径
///     INSTALLDIR        REG_SZ    C:\Program Files\Java\jdk-21.0.12.1\ <- 路径（MSI 子键）
/// ```
///
/// 所以只认 `JavaHome` 与 `INSTALLDIR` 两个**值名**。
///
/// ## 它不做的事
///
/// **不去判断那个值"看起来像不像一条路径"** —— 一个靠 `contains('\\')`
/// 猜的判据会在路径形态变化时静默失效，而值名是格式的一部分，稳定得多。
/// 真正的验证交给下一步（那里会去实际找 `bin\java.exe`）。
fn parse_reg_value_line(line: &str) -> RegValue {
    let Some(idx) = line.find("REG_SZ") else {
        return RegValue::NotAValue;
    };
    let name = line[..idx].trim();
    let value = line[idx + "REG_SZ".len()..].trim();
    if value.is_empty() {
        return RegValue::NotAValue;
    }
    if name != "JavaHome" && name != "INSTALLDIR" {
        return RegValue::NotAPath;
    }
    RegValue::Path {
        name: name.to_string(),
        value: value.to_string(),
    }
}

/// 从注册表读 `JavaSoft` 的安装项。
///
/// 用 `reg.exe query` 而不是引入 `winreg` 依赖：
/// **这是尖刺**，多一条依赖就多一条要审的许可；
/// 而 `reg` 是系统自带、输出稳定、失败时返回非零可判。
fn from_registry(out: &mut Discovery) {
    // 覆盖面：三处根 + 32 位视图（WOW6432Node）。
    // 32 位 Java 也要收——S6 要求"无漏报"，
    // 而"只有 32 位 Java"正是最需要说清的情形。
    let keys = [
        r"HKLM\SOFTWARE\JavaSoft\Java Runtime Environment",
        r"HKLM\SOFTWARE\JavaSoft\Java Development Kit",
        r"HKLM\SOFTWARE\JavaSoft\JRE",
        r"HKLM\SOFTWARE\JavaSoft\JDK",
        r"HKLM\SOFTWARE\WOW6432Node\JavaSoft\Java Runtime Environment",
        r"HKLM\SOFTWARE\WOW6432Node\JavaSoft\Java Development Kit",
        r"HKLM\SOFTWARE\WOW6432Node\JavaSoft\JRE",
        r"HKLM\SOFTWARE\WOW6432Node\JavaSoft\JDK",
    ];
    // 有多少个注册表值**不是**路径（版本号、开关）。它们汇总成**一条**记录。
    //
    // **它在循环外**：第一版把它写在 `for key` 里，于是末尾那段汇总引用了
    // 一个已经出作用域的名字。编译器拦住了，而它值得记一句：
    // **跨迭代的汇总必须声明在迭代之外** —— 否则那段汇总要么编译不过，
    // 要么（如果它当时能编译）变成"每个键各报一条"。
    let mut registry_non_path_values = 0usize;
    for key in keys {
        let outq = Command::new("reg").args(["query", key, "/s"]).output();
        let Ok(o) = outq else {
            out.skipped.push((key.into(), "reg query 无法执行".into()));
            continue;
        };
        if !o.status.success() {
            // 键不存在是正常的：多数机器只装一种 Java。
            // 但"命令失败"与"键不存在"要分开记，否则查漏报时会看错方向。
            out.skipped
                .push((key.into(), "注册表键不存在或无权限".into()));
            continue;
        }
        let text = String::from_utf8_lossy(&o.stdout);
        for line in text.lines() {
            let line = line.trim();
            // 形如：    JavaHome    REG_SZ    C:\Program Files\Java\jre1.8.0_442
            //
            // -----------------------------------------------------------------
            // ⚠️ 这里必须**同时**取"值名"与"值"，而第一版只取了值 ——
            // 那是一个真 bug，而它是被 CLI 的输出暴露的：
            //
            // `reg query /s` 会输出该键下的**所有**值，于是
            // `CurrentVersion  REG_SZ  25.0.4.1` 与 `NOSTARTMENU  REG_SZ  0`
            // 也被当成候选路径，然后每一个都报
            // 「注册表登记的 JavaHome 下没有 bin\java.exe」——
            // **一条在说谎的诊断**：那些值从来不是路径。
            //
            // 后果不只是刷屏：那条记录会把"26 个跳过"写进漏报台账，
            // 而其中 21 个根本不是候选。一个把噪声记成缺陷的台账，
            // 会在真正要查漏报时把人引到错方向。
            //
            // 所以现在只认**确实指向 Java 安装目录的两个值名**：
            //   `JavaHome`    —— JavaSoft 键的标准值名
            //   `INSTALLDIR`  —— MSI 子键下记录的同一个目录
            // 其余值名**计数但不逐个记账**，因为它们不是候选。
            //
            // 判据被抽成 `parse_reg_value_line` 并由单测钉住 ——
            // 否则"哪天有人把值名判断删掉"会没有任何测试会红。
            let (name, value) = match parse_reg_value_line(line) {
                RegValue::NotAValue => continue,
                RegValue::NotAPath => {
                    registry_non_path_values += 1;
                    continue;
                }
                RegValue::Path { name, value } => (name, value),
            };
            let base = PathBuf::from(&value);
            let mut hit = false;
            for name_exe in java_exe_names() {
                let p = base.join("bin").join(name_exe);
                if looks_like_java(&p) {
                    out.sources.push(DiscoverySource {
                        kind: format!("registry:{key}"),
                        path: p.display().to_string(),
                    });
                    hit = true;
                }
            }
            if !hit {
                // **这一条是真信息**：注册表明确说 Java 在这里，而那里没有 java.exe。
                // 它与"注册表里有个版本号"是不同的两件事，所以分开记。
                out.skipped.push((
                    value.to_string(),
                    format!("注册表的值 `{name}` 指向这里，但下面没有 bin\\java.exe"),
                ));
            }
        }
    }
    if registry_non_path_values > 0 {
        // 汇总成一条，而不是 N 条。**"不是候选"与"跳过了一个候选"是两件事。**
        out.skipped.push((
            format!("（{registry_non_path_values} 个非路径值）"),
            "注册表里的版本号/开关等值（如 CurrentVersion / NOSTARTMENU）—— \
             它们从来不是 Java 安装路径，所以不计为候选"
                .into(),
        ));
    }
}

/// **S6 第 1 步**：扫描本机全部 Java。
///
/// 四路并行（注册表 / 环境变量 / PATH / 常见目录），最后按**规范化路径**去重。
///
/// 去重必须在**探测之后**：同一个 `java.exe` 会被多路命中（例如既在 PATH 又在注册表），
/// 不去重会让"发现 6 个 Java"这种数字虚高，而那个数字要写进结论。
pub fn discover() -> Discovery {
    let mut d = Discovery::default();
    from_registry(&mut d);
    from_env(&mut d);
    from_path(&mut d);
    from_common_dirs(&mut d);

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut uniq_sources: Vec<DiscoverySource> = Vec::new();
    for s in &d.sources {
        // 规范化：小写 + 去掉 `javaw.exe`（它和 `java.exe` 同目录，属同一个运行时）
        let dir = Path::new(&s.path)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let key = dir.to_lowercase();
        if seen.insert(key) {
            uniq_sources.push(s.clone());
        }
    }
    d.sources = uniq_sources;
    d
}

/// 从 `<java> -XshowSettings:properties -version` 的输出里取一个属性。
fn prop(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start_matches([' ', '=']).trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// **S6 第 2 步**：问一个可执行文件"你是谁"。
///
/// 为什么不用 `-version` 单独解析：它的输出格式**跨厂商与年代都不一样**
/// （`java version "1.8.0_442"` vs `openjdk version "21.0.5"`）。
/// `-XshowSettings:properties` 给出的是**结构化**的 `java.version` / `java.vendor` /
/// `sun.arch.data.model`，解析更稳；`-version` 只作为**兜底与交叉验证**。
///
/// 失败返回 `Err(原因)` —— **不许静默丢**：一个探测失败的候选必须能被看到，
/// 否则"发现 3 个 Java"可能其实是"发现 4 个、其中 1 个问不出来"。
pub fn probe(java_path: &str) -> Result<JavaCandidate, String> {
    let out = Command::new(java_path)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .map_err(|e| format!("无法执行 {java_path}：{e}"))?;

    // `-version` 系列把版本信息写到 **stderr**，不是 stdout。
    // 这一点踩过才知道：只看 stdout 会得到空串。
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let version = prop(&text, "java.version")
        // 兜底：从 `version "1.8.0_442"` 里抓引号内容
        .or_else(|| {
            text.lines()
                .find(|l| l.contains("version \""))
                .and_then(|l| l.split('"').nth(1))
                .map(|s| s.to_string())
        })
        .ok_or_else(|| format!("{java_path} 的输出里找不到 java.version"))?;

    // 主版本：`1.8.0_442` -> 8；`21.0.5` -> 21
    let major = {
        let mut it = version.split('.');
        let first = it.next().unwrap_or("");
        let second = it.next();
        if first == "1" {
            second
                .and_then(|s| s.split(['_', '-', '+']).next())
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| format!("无法从版本串 {version} 解析主版本"))?
        } else {
            first
                .split(['_', '-', '+'])
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or_else(|| format!("无法从版本串 {version} 解析主版本"))?
        }
    };

    let bits = prop(&text, "sun.arch.data.model")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or_else(|| {
            // 兜底：从 `java.vm.name` 里的 `64-Bit` 判断（旧版 JDK 有这种写法）
            if text.contains("64-Bit") {
                64
            } else {
                32
            }
        });

    let vendor = prop(&text, "java.vendor").unwrap_or_else(|| "未知厂商".into());

    Ok(JavaCandidate {
        path: java_path.to_string(),
        major,
        version,
        bits,
        vendor,
    })
}

/// 发现 + 探测一步到位（**穷尽探测**：每个候选都问一次，失败也记下来）。
pub fn discover_and_probe() -> (Discovery, Vec<(String, String)>) {
    let mut d = discover();
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut ok: Vec<JavaCandidate> = Vec::new();
    for s in &d.sources {
        match probe(&s.path) {
            Ok(c) => ok.push(c),
            Err(e) => failed.push((s.path.clone(), e)),
        }
    }
    d.candidates = ok;
    (d, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────────── `reg query` 行解析 ─────────────────────
    //
    // 这一组测试钉住的是一个**被 CLI 输出暴露出来的真 bug**：
    // 第一版把 `reg query /s` 输出里的**每一个 `REG_SZ` 值**都当成候选路径，
    // 于是注册表里的版本号与开关全成了"跳过"，
    // 而每一条都报「注册表登记的 JavaHome 下没有 bin\java.exe」——
    // **一句在说谎的诊断**。
    //
    // 它把 21 条噪声写进了"漏报台账"，而那份台账的作用恰恰是查我们漏了什么。

    #[test]
    fn java_home_line_is_a_path() {
        let line = r"    JavaHome    REG_SZ    C:\Program Files\Java\jdk-21.0.12.1";
        match parse_reg_value_line(line) {
            RegValue::Path { name, value } => {
                assert_eq!(name, "JavaHome");
                assert_eq!(value, r"C:\Program Files\Java\jdk-21.0.12.1");
            }
            other => panic!("应当是路径：{other:?}"),
        }
    }

    #[test]
    fn install_dir_line_is_a_path() {
        // MSI 子键下记录的同一个目录，**末尾带反斜杠** —— 那一份也要收
        let line = r"    INSTALLDIR    REG_SZ    C:\Program Files\Java\jdk-21.0.12.1\";
        match parse_reg_value_line(line) {
            RegValue::Path { name, value } => {
                assert_eq!(name, "INSTALLDIR");
                assert!(value.ends_with('\\'));
            }
            other => panic!("应当是路径：{other:?}"),
        }
    }

    #[test]
    fn version_and_switch_values_are_not_paths() {
        // ⚠️ **这三行就是那个 bug 的现场。**
        // 实测 `reg query HKLM\SOFTWARE\JavaSoft\JDK /s` 会输出它们，
        // 而它们**从来不是** Java 安装路径。
        for line in [
            r"    CurrentVersion    REG_SZ    25.0.4.1",
            r"    NOSTARTMENU    REG_SZ    0",
            r"    Version    REG_SZ    1.8",
            r"    DisplayName    REG_SZ    Java 8 Update 503",
            r"    Path    REG_SZ    C:\Program Files\Common Files\Oracle\Java\javapath",
        ] {
            assert_eq!(
                parse_reg_value_line(line),
                RegValue::NotAPath,
                "**这一行不是候选路径**，而它在第一版里被当成了路径：{line}"
            );
        }
        // 最后那一条尤其值得留意：它**看起来像路径**（含反斜杠），
        // 而它的值名是 `Path` —— 那是"启动器存根"目录，不是 Java 安装目录。
        // 判据用**值名**而不是"像不像路径"，正是为了不在这里猜。
    }

    #[test]
    fn key_header_lines_and_blank_lines_are_not_values() {
        for line in [
            r"HKEY_LOCAL_MACHINE\SOFTWARE\JavaSoft\JDK",
            r"HKEY_LOCAL_MACHINE\SOFTWARE\JavaSoft\JDK\21.0.12.1",
            "",
            "    ",
        ] {
            assert_eq!(
                parse_reg_value_line(line),
                RegValue::NotAValue,
                "键名行/空行不该被当成值：{line:?}"
            );
        }
    }

    #[test]
    fn other_reg_types_are_not_values() {
        // `reg query` 还会输出 REG_DWORD / REG_EXPAND_SZ / REG_MULTI_SZ。
        // 第一版只看 `"REG_SZ"` 子串，于是把 `REG_EXPAND_SZ` 也匹配上了
        // （它**包含** `REG_SZ` 吗？不 —— 但 `REG_SZ` 出现在
        // `REG_EXPAND_SZ` 里的位置不同，所以这里显式钉住形态）。
        for line in [
            r"    Enabled    REG_DWORD    0x1",
            r"    Foo    REG_MULTI_SZ    a\0b",
        ] {
            assert_eq!(parse_reg_value_line(line), RegValue::NotAValue, "{line}");
        }
    }

    #[test]
    fn empty_value_is_not_a_value() {
        // `JavaHome  REG_SZ  ` （值名对但值为空）：**不是路径**，也不是"非路径值"。
        // 它落进 `NotAValue`，于是不会被记两次。
        assert_eq!(
            parse_reg_value_line(r"    JavaHome    REG_SZ    "),
            RegValue::NotAValue
        );
    }

    #[test]
    fn value_name_is_matched_exactly_not_as_substring() {
        // 一个用 `contains("Home")` 之类判据的实现会在将来误收别的值。
        // 断言的判据是**精确相等**。
        for line in [
            r"    JavaHomeBackup    REG_SZ    C:\x",
            r"    MyINSTALLDIR    REG_SZ    C:\y",
            r"    JavaHome    REG_SZ    ",
        ] {
            assert_ne!(
                std::mem::discriminant(&parse_reg_value_line(line)),
                std::mem::discriminant(&RegValue::Path {
                    name: String::new(),
                    value: String::new()
                }),
                "值名必须精确匹配：{line}"
            );
        }
        // 而精确的那个仍然是路径
        assert!(matches!(
            parse_reg_value_line(r"    JavaHome    REG_SZ    C:\z"),
            RegValue::Path { .. }
        ));
    }
}
