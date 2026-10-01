//! # Java 运行时要求与选择规则（**纯规则，零 IO**）
//!
//! 这一层只回答两个问题：
//! 1. **某个游戏版本需要哪一档 Java？**
//! 2. **在若干候选里，哪一个该被选中？**
//!
//! **它不认识文件系统、不认识注册表、不认识进程**——那些在 `qul-infra`。
//! 这个切分不是为了好看：**"该选哪个"是可以被穷举验证的规则**，
//! 而"本机装了哪些"只能实测。混在一起就没法对规则本身写穷举测试。
//!
//! ## 为什么这件事值得单独成模块
//!
//! 方案 S6 的原话：**选错 Java 是"游戏起不来"的头号原因，且用户看不懂报错。**
//! 也就是说这里错了，用户拿到的是一个**他无法归因的失败**。
//! 所以本模块的目标不是"能选"，而是**"选不出来时能说清为什么"**。

use serde::{Deserialize, Serialize};
use std::fmt;

/// 一个 Java 候选的**已知属性**。
///
/// **没有 `path` 以外的 IO 信息**——路径只是个字符串，本模块不碰它。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaCandidate {
    /// 可执行文件路径（仅作为标识与展示）
    pub path: String,
    /// 主版本号（Java 8 = 8，Java 21 = 21）
    pub major: u32,
    /// 完整版本串（如 `21.0.5` / `1.8.0_442`）
    pub version: String,
    /// 64 位还是 32 位
    pub bits: u32,
    /// 厂商（如 `Eclipse Adoptium` / `Oracle Corporation`）
    pub vendor: String,
}

impl fmt::Display for JavaCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Java {} ({}, {} 位, {})",
            self.major, self.version, self.bits, self.vendor
        )
    }
}

// Java 需求档。**用档而不是用数字**，因为方案里写的是档位：
// "1.17+ 需 Java 16+，1.20.5+ 需 Java 21"。
//
// ---------------------------------------------------------------------------
// ⚠️ 这个枚举在 M2 被**开了一个口子**，理由是一条实测事实：
//
//   版本详情里的 `javaVersion.majorVersion` 会**随年代爬升**，而实测已经到
//   **25**（`26.3`，2026 年）。原先的封闭四档（最大 Java21）**表达不了它** ——
//   于是"从 JSON 读需求"会遇到一个无法表示的值。
//
// 按方案 §1.1.5 的判据（**会变的是配置/数据，不是编译期常量**），
// "那个数字会涨"正是它该是**数据**的理由。
//
// ## 为什么用"开一个口子"而不是"再加一档"
//
// 再加 `Java25` 会在 2027 年重演同一个问题 —— 而那正是
// **一个封闭枚举描述一个开放世界**的经典失效。
// `AtLeast(n)` 让"任何最低版本"都能被表达，**而命名的四档保留下来**：
// 它们是**可读的常用值**，且能让既有代码与文档继续说"Java 8 档"。
//
// ## 而没有改成"纯 u32"
//
// 因为 `Java16` 这一档**必须能与 `Java17` 区分** —— 见
// [`GameVersion::requirement`] 里 1.17 与 1.18 为什么要分开。
// 如果只留一个数字，那条区分就只能靠注释维持。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum JavaRequirement {
    /// Java 8 —— 1.16.5 及更早（含 1.12.2 / 1.8.9 / 1.7.10）
    Java8,
    /// Java 16+ —— 1.17 起（1.17 需要 16，1.18+ 实际要求 17）
    Java16,
    /// Java 17+ —— 1.18 起到 1.20.4
    Java17,
    /// Java 21+ —— 1.20.5 起到某个未来版本
    Java21,
    /// **任意最低主版本** —— 用于表达"清单里声明的那个数字"。
    ///
    /// 它存在是因为**实测那个数字已经涨到 25**，而将来还会涨。
    AtLeast(u32),
}

impl JavaRequirement {
    /// 任意最低主版本。
    pub const fn at_least(major: u32) -> Self {
        Self::AtLeast(major)
    }

