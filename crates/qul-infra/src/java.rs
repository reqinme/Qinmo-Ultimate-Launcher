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
            if !line.contains("REG_SZ") {
                continue;
            }
            let Some(idx) = line.find("REG_SZ") else {
                continue;
            };
            let value = line[idx + "REG_SZ".len()..].trim();
            if value.is_empty() {
                continue;
            }
            let base = PathBuf::from(value);
            let mut hit = false;
            for name in java_exe_names() {
                let p = base.join("bin").join(name);
                if looks_like_java(&p) {
                    out.sources.push(DiscoverySource {
                        kind: format!("registry:{key}"),
                        path: p.display().to_string(),
                    });
                    hit = true;
                }
            }
            if !hit {
                out.skipped.push((
                    value.to_string(),
                    "注册表登记的 JavaHome 下没有 bin\\java.exe".into(),
                ));
            }
        }
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