    /// **声明的主版本 → 需求。**
    ///
    /// 这是"从清单读需求"的唯一入口，而它的语义刻意保持朴素：
    /// **声明多少就要多少**，不做"往上取整到某一档"的推断。
    ///
    /// ## 为什么不做取整
    ///
    /// 一个"看到 25 就归到最近的 Java21 档"的实现会让**需求被低估** ——
    /// 而后果是选出一个版本过低的 Java，然后游戏以一个难查的方式失败。
    /// **少要一个版本号是安全的，多要一个是不安全的。**
    ///
    /// 而 `None`（清单没声明）**不在这里兜底** —— 那件事属于
    /// [`GameVersion::requirement`] 的职责，且它必须能被调用方看见。
    pub const fn from_declared_major(major: u32) -> Self {
        Self::AtLeast(major)
    }

    /// 该档要求的最低主版本号。
    pub const fn min_major(self) -> u32 {
        match self {
            JavaRequirement::Java8 => 8,
            JavaRequirement::Java16 => 16,
            JavaRequirement::Java17 => 17,
            JavaRequirement::Java21 => 21,
            JavaRequirement::AtLeast(n) => n,
        }
    }

    /// 给人看的一句话（错误信息里要用）。
    ///
    /// **返回 `String` 而不是 `&'static str`**：`AtLeast(n)` 要拼出
    /// "Java 25 或更高"，而那是**运行期**才知道的内容。
    /// 第一版返回 `&'static str`，加上 `AtLeast` 之后无法表达。
    pub fn human(self) -> String {
        match self {
            JavaRequirement::Java8 => "Java 8".to_string(),
            JavaRequirement::Java16 => "Java 16 或更高".to_string(),
            JavaRequirement::Java17 => "Java 17 或更高".to_string(),
            JavaRequirement::Java21 => "Java 21 或更高".to_string(),
            JavaRequirement::AtLeast(n) => format!("Java {n} 或更高"),
        }
    }
}

/// 一个游戏版本对 Java 的要求。
///
/// **只认 `(主, 次, 修订)` 三个数**：Minecraft 的版本号是 `1.<minor>.<patch>`，
/// 而跨年代还有 `1.7.10` 这种；预发布/快照的尾缀（`-pre1`、`-rc1`）对 Java
/// 要求没有影响，故解析时丢弃。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    /// 原串，用于展示与错误信息
    pub raw: String,
}

impl GameVersion {
    /// 解析 `1.20.5` / `1.20.5-pre1` / `1.7.10`。
    ///
    /// 非法输入返回 `None`——**不猜**。猜错会让用户得到一个静默的错选择，
    /// 而"版本号解析不了"本身是必须告诉用户的事实。
    pub fn parse(s: &str) -> Option<Self> {
        let raw = s.trim().to_string();
        // 去掉预发布/候选/快照尾缀
        let core = raw
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() < 2 {
            return None;
        }
        let major = parts[0].parse::<u32>().ok()?;
        let minor = parts[1].parse::<u32>().ok()?;
        let patch = if parts.len() >= 3 {
            parts[2].parse::<u32>().unwrap_or(0)
        } else {
            0
        };
        Some(Self {
            major,
            minor,
            patch,
            raw,
        })
    }

    /// 该版本需要哪一档 Java。
    ///
    /// ## ⚠️ 它是**兜底**，而首选来源是版本详情里的声明
    ///
    /// 实测事实：现代版本的详情里**直接声明** `javaVersion.majorVersion`
    /// （`1.19.3` → 17、`26.3` → **25**），而老版本没有（`1.6.4` 完全没有）。
    ///
    /// 所以调用方**应当先用** [`crate::descriptor::Descriptor::java_requirement`]，
    /// 只在它落到 `GuessedFromVersionTable` / `UnknownVersionId` 时才用本函数。
    /// 本条表**只覆盖它实测能覆盖的范围**。
    ///
    /// ## 判据来源（方案 S6 与官方发布说明）
    ///
    /// | 游戏版本 | 要求 |
    /// |---|---|
    /// | ≤ 1.16.5 | Java 8 |
    /// | 1.17 | Java 16 |
    /// | 1.18 – 1.20.4 | Java 17 |
    /// | 1.20.5 – 1.20.x | Java 21 |
    /// | **≥ 1.21** | **`AtLeast(n)`** —— 实测里那个数字还在涨，所以不再钉死 |
    ///
    /// **1.17 与 1.18 分开是有意的**：把 1.17 归到 Java 17 会让 1.17 用户
    /// 在只有 Java 16 的机器上被误判为"缺 Java"；而 1.17 其实能用 16。
    /// **宁可少要一个版本，也不要把能跑的组合判成不能跑。**
    ///
    /// **1.21 起为什么改成 `AtLeast`**：实测 `1.21.4` 声明 Java 21 而
    /// `26.3` 声明 **25**。把它继续钉在 `Java21` 会让"表"与"事实"漂开，
    /// 而漂开的方向是**低估需求**。用 `AtLeast(n)` 表达"这一档往后的要求
    /// 就是它自己的次版本号"，于是表**不需要在每次 Java 升级时改**。
    pub fn requirement(&self) -> JavaRequirement {
        let (m, n, p) = (self.major, self.minor, self.patch);
        if m != 1 {
            // 未来的 2.x 及以后：**不猜**。给一个显然"需要人看"的值，
            // 而不是静默当作 1.x 的某一档。
            return JavaRequirement::AtLeast(n.max(21));
        }
        match n {
            0..=16 => JavaRequirement::Java8,
            17 => JavaRequirement::Java16,
            18..=19 => JavaRequirement::Java17,
            20 => {
                if p >= 5 {
                    JavaRequirement::Java21
                } else {
                    JavaRequirement::Java17
                }
            }
            // **1.21 起不再钉死。** 实测 `1.21.4` 声明 21、`26.3` 声明 **25**
            // —— 那个数字还在涨，所以表在这一档说的是"要求就是它自己的次版本号"。
            21..=24 => JavaRequirement::Java21,
            // 25 及以后：用它自己。这个分支**存在本身就是"表会漂"的证据**，
            // 而 `AtLeast` 让它漂得起。
            n => JavaRequirement::AtLeast(n),
        }
    }
}

/// 选择结果。**三态，而不是 `Option`** —— 因为"为什么没选出来"必须能说清。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JavaChoice {
    /// 选中了某个候选
    Selected {
        candidate: JavaCandidate,
        /// 为什么选它（例如"唯一满足 Java 21 的 64 位运行时"）
        reason: String,
    },
    /// 有候选，但没有一个满足要求
    NoneSatisfies {
        requirement: JavaRequirement,
        /// 本机已有的（用于提示"已找到 Java 8 与 17"）
        available: Vec<JavaCandidate>,
    },
    /// 一个候选都没有
    NoJavaAtAll { requirement: JavaRequirement },
}

impl JavaChoice {
    pub fn selected(&self) -> Option<&JavaCandidate> {
        match self {
            JavaChoice::Selected { candidate, .. } => Some(candidate),
            _ => None,
        }
    }
}

/// **本模块的核心**：在候选里选一个满足要求的。
///
/// 顺序（每一步都写清理由，因为顺序错了用户看到的是"随机失败"）：
/// 1. **先筛满足最低版本的**
/// 2. **再筛"是真正的 JDK/JRE 目录"** —— 见 [`is_launcher_stub`]。
///    这一条是实测加的：本机第一次跑出来把 Java 8 选成了
///    `Common Files\Oracle\Java\java8path\java.exe`，那是 Oracle 的**启动器存根**，
///    它能用，但它依赖 Oracle 自己的注册表设置，**不是我们的运行时**。
/// 3. **再筛 64 位** —— 32 位 Java 装不了现代版本的内存需求，很多模组也要求 64 位；
///    **32 位只在"没有 64 位可选"时才考虑**，并把取舍写进 `reason`
/// 4. 同档里**选主版本最低的那个** —— 能用 Java 17 跑就别上 21，
///    因为**更高版本有时会让老模组出问题**，而用户看不出是这个原因
/// 5. 仍并列时按版本串、再按路径排序 —— 让结果**确定**（同机同输入同输出）
pub fn choose_java(candidates: &[JavaCandidate], requirement: JavaRequirement) -> JavaChoice {
    if candidates.is_empty() {
        return JavaChoice::NoJavaAtAll { requirement };
    }

    let min = requirement.min_major();
    let satisfying: Vec<&JavaCandidate> = candidates.iter().filter(|c| c.major >= min).collect();

    if satisfying.is_empty() {
        return JavaChoice::NoneSatisfies {
            requirement,
            available: candidates.to_vec(),
        };
    }

    // 优先"非存根"的候选；只有当全部满足项都是存根时才退回它们。
    let non_stub: Vec<&JavaCandidate> = satisfying
        .iter()
        .copied()
        .filter(|c| !is_launcher_stub(&c.path))
        .collect();
    let pool: Vec<&JavaCandidate> = if non_stub.is_empty() {
        satisfying.clone()
    } else {
        non_stub
    };
    // `used_stub_fallback` is derived from the pool, not from `non_stub` after
    // the move: `non_stub` has already been consumed by the `if` above.
    let used_stub_fallback = pool.iter().all(|c| is_launcher_stub(&c.path));

    let best = pool
        .iter()
        .copied()
        .min_by(|a, b| {
            // 64 位优先（true 排前）
            let ab = a.bits >= 64;
            let bb = b.bits >= 64;
            bb.cmp(&ab)
                // 主版本低者优先
                .then(a.major.cmp(&b.major))
                // 再按版本串与路径，保证确定性
                .then(a.version.cmp(&b.version))
                .then(a.path.cmp(&b.path))
        })
        .expect("pool 非空时 min_by 必有结果");

    let is64 = best.bits >= 64;
    let reason = if used_stub_fallback {
        format!(
            "满足 {}，但本机只有一个「启动器存根」形式的入口（{}）—— 可用但不理想，建议安装独立 JDK",
            requirement.human(),
            best.path
        )
    } else if is64 {
        format!(
            "满足 {}、非存根、64 位；同档候选中主版本最低的一个",
            requirement.human()
        )
    } else {
        format!(
            "满足 {}，但本机没有 64 位候选，只能退回 32 位 —— 内存与模组兼容性可能受限",
            requirement.human()
        )
    };

    JavaChoice::Selected {
        candidate: best.clone(),
        reason,
    }
}

/// 这个路径是不是"启动器存根"而不是真正的 JDK/JRE 目录？
///
/// **为什么必须单独判这一类**：Oracle 会装一个 `java.exe` 到
/// `Common Files\Oracle\Java\javapath\`（以及 `java8path` 等）。
/// 那个 exe **不是运行时**，而是一个**按注册表决定转发给哪个 JRE 的启动器**。
///
/// 选它的后果：**用户在我们不知道的地方改了注册表，我们选的 Java 就变了**。
/// 那正是 S6 要消灭的那类"用户看不懂的失败"——所以这里主动避开。
pub fn is_launcher_stub(path: &str) -> bool {
    let p = path.to_lowercase().replace('/', "\\");
    p.contains("\\common files\\oracle\\java\\") && p.contains("path\\java")
}

/// **缺 Java 时的可操作提示**（S6 第 4 条）。
///
/// 方案要求的形式是：**"需要 Java 21，未找到；已找到 Java 8 与 17"**。
/// 所以本函数必须做三件事：**说需要什么、说本机有什么、说下一步做什么**。
///
/// **绝不允许只说"未找到 Java"** —— 那不是提示，是把问题丢回给用户。
pub fn missing_java_message(choice: &JavaChoice) -> Option<String> {
    match choice {
        JavaChoice::Selected { .. } => None,
        JavaChoice::NoJavaAtAll { requirement } => Some(format!(
            "需要 {}，但本机没有检测到任何 Java。\n\
             请安装 {}（推荐 Eclipse Temurin 或 Oracle JDK），\
             或在设置里手动指定 Java 路径。",
            requirement.human(),
            requirement.human()
        )),
        JavaChoice::NoneSatisfies {
            requirement,
            available,
        } => {
            let mut found: Vec<String> = available
                .iter()
                .map(|c| format!("Java {}", c.major))
                .collect();
            found.sort();
            found.dedup();
            Some(format!(
                "需要 {}，本机未找到；已找到 {}。\n\
                 请安装 {}，或在设置里手动指定 Java 路径。",
                requirement.human(),
                found.join(" 与 "),
                requirement.human()
            ))
        }
    }
}
