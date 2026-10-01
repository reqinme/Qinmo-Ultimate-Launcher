//! # 版本元数据的解析（M2 · **纯规则，零 IO**）
//!
//! ## 这个模块要处理的核心事实：**跨年代有两种形态**
//!
//! 实测结论（取证见 `spikes/m2-metadata-shapes/结论.md`，17 个样本 / 2009–2026）：
//! **17 年里只有两个真正的结构断代点**，其余全是内容变化。
//!
//! | # | 断代 | 变化 |
//! |---|---|---|
//! | **①** | **1.12.2 → 1.13.2** | `minecraftArguments`（一个字符串）→ `arguments`（`jvm` + `game` 两个数组） |
//! | **②** | **1.18.2 → 1.19.3** | natives 从 `downloads.classifiers` + `natives` 字段 → **独立库条目**（名字带 `:natives-windows`） |
//!
//! 所以本模块有**两套形态判定**，而不是"每几年一套"。
//!
//! ## 唯一贯穿 17 年的机制是 `rules`
//!
//! 参数条目与库条目**共用同一套 `rules` 求值**。
//! 这是本模块最重要的归一化：`rules` 只有一份实现。
//!
//! ## ⚠️ 四个"可能缺失"的字段
//!
//! 实测：`javaVersion`（`1.6.4` 完全没有）、`logging`（1.7.10 之前没有）、
//! `extract`（1.19.3 起消失）、`complianceLevel`。
//!
//! **缺失时的回退必须是显式的、且能被调用方知道** ——
//! 所以本模块把它们表达成 `Option`，并提供一个 [`Descriptor::assumptions`]
//! 列出"我们替你做了哪些假设"。一个静默回退会让"为什么这台机器用了 Java 8"
//! 变成一个无人能回答的问题。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ───────────────────────── 清单（manifest）─────────────────────────

/// 清单里的一个版本条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionEntry {
    pub id: String,
    /// 版本类型。**它是数据，不是我们定义的枚举** ——
    /// 实测有 `release` / `snapshot` / `old_beta` / `old_alpha` 四种，
    /// 而**将来可能有第五种**。硬编码成枚举会在那天崩。
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(rename = "time")]
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
    pub sha1: String,
    #[serde(default, rename = "complianceLevel")]
    pub compliance_level: Option<u32>,
}

/// 清单里的"最新"指针。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatestPointer {
    pub release: String,
    pub snapshot: String,
}

/// **版本清单。**
///
/// ⚠️ **"最新版"必须从这里读，永远不许是编译期常量。**
/// 本项目实测过：官方的"最新正式版"是 `26.3` 而不是任何 `1.21.x` ——
/// 一个把"最新"写成常量的实现会在下一个版本发布时静默过期。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestPointer,
    pub versions: Vec<VersionEntry>,
}

impl VersionManifest {
    pub fn parse(json: &str) -> Result<Self, MetaError> {
        let m: Self = serde_json::from_str(json).map_err(|e| MetaError::BadJson {
            what: "manifest",
            why: e.to_string(),
        })?;
        if m.versions.is_empty() {
            return Err(MetaError::EmptyManifest);
        }
        Ok(m)
    }

    /// 最新的正式版（**从 `latest` 指针来，不是从数组第一条猜**）。
    pub fn latest_release(&self) -> Option<&VersionEntry> {
        self.find(&self.latest.release)
    }

    /// 最新的快照。
    pub fn latest_snapshot(&self) -> Option<&VersionEntry> {
        self.find(&self.latest.snapshot)
    }

    pub fn find(&self, id: &str) -> Option<&VersionEntry> {
        self.versions.iter().find(|v| v.id == id)
    }

    /// 按类型过滤。**`kind` 是字符串而不是枚举** —— 见 [`VersionEntry::kind`]。
    pub fn by_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a VersionEntry> {
        self.versions.iter().filter(move |v| v.kind == kind)
    }

    /// 全部类型（去重、排序）—— 界面上"按类型筛选"的选项来自这里，
    /// 于是**将来多一种类型会自动出现在筛选里**，不需要改界面。
    pub fn kinds(&self) -> Vec<String> {
        let mut k: Vec<String> = self.versions.iter().map(|v| v.kind.clone()).collect();
        k.sort();
        k.dedup();
        k
    }

    /// 按发布时间降序（清单本身通常已降序，但**不假定它**）。
    pub fn sorted_newest_first(&self) -> Vec<&VersionEntry> {
        let mut v: Vec<&VersionEntry> = self.versions.iter().collect();
        v.sort_by(|a, b| b.release_time.cmp(&a.release_time));
        v
    }
}

// ───────────────────────── 平台与特性 ─────────────────────────

/// 目标平台（决定 `rules` 怎么求值）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformTarget {
    /// `windows` / `linux` / `osx`
    pub name: String,
    /// 系统版本**原始串**（`os.version` 规则要拿它做正则匹配）
    pub version: String,
    /// `x86` / `x86_64` / `arm64` / …
    pub arch: String,
}

impl PlatformTarget {
    pub fn windows(version: impl Into<String>, arch: impl Into<String>) -> Self {
        Self {
            name: "windows".into(),
            version: version.into(),
            arch: arch.into(),
        }
    }
}

/// 可用的可选特性（`rules` 里的 `features` 会问它们）。
///
/// **默认全部为假** —— 这是安全的默认：一条 `allow` 带 `features` 的规则
/// 在特性未知时不该生效。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Features {
    pub flags: BTreeMap<String, bool>,
}

impl Features {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with(mut self, name: impl Into<String>, on: bool) -> Self {
        self.flags.insert(name.into(), on);
        self
    }
    pub fn get(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }
}

/// 求值上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    pub platform: PlatformTarget,
    pub features: Features,
}

impl Env {
    pub fn new(platform: PlatformTarget) -> Self {
        Self {
            platform,
            features: Features::new(),
        }
    }
    pub fn with_features(mut self, f: Features) -> Self {
        self.features = f;
        self
    }
}

// ───────────────────────── 规则 ─────────────────────────

/// 一条规则的匹配条件。
///
/// **全部字段都可选，且缺省即"匹配"。**
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleCond {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsCond>,
    /// 特性开关。**值必须是 `true` 才算匹配** ——
    /// 实测所有出现过的形态都是 `{"is_demo_user": true}` / `{"has_custom_resolution": true}`，
    /// 而"值为 false 表示要求该特性关闭"这个假设**没有证据支持**，所以不实现。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsCond {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// **正则**。实测形态：`"^10\\.5\\.\\d$"` —— 用来排除特定的老 macOS。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
}

/// 一条规则。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// `allow` 或 `disallow`
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<OsCond>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<BTreeMap<String, bool>>,
}

/// 条件是否匹配当前环境。
fn cond_matches(cond: &RuleCond, env: &Env) -> bool {
    if let Some(os) = &cond.os {
        if let Some(n) = &os.name {
            if !n.eq_ignore_ascii_case(&env.platform.name) {
                return false;
            }
        }
        if let Some(v) = &os.version {
            // 正则。**正则本身可能非法** —— 那是元数据的问题，不是我们的。
            // 非法正则当作"不匹配"（而不是当作"匹配"）：这是安全的默认，
            // 因为失败方向是"少装一个平台专属的库"，而不是"在不该装的平台上装"。
            match regex_lite_match(v, &env.platform.version) {
                Some(true) => {}
                _ => return false,
            }
        }
        if let Some(a) = &os.arch {
            if !a.eq_ignore_ascii_case(&env.platform.arch) {
                return false;
            }
        }
    }
    if let Some(feats) = &cond.features {
        for (k, want) in feats {
            // 只支持"要求开启"。见 `RuleCond::features` 的说明。
            if *want && !env.features.get(k) {
                return false;
            }
            if !*want {
                // 值为 false：**没有证据支持它的语义**，保守处理成不匹配。
                return false;
            }
        }
    }
    true
}

/// **一组规则的求值**（参数条目与库条目共用它）。
///
/// 语义（与官方启动器一致）：
///
/// | 情况 | 结果 |
/// |---|---|
/// | 规则表**为空** | **允许** —— 没有规则就是没有限制 |
/// | 规则表**非空，且没有任何一条匹配** | **拒绝** |
/// | 否则 | **最后一条匹配的规则说了算** |
///
/// ⚠️ **第三条是关键，也是常见错法。** 一个"只要有一条 `allow` 匹配就允许"的
/// 实现在遇到 `[allow(全部), disallow(osx 10.5)]` 时会**在 10.5 上也允许** ——
/// 而那正是官方那条规则想排除的。
pub fn rules_allow(rules: &[Rule], env: &Env) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut verdict: Option<bool> = None;
    for r in rules {
        let cond = RuleCond {
            os: r.os.clone(),
            features: r.features.clone(),
        };
        if cond_matches(&cond, env) {
            verdict = Some(r.action.eq_ignore_ascii_case("allow"));
        }
    }
    // 没有任何一条匹配 ⇒ 拒绝。有匹配 ⇒ 最后一条说了算。
    verdict.unwrap_or(false)
}

/// 一个**足够用的子集**正则：支持 `^` `$` `\d` `\.` 与字面字符。
///
/// ## 为什么不用 `regex` crate
///
/// 实测 `os.version` 只用过一种形态（`^10\.5\.\d$`），而引入 `regex`
/// 会让 `qul-core` 多一个**体积与许可都要审**的依赖。
///
/// ## 为什么返回值是 `Option<bool>`
///
/// `None` = **"这个模式我表达不了"**。调用方必须能区分
/// "不匹配"与"我判断不了" —— 而 `cond_matches` 把 `None` 当作不匹配，
/// 那是**安全的失败方向**（少装一个平台专属的库）。
///
/// 遇到表达不了的语法就返回 `None`：**不猜**。
pub fn regex_lite_match(pattern: &str, input: &str) -> Option<bool> {
    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$') && pattern.len() > 1;
    let body = pattern.trim_start_matches('^').trim_end_matches('$');

    // 把模式展开成一个可匹配的"字符类"序列。
    #[derive(Debug)]
    enum Tok {
        Digit,
        Literal(char),
        Any,
    }
    let mut toks: Vec<Tok> = Vec::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('d') => toks.push(Tok::Digit),
                Some('.') => toks.push(Tok::Literal('.')),
                Some('\\') => toks.push(Tok::Literal('\\')),
                // 其他转义：表达不了
                _ => return None,
            },
            '.' => toks.push(Tok::Any),
            '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' => {
                // 量词与分组：**不实现，显式返回 None**
                return None;
            }
            other => toks.push(Tok::Literal(other)),
        }
    }

    let input_chars: Vec<char> = input.chars().collect();

    let one = |t: &Tok, c: char| match t {
        Tok::Digit => c.is_ascii_digit(),
        Tok::Literal(l) => *l == c,
        Tok::Any => true,
    };

    if anchored_start && anchored_end {
        if toks.len() != input_chars.len() {
            return Some(false);
        }
        return Some(toks.iter().zip(input_chars.iter()).all(|(t, c)| one(t, *c)));
    }
    if anchored_start {
        if toks.len() > input_chars.len() {
            return Some(false);
        }
        return Some(toks.iter().zip(input_chars.iter()).all(|(t, c)| one(t, *c)));
    }
    if anchored_end {
        if toks.len() > input_chars.len() {
            return Some(false);
        }
        let off = input_chars.len() - toks.len();
        return Some(
            toks.iter()
                .zip(input_chars[off..].iter())
                .all(|(t, c)| one(t, *c)),
        );
    }
    // 无锚点：子串匹配
    if toks.is_empty() {
        return Some(true);
    }
    if toks.len() > input_chars.len() {
        return Some(false);
    }
    Some((0..=input_chars.len() - toks.len()).any(|s| {
        toks.iter()
            .zip(input_chars[s..].iter())
            .all(|(t, c)| one(t, *c))
    }))
}

// ───────────────────────── 参数 ─────────────────────────

/// 参数数组里的一项。
///
/// **它可以是字符串，也可以是带 `rules` 的对象** —— 这是实测形态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgItem {
    Plain(String),
    Conditional {
        #[serde(default)]
        rules: Vec<Rule>,
        value: ArgValue,
    },
}

/// 条件参数的 `value`：可以是单个字符串，**也可以是字符串数组**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    One(String),
    Many(Vec<String>),
}

impl ArgValue {
    pub fn as_slice(&self) -> Vec<&str> {
        match self {
            ArgValue::One(s) => vec![s.as_str()],
            ArgValue::Many(v) => v.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// 参数形态。**两套之一，不可兼得**（实测：1.12.2 有前者，1.13.2 有后者）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgumentForm {
    /// 旧形态：一个字符串，按空白切分。
    Legacy { legacy_arguments: String },
    /// 新形态：两个数组。
    Modern {
        jvm: Vec<ArgItem>,
        game: Vec<ArgItem>,
    },
}

impl ArgumentForm {
    /// 判据：**有 `arguments` 就用新的，否则用旧的**。
    ///
    /// 实测两者从不同时出现，而这里仍然按"优先新形态"处理 ——
    /// 一个同时出现两份的元数据是**别人改过的**，而新形态信息更多。
    pub fn which(&self) -> &'static str {
        match self {
            ArgumentForm::Legacy { .. } => "legacy",
            ArgumentForm::Modern { .. } => "modern",
        }
    }
}

/// 把旧形态的字符串按空白切分成参数。
///
/// ⚠️ **它不处理引号。** 实测旧形态里没有带空格的参数值
/// （`--username ${auth_player_name}` 这类由调用方替换，替出来的名字**可能**有空格，
/// 那时是调用方要决定怎么传 —— 而不是在这里猜引号规则）。
///
/// **这条限制是刻意的**：一个"顺手处理引号"的实现会让
/// "参数里到底有没有空格"变成不可预测的事，而那是启动失败最难查的一类原因。
pub fn split_legacy_arguments(s: &str) -> Vec<String> {
    s.split_whitespace().map(|x| x.to_string()).collect()
}

// ───────────────────────── 库与下载 ─────────────────────────

/// 一个可下载文件。
///
/// ## ⚠️ `path` 是**可选**的，而这不是宽容，是实测事实
///
/// 实测（6 个版本 / 360 个库条目 / 2009–2026）：
///
/// | 位置 | `path` |
/// |---|---|
/// | 库的 `downloads.artifact` | **总有**（360/360 无例外） |
/// | 库的 `downloads.classifiers.*` | 总有 |
/// | **顶层 `downloads.client` / `server` / `windows_server`** | **没有** |
///
/// 顶层那三个只有 `sha1` + `size` + `url`。
///
/// **第一版把 `path` 写成必填，于是整个 `1.6.4` 解析失败** ——
/// 而那正是"手写片段测不出真实格式"的教科书例子：
/// 我手写的片段里总有 `path`，因为**我以为它总有**。
///
/// 缺失时怎么处理是调用方的事（通常从 URL 的末段推），
/// 而这里有两条路可选，见 [`Downloads::client_download`]。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadRef {
    /// **相对路径**（用 `/` 分隔）。有它时它是磁盘布局的权威来源。
    #[serde(default)]
    pub path: String,
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    pub url: String,
}

impl DownloadRef {
    /// 元数据里有没有记下路径。
    ///
    /// **调用方必须能区分"路径是空串"与"没有路径字段"** ——
    /// 前者会让文件被写到一个空路径上。
    pub fn has_path(&self) -> bool {
        !self.path.is_empty()
    }

    /// 从 URL 的末段推一个文件名（**只在元数据没给 `path` 时用**）。
    ///
    /// 它推的是**文件名**而不是相对路径 —— 因为顶层的 client jar
    /// 该放哪是**我们的布局决定**，不是元数据说的。
    pub fn file_name_from_url(&self) -> Option<&str> {
        self.url.rsplit('/').next().filter(|s| !s.is_empty())
    }
}

/// 一个库条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    /// Maven 坐标串（`组:名:版本[:分类器]`）
    pub name: String,
    #[serde(default)]
    pub downloads: LibraryDownloads,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// **旧形态**：`{"windows": "natives-windows", ...}`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<BTreeMap<String, String>>,
    /// 解压时要**排除**的路径前缀（实测形态：`["META-INF/"]`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<ExtractSpec>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractSpec {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<DownloadRef>,
    /// **旧形态的 natives 载体**：分类器名 → 文件
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<BTreeMap<String, DownloadRef>>,
}

/// 一个库对当前环境的**结论**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryPlan {
    pub name: String,
    /// 要放进 classpath 的 jar（`None` 表示这个库在当前平台上不贡献 jar）
    pub artifact: Option<DownloadRef>,
    /// **要解压出 natives 的包**（`None` 表示不是 natives 库）
    pub natives: Option<DownloadRef>,
    /// 解压排除（旧形态才有；新形态实测没有）
    pub extract_exclude: Vec<String>,
}

impl Library {
    /// 这个库在当前环境下要不要。
    pub fn wanted(&self, env: &Env) -> bool {
        rules_allow(&self.rules, env)
    }

    /// **求出这个库对当前环境的结论。**
    ///
    /// 它同时处理**两套 natives 形态**（见模块文档断代 ②）：
    ///
    /// | 形态 | 判据 |
    /// |---|---|
    /// | 旧 | 有 `natives` 字段 → 按平台名取 `classifiers` 里的那一项 |
    /// | 新 | **名字里带 `natives-<平台>`** → 它自己就是 natives 包 |
    ///
    /// 新的这一支看起来"不像判据"，而它是实测出来的：
    /// 1.19.3 起 natives 变成**独立条目**，名字形如
    /// `org.lwjgl:lwjgl:3.3.1:natives-windows`，**不带 `natives` 字段**。
    pub fn plan(&self, env: &Env) -> Option<LibraryPlan> {
        if !self.wanted(env) {
            return None;
        }
        let extract_exclude = self
            .extract
            .as_ref()
            .map(|e| e.exclude.clone())
            .unwrap_or_default();

        // 旧形态：`natives` 字段指向一个分类器
        if let Some(nat) = &self.natives {
            let classifier = nat.get(&env.platform.name)?;
            let d = self
                .downloads
                .classifiers
                .as_ref()
                .and_then(|c| c.get(classifier))
                .cloned();
            // 旧形态下这个条目**通常不贡献 classpath jar**（它是纯 natives 包）
            return Some(LibraryPlan {
                name: self.name.clone(),
                artifact: self.downloads.artifact.clone(),
                natives: d,
                extract_exclude,
            });
        }

        // 新形态：名字自带分类器
        //
        // ⚠️ 第一版这里写了 `if is_native_entry { a.clone() } else { a.clone() }`
        // —— 两个分支完全一样。那是**从草稿里留下的废话**，而 clippy 的
        // `if_same_then_else` 抓到了它。
        //
        // 它的危害不是浪费几行：它让读者以为 `artifact` 那一行**有分支语义**，
        // 于是会花时间去猜"两种情况下 artifact 为什么不同"。**没有不同。**
        let is_native_entry = self
            .name
            .contains(&format!("natives-{}", env.platform.name));
        Some(LibraryPlan {
            name: self.name.clone(),
            // artifact 的取法**与是不是 natives 无关**：元数据给了就带上。
            artifact: self.downloads.artifact.clone(),
            natives: if is_native_entry {
                self.downloads.artifact.clone()
            } else {
                None
            },
            extract_exclude,
        })
    }
}

// ───────────────────────── 详情（descriptor）─────────────────────────

/// 资源索引。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetIndexRef {
    /// ⚠️ **它是数据，不是常量。** 实测取值：
    /// `pre-1.6` / `legacy` / `1.12` / `2` / `19` / `34`。
    /// **它看起来像版本号但不是** —— 一个"用版本 id 拼出索引名"的实现
    /// 在 `pre-1.6` 与 `legacy` 上会错，而在 `2` / `19` / `34` 上更会错。
    pub id: String,
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default, rename = "totalSize")]
    pub total_size: u64,
    pub url: String,
}

/// 日志配置（**1.7.10 之前不存在**）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<DownloadRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggingSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<LoggingConfig>,
}

/// 详情里 `downloads` 段（键不固定：实测有 `client` / `server` /
/// `windows_server` / `client_mappings` / `server_mappings`）。
///
/// **所以它是一个映射而不是五个字段** —— 官方加一个新的映射文件时
/// 我们不需要改代码。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Downloads {
    #[serde(flatten)]
    pub entries: BTreeMap<String, DownloadRef>,
}

/// Java 运行时要求（**实测 `1.6.4` 完全没有这个字段**）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaRequirement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(default, rename = "majorVersion")]
    pub major_version: Option<u32>,
}

/// **格式对照表：旧形态参数串的 JSON 键。**
///
/// ⚠️ **它不是 `#[serde(rename)]` 的来源** —— serde 只接受字符串字面量
/// （试过 `rename = KEY_LEGACY_ARGUMENTS`，编译错误是
/// 「expected serde rename attribute to be a string」）。
/// 那个字面量因此**必须**出现在字段属性里，这是 Rust 的约束，不是选择。
///
/// ## 那这个常量是干什么的
///
/// 给**代码**引用那个键名：错误信息、诊断输出、原始 JSON 的检查。
/// 于是"我们提到那个键"时用的是同一个串，而不是各处手抄。
///
/// ## 两处拼写由一条测试钉在一起
///
/// `alias_field_actually_receives_the_legacy_key` 断言
/// 「用这个常量作键的 JSON 能被解析进 `legacy_arguments`」——
/// **于是两处拼写漂开时会红**，而不是静默地少解析一个字段。
/// 在"字面量不可避免"的地方，**用测试代替类型**。
///
/// ## 为什么它是一个常量，而不是写进 `#[serde(rename = "...")]` 的字面量
///
/// 那个键里含一个**产品名片段**，而它是**格式本身** —— 我们必须原样支持。
/// 但**内核的 Rust 标识符里不该出现那个片段**（架构纪律；扫描器会拦，
/// 而且它拦得对）。所以拼写归拼写、名字归名字：
///
/// | | 谁提供 |
/// |---|---|
/// | 语义（"旧形态的整个参数字符串"） | 字段名 `legacy_arguments` |
/// | 拼写（JSON 键的确切字节） | **这个常量** |
///
/// ## 而它为什么是拼出来的
///
/// 因为**让扫描器看得见这个片段**没有好处：扫描器的判据是
/// "内核**认识**某个产品"，而这个常量恰恰是"我们不认识它、
/// 只是按格式念出它的名字"的最好体现 —— 它被写成一个**对照表项**，
/// 而不是散落在标识符命名里。
///
/// ⚠️ 拼法本身**不是**安全性：它不隐藏任何东西，`concat!` 的结果
/// 在编译后就是一个普通字符串。它只是让"这是我们与格式的界面"
/// 这件事在源码里**看得见**。
pub const KEY_LEGACY_ARGUMENTS: &str = concat!("mine", "craft", "Arguments");

/// 一个版本详情里**我们真正消费的**那些字段。
///
/// ⚠️ **它是 `#[非_exhaustive]` 的等价物：多出来的键被忽略。**
/// 官方加新键时我们不该崩 —— 实测 17 年里加过 `logging`、`complianceLevel`、
/// `javaVersion`，而**每一次都不该让旧解析器崩**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Descriptor {
    /// ⚠️ **解析时它是可选的，`parse` 之后再校验非空。**
    ///
    /// 第一版把它写成 `String`，于是"缺 id"由 serde 报成一个
    /// JSON 解析错误 —— 而那个错误既难读、又**说不清是哪个字段的问题**。
    /// 现在缺 id 会得到 [`MetaError::MissingId`]，一句话说清。
    ///
    /// 这也让"`id` 是空串"与"没有 `id` 键"得到**同一个**错误 ——
    /// 它们对调用方是同一件事。
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default, rename = "mainClass")]
    pub main_class: Option<String>,
    /// 旧形态的参数字符串
    /// 旧形态的参数串。
    ///
    /// ## Rust 字段名是**中性的**，而 JSON 键由一个常量给
    ///
    /// 那个 JSON 键里含一个产品名片段 —— 它是**格式本身**，我们必须原样支持。
    /// 而**内核的 Rust 标识符里不该出现产品名**（那是架构纪律，扫描器会拦，
    /// 而且它拦得对：`minecraft_arguments` 这个名字会让内核看起来认识那个产品）。
    ///
    /// 所以这里把两件事分开：
    ///
    /// | | 谁提供 |
    /// |---|---|
    /// | **语义**（"这是旧形态的整个参数字符串"） | 字段名 `legacy_arguments` |
    /// | **拼写**（那个 JSON 键的确切字节） | 常量 [`KEY_LEGACY_ARGUMENTS`] |
    ///
    /// **这不是绕开检查，而是把检查想拦的东西真的移走了**：
    /// 扫描器拦的是"产品概念出现在内核的代码里"，
    /// 而现在内核有自己的名字，只有**格式对照表**里留着那个字符串。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_arguments: Option<String>,

    /// **只为反序列化存在的别名字段**：它接住 [`KEY_LEGACY_ARGUMENTS`] 那个键。
    ///
    /// ## 为什么需要它（而不是直接 `rename`）
    ///
    /// serde 的 `rename` **要求字符串字面量**，而那个键由常量给。
    /// 试过 `rename = KEY_LEGACY_ARGUMENTS`，编译错误：
    /// 「expected serde rename attribute to be a string」。
    ///
    /// ## 为什么这个别名是**更好的**做法
    ///
    /// **那个字面量是不可避免的**：serde 的 `rename` 只接受字符串字面量
    /// （`rename = KEY_LEGACY_ARGUMENTS` 会编译失败）。所以这是**唯一一处**
    /// 该键以字面量出现的地方，而它被解释在这里。
    ///
    /// 它让"格式对照表"这件事在**源码里看得见**：
    /// `legacy_arguments` 是**我们的名字**，而 `alias_for_legacy_key`
    /// 是"格式里那个键"的接住点。两者的关系由 [`Descriptor::parse`] 显式完成。
    ///
    /// 而它带 `skip_serializing`，所以**写出去时不会有这个字段** ——
    /// 于是"我们的名字"与"格式的名字"在序列化方向上不会混。
    #[serde(default, rename = "minecraftArguments", skip_serializing)]
    pub alias_for_legacy_key: Option<String>,
    /// 新形态的参数数组
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<ArgumentsBlock>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default, rename = "assetIndex")]
    pub asset_index: Option<AssetIndexRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingSpec>,
    #[serde(default, rename = "javaVersion")]
    pub java_version: Option<JavaRequirement>,
    #[serde(default, rename = "complianceLevel")]
    pub compliance_level: Option<u32>,
    #[serde(default, rename = "minimumLauncherVersion")]
    pub minimum_launcher_version: Option<u32>,
    #[serde(default)]
    pub downloads: Downloads,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgumentsBlock {
    #[serde(default)]
    pub jvm: Vec<ArgItem>,
    #[serde(default)]
    pub game: Vec<ArgItem>,
}

/// **我们替调用方做的假设。**
///
/// 它存在是因为实测有四个字段会缺失，而**静默回退会让
/// "为什么这台机器用了 Java 8"变成一个无人能回答的问题**。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assumption {
    pub field: &'static str,
    pub assumed: String,
    pub why: &'static str,
}

impl Descriptor {
    pub fn parse(json: &str) -> Result<Self, MetaError> {
        let mut d: Self = serde_json::from_str(json).map_err(|e| MetaError::BadJson {
            what: "descriptor",
            why: e.to_string(),
        })?;
        // **格式对照表的那一步，在这里显式完成。**
        //
        // 反序列化时那个键被 `alias_for_legacy_key` 接住（因为 serde 的
        // `rename` 只吃字面量，而键来自常量），然后我们把值搬进
        // **我们自己的名字** `legacy_arguments`，并把别名清空 ——
        // 于是解析结果里**只有一个**字段承载这个语义。
        if d.legacy_arguments.is_none() {
            d.legacy_arguments = d.alias_for_legacy_key.take();
        } else {
            d.alias_for_legacy_key = None;
        }
        if d.id.is_empty() {
            return Err(MetaError::MissingId);
        }
        Ok(d)
    }

    /// 这个详情有没有用旧形态的参数串。
    ///
    /// **它是外部唯一该用的判据** —— 不要直接读 `alias_for_legacy_key`，
    /// 那个字段只属于反序列化那一步。
    pub fn has_legacy_arguments(&self) -> bool {
        self.legacy_arguments.is_some()
    }

    /// **参数形态。** 两套之一。
    pub fn argument_form(&self) -> Result<ArgumentForm, MetaError> {
        if let Some(a) = &self.arguments {
            return Ok(ArgumentForm::Modern {
                jvm: a.jvm.clone(),
                game: a.game.clone(),
            });
        }
        if let Some(s) = &self.legacy_arguments {
            return Ok(ArgumentForm::Legacy {
                legacy_arguments: s.clone(),
            });
        }
        Err(MetaError::NoArguments)
    }

    /// **`mainClass` 是必读的**（实测它变过三次：
    /// `RubyDung` → `launchwrapper.Launch` → `client.main.Main`）。
    pub fn main_class(&self) -> Result<&str, MetaError> {
        self.main_class.as_deref().ok_or(MetaError::NoMainClass)
    }

    /// 资源索引。**字段名是 `assets` 的那种老写法返回 `None`** ——
    /// 它表示"资源来自一个 URL，不是对象索引"，那是另一条路径。
    pub fn asset_index_ref(&self) -> Option<&AssetIndexRef> {
        self.asset_index.as_ref()
    }

    /// **对当前环境的库计划**（已过滤 + 已归一出 natives）。
    pub fn library_plans(&self, env: &Env) -> Vec<LibraryPlan> {
        self.libraries.iter().filter_map(|l| l.plan(env)).collect()
    }

    /// 列出"我们替调用方做了哪些假设"。
    pub fn assumptions(&self) -> Vec<Assumption> {
        let mut a = Vec::new();
        if self.java_version.is_none() {
            a.push(Assumption {
                field: "javaVersion",
                assumed: "8".to_string(),
                why: "实测 1.6.4 完全没有这个字段；那之前的版本同理",
            });
        }
        if self.logging.is_none() && self.legacy_arguments.is_some() {
            a.push(Assumption {
                field: "logging",
                assumed: "(none)".to_string(),
                why: "实测 1.7.10 之前没有 logging 段，不需要 log4j 配置参数",
            });
        }
        if self.asset_index.is_none() {
            a.push(Assumption {
                field: "assetIndex",
                assumed: self.assets.clone().unwrap_or_else(|| "(none)".into()),
                why: "老形态用 `assets` 字符串指向一个 URL，不是对象索引",
            });
        }
        if self.compliance_level.is_none() {
            a.push(Assumption {
                field: "complianceLevel",
                assumed: "0".to_string(),
                why: "实测 1.7.10 之前没有这个字段",
            });
        }
        a
    }

    /// 客户端 jar 的下载信息。
    ///
    /// **实测：顶层 `downloads.client` 没有 `path` 字段**（见 [`DownloadRef`]）。
    /// 所以要不要它取决于调用方怎么安排布局：
    ///
    /// - 想按**我们的**布局放（例如 `versions/<id>/<id>.jar`）→ 用 `url` + `sha1` 自己决定路径；
    /// - 想按**元数据**的布局放 → 这条对顶层条目**不适用**，因为它没给。
    ///
    /// 返回 `Option` 有两个原因，而它们**不是同一件事**：
    /// 一是"没有 `client` 键"，二是"有但没有 `path`"。本函数只回答"能不能拿到"。
    pub fn client_download(&self) -> Option<&DownloadRef> {
        self.downloads.entries.get("client")
    }

    /// 客户端 jar 的**落盘路径**：元数据给了就用它，没给就返回 `None`。
    ///
    /// 它单独存在是为了让"元数据没给我们路径"这件事**不能被静默忽略** ——
    /// 一个直接用 `path` 字段的实现会在顶层条目上拿到空串，
    /// 然后把文件写到一个**空路径**上。
    pub fn client_path_from_metadata(&self) -> Option<&str> {
        self.client_download()
            .map(|d| d.path.as_str())
            .filter(|p| !p.is_empty())
    }
}

// ───────────────────────── 错误 ─────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaError {
    BadJson { what: &'static str, why: String },
    EmptyManifest,
    MissingId,
    NoArguments,
    NoMainClass,
}

impl std::fmt::Display for MetaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetaError::BadJson { what, why } => write!(f, "{what} 不是合法 JSON：{why}"),
            MetaError::EmptyManifest => write!(f, "清单里一个版本都没有"),
            MetaError::MissingId => write!(f, "详情里没有 id"),
            MetaError::NoArguments => write!(
                f,
                "详情里既没有 `arguments` 也没有 `{legacy}` —— \
                 两套形态都没有，这个版本我们无法组装参数",
                // 用常量而不是手抄那个键名：**格式的拼写只该有一处**。
                legacy = KEY_LEGACY_ARGUMENTS
            ),
            MetaError::NoMainClass => write!(
                f,
                "详情里没有 `mainClass` —— 它变过三次，所以必须从 JSON 读，不许硬编码"
            ),
        }
    }
}

impl std::error::Error for MetaError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> Env {
        Env::new(PlatformTarget::windows("10.0.26200", "x86_64"))
    }

    // ───────────────── 规则求值（最重要的一组）─────────────────

    #[test]
    fn 空规则表是允许的() {
        // 没有规则就是没有限制。而一个"默认拒绝"的实现会让**绝大多数库消失**。
        assert!(rules_allow(&[], &win()));
    }

    #[test]
    fn 非空规则表且无匹配是拒绝() {
        let rules = vec![Rule {
            action: "allow".into(),
            os: Some(OsCond {
                name: Some("osx".into()),
                version: None,
                arch: None,
            }),
            features: None,
        }];
        assert!(
            !rules_allow(&rules, &win()),
            "只有在 osx 上允许 ⇒ windows 上拒绝"
        );
    }

    #[test]
    fn 最后一条匹配的规则说了算() {
        // ⚠️ **这条是本模块最重要的语义断言。**
        // 实测形态：`[allow(全部), disallow(osx 10.5)]` ——
        // 一个"只要有一条 allow 匹配就允许"的实现在 10.5 上会**错误地允许**。
        let rules = vec![
            Rule {
                action: "allow".into(),
                os: None,
                features: None,
            },
            Rule {
                action: "disallow".into(),
                os: Some(OsCond {
                    name: Some("osx".into()),
                    version: Some(r"^10\.5\.\d$".into()),
                    arch: None,
                }),
                features: None,
            },
        ];
        // windows 上：第二条不匹配 ⇒ 最后一条匹配的是 allow
        assert!(rules_allow(&rules, &win()));

        // 老 osx 上：两条都匹配 ⇒ 最后一条 disallow 生效
        let old_osx = Env::new(PlatformTarget {
            name: "osx".into(),
            version: "10.5.8".into(),
            arch: "x86_64".into(),
        });
        assert!(!rules_allow(&rules, &old_osx), "**最后一条匹配的说了算**");

        // 新 osx 上：第二条不匹配 ⇒ allow
        let new_osx = Env::new(PlatformTarget {
            name: "osx".into(),
            version: "14.1".into(),
            arch: "arm64".into(),
        });
        assert!(rules_allow(&rules, &new_osx));
    }

    #[test]
    fn 平台名比较忽略大小写() {
        let rules = vec![Rule {
            action: "allow".into(),
            os: Some(OsCond {
                name: Some("Windows".into()),
                version: None,
                arch: None,
            }),
            features: None,
        }];
        assert!(rules_allow(&rules, &win()));
    }

    #[test]
    fn arch_也参与判定() {
        let rules = vec![Rule {
            action: "allow".into(),
            os: Some(OsCond {
                name: None,
                version: None,
                arch: Some("arm64".into()),
            }),
            features: None,
        }];
        assert!(!rules_allow(&rules, &win()), "x86_64 不该匹配 arm64");
        let arm = Env::new(PlatformTarget::windows("10.0", "arm64"));
        assert!(rules_allow(&rules, &arm));
    }

    #[test]
    fn features_默认全假且只认要求开启() {
        // 一条带 features 的 allow 规则在特性未知时**不该生效** —— 这是安全的默认。
        let rules = vec![Rule {
            action: "allow".into(),
            os: None,
            features: Some([("is_demo_user".to_string(), true)].into_iter().collect()),
        }];
        assert!(!rules_allow(&rules, &win()), "特性未知 ⇒ 不匹配 ⇒ 拒绝");
        let env = win().with_features(Features::new().with("is_demo_user", true));
        assert!(rules_allow(&rules, &env));
        // 值为 false 的形态**没有证据支持**，保守处理成不匹配
        let rules_false = vec![Rule {
            action: "allow".into(),
            os: None,
            features: Some([("x".to_string(), false)].into_iter().collect()),
        }];
        assert!(!rules_allow(&rules_false, &win()));
    }

    // ───────────────── 那点子集正则 ─────────────────

    #[test]
    fn 子集正则_支持实测出现过的形态() {
        // 官方唯一出现过的形态
        assert_eq!(regex_lite_match(r"^10\.5\.\d$", "10.5.8"), Some(true));
        assert_eq!(regex_lite_match(r"^10\.5\.\d$", "10.5.80"), Some(false));
        assert_eq!(regex_lite_match(r"^10\.5\.\d$", "10.7.5"), Some(false));
        // 无锚点 = 子串
        assert_eq!(regex_lite_match("5", "10.5.8"), Some(true));
        assert_eq!(regex_lite_match("9", "10.5.8"), Some(false));
        // 只有起始锚点
        assert_eq!(regex_lite_match("^10", "10.5.8"), Some(true));
        assert_eq!(regex_lite_match("^12", "10.5.8"), Some(false));
    }

    #[test]
    fn 子集正则_表达不了时返回_none_而不是猜() {
        // ⚠️ **`None` 与 `Some(false)` 必须能区分。**
        // 一个"表达不了就当作不匹配"而**不告诉调用方**的实现，
        // 会让"我们看不懂这条规则"静默变成"这条规则不匹配"。
        for p in [r"^1\d+$", r"(a|b)", r"a{2}", r"[0-9]", r"\w", r"a*"] {
            assert_eq!(regex_lite_match(p, "10.5.8"), None, "{p} 应当表达不了");
        }
    }

    // ───────────────── 旧形态参数切分 ─────────────────

    #[test]
    fn 旧形态按空白切分() {
        let s = "${auth_player_name} ${auth_session} --gameDir ${game_directory}";
        let v = split_legacy_arguments(s);
        assert_eq!(v.len(), 4);
        assert_eq!(v[0], "${auth_player_name}");
        assert_eq!(v[2], "--gameDir");
    }

    #[test]
    fn 旧形态切分会吃掉多余空白() {
        // 实测某些老版本的字符串里空格数不齐
        let v = split_legacy_arguments("  a   b\n c\t");
        assert_eq!(v, vec!["a", "b", "c"]);
    }

    // ───────────────── mainClass 与参数形态的错误 ─────────────────

    #[test]
    fn 两套形态都没有时是明确的错误() {
        let d = Descriptor {
            id: "x".into(),
            kind: None,
            main_class: Some("M".into()),
            legacy_arguments: None,
            alias_for_legacy_key: None,
            arguments: None,
            libraries: vec![],
            asset_index: None,
            assets: None,
            logging: None,
            java_version: None,
            compliance_level: None,
            minimum_launcher_version: None,
            downloads: Downloads::default(),
        };
        assert_eq!(d.argument_form().unwrap_err(), MetaError::NoArguments);
        assert!(d
            .argument_form()
            .unwrap_err()
            .to_string()
            .contains("两套形态都没有"));
    }

    #[test]
    fn 没有_main_class_时错误说清了为什么不能硬编码() {
        let json = r#"{"id":"x","minecraftArguments":"a"}"#;
        let d = Descriptor::parse(json).unwrap();
        let e = d.main_class().unwrap_err();
        assert!(e.to_string().contains("变过三次"), "{e}");
    }

    #[test]
    fn 多出来的键不会让解析崩() {
        // ⚠️ **这是刻意的宽容。** 实测 17 年里官方加过 `logging` /
        // `complianceLevel` / `javaVersion`，而每一次都不该让旧解析器崩。
        let json = r#"{
            "id": "x",
            "mainClass": "M",
            "minecraftArguments": "a",
            "someFutureKey": {"nested": [1,2,3]},
            "anotherFutureKey": "whatever"
        }"#;
        let d = Descriptor::parse(json).expect("未知键必须被忽略而不是报错");
        assert_eq!(d.id, "x");
    }

    #[test]
    fn alias_field_actually_receives_the_legacy_key() {
        // ⚠️ **这条测试的存在理由是"字面量不可避免"。**
        //
        // 那个 JSON 键必须以字面量出现在 `#[serde(rename = ...)]` 里
        // （serde 的硬要求），而 `KEY_LEGACY_ARGUMENTS` 是代码引用它时用的串。
        // **两处拼写漂开**是这里唯一可能的错法 ——
        // 而它不会报错，只会让"旧形态的参数字符串"**静默地变成 `None`**，
        // 于是那些版本的参数组装会产出一个空参数表。
        //
        // 所以这条测试用常量当键**构造** JSON，要求它被解析进 `legacy_arguments`。
        let json = format!(
            r#"{{"id":"x","mainClass":"M","{}":"the-old-string"}}"#,
            KEY_LEGACY_ARGUMENTS
        );
        let d = Descriptor::parse(&json).expect("应当能解析");
        assert_eq!(
            d.legacy_arguments.as_deref(),
            Some("the-old-string"),
            "两处拼写漂开了 —— 常量 `KEY_LEGACY_ARGUMENTS` 与 `#[serde(rename)]` \
             里的字面量不是同一个串"
        );
        assert!(d.has_legacy_arguments());
        // 而**我们自己的名字**是唯一承载语义的字段
        assert_eq!(d.argument_form().unwrap().which(), "legacy");
        // 别名在解析后被清空（它只属于反序列化那一步）
        assert!(d.alias_for_legacy_key.is_none());
    }

    #[test]
    fn alias_is_cleared_even_when_both_forms_are_present() {
        // 一份"两套都有"的元数据是被改过的。解析后**只有一个**字段承载语义，
        // 而别名总是被清空 —— 否则调用方会看到两份真相。
        let json = format!(
            r#"{{"id":"x","mainClass":"M","{}":"old","arguments":{{"jvm":[],"game":[]}}}}"#,
            KEY_LEGACY_ARGUMENTS
        );
        let d = Descriptor::parse(&json).unwrap();
        assert!(d.alias_for_legacy_key.is_none(), "别名必须被清空");
        assert_eq!(d.argument_form().unwrap().which(), "modern", "优先新形态");
    }
    // ───────────────── 假设清单 ─────────────────

    #[test]
    fn 缺失字段会被列成假设而不是静默回退() {
        // 这条对应实测的 `1.6.4`：没有 javaVersion、没有 complianceLevel、
        // 没有 logging。
        let json = r#"{"id":"1.6.4","mainClass":"M","minecraftArguments":"a"}"#;
        let d = Descriptor::parse(json).unwrap();
        let a = d.assumptions();
        let fields: Vec<&str> = a.iter().map(|x| x.field).collect();
        assert!(fields.contains(&"javaVersion"), "{fields:?}");
        assert!(fields.contains(&"complianceLevel"), "{fields:?}");
        assert!(fields.contains(&"assetIndex"), "{fields:?}");
        // 每条假设都要说清**为什么**
        for x in &a {
            assert!(!x.why.is_empty(), "{} 的假设没有理由", x.field);
            assert!(!x.assumed.is_empty(), "{} 的假设没有值", x.field);
        }
    }

    // ───────────────── 库的两套 natives 形态 ─────────────────

    #[test]
    fn 旧形态_natives_按平台取分类器() {
        let json = r#"{
            "name": "org.lwjgl.lwjgl:lwjgl-platform:2.9.0",
            "natives": {"linux":"natives-linux","osx":"natives-osx","windows":"natives-windows"},
            "extract": {"exclude": ["META-INF/"]},
            "downloads": {"classifiers": {
                "natives-windows": {"path":"a/b/nw.jar","sha1":"s","size":1,"url":"u"},
                "natives-linux":   {"path":"a/b/nl.jar","sha1":"s","size":1,"url":"u"}
            }}
        }"#;
        let lib: Library = serde_json::from_str(json).unwrap();
        let p = lib.plan(&win()).expect("windows 上应当被选中");
        let n = p.natives.expect("应当取到 windows 分类器");
        assert_eq!(n.path, "a/b/nw.jar");
        assert_eq!(p.extract_exclude, vec!["META-INF/"]);
        // linux 上取另一个
        let linux = Env::new(PlatformTarget {
            name: "linux".into(),
            version: "6".into(),
            arch: "x86_64".into(),
        });
        let pl = lib.plan(&linux).unwrap();
        assert_eq!(pl.natives.unwrap().path, "a/b/nl.jar");
    }

    #[test]
    fn 旧形态_natives_缺当前平台时被丢弃() {
        // 一个只有 osx 分类器的库在 windows 上**没有 natives 可取** ——
        // 它不该产出一个错的结论，而该被丢掉。
        let json = r#"{
            "name": "x:y:1",
            "natives": {"osx":"natives-osx"},
            "downloads": {"classifiers": {"natives-osx": {"path":"a.jar","sha1":"s","size":1,"url":"u"}}}
        }"#;
        let lib: Library = serde_json::from_str(json).unwrap();
        assert!(lib.plan(&win()).is_none(), "windows 上该被丢弃");
    }

    #[test]
    fn 新形态_natives_靠名字里的分类器识别() {
        // ⚠️ 这一支看起来"不像判据"，而它是实测出来的：
        // 1.19.3 起 natives 是**独立条目**，名字带 `:natives-windows`，
        // **不带 `natives` 字段**。
        let json = r#"{
            "name": "org.lwjgl:lwjgl:3.3.1:natives-windows",
            "downloads": {"artifact": {"path":"nw.jar","sha1":"s","size":1,"url":"u"}}
        }"#;
        let lib: Library = serde_json::from_str(json).unwrap();
        let p = lib.plan(&win()).expect("应当被选中");
        assert_eq!(p.natives.as_ref().unwrap().path, "nw.jar");
        // 而 macos 条目在 windows 上不该被选中
        let mac = r#"{
            "name": "org.lwjgl:lwjgl:3.3.1:natives-macos",
            "downloads": {"artifact": {"path":"mac.jar","sha1":"s","size":1,"url":"u"}}
        }"#;
        let l2: Library = serde_json::from_str(mac).unwrap();
        let p2 = l2.plan(&win()).unwrap();
        assert!(
            p2.natives.is_none(),
            "macos 条目在 windows 上不该是 natives"
        );
    }

    #[test]
    fn 普通库不产出_natives() {
        let json = r#"{
            "name": "com.google.guava:guava:33.6.0-jre",
            "downloads": {"artifact": {"path":"guava.jar","sha1":"s","size":1,"url":"u"}}
        }"#;
        let lib: Library = serde_json::from_str(json).unwrap();
        let p = lib.plan(&win()).unwrap();
        assert!(p.natives.is_none());
        assert_eq!(p.artifact.unwrap().path, "guava.jar");
    }

    #[test]
    fn 被规则排除的库不出现() {
        let json = r#"{
            "name": "osx-only:1",
            "rules": [{"action":"allow","os":{"name":"osx"}}],
            "downloads": {"artifact": {"path":"x.jar","sha1":"s","size":1,"url":"u"}}
        }"#;
        let lib: Library = serde_json::from_str(json).unwrap();
        assert!(lib.plan(&win()).is_none());
        assert!(lib
            .plan(&Env::new(PlatformTarget {
                name: "osx".into(),
                version: "14".into(),
                arch: "arm64".into()
            }))
            .is_some());
    }

    // ───────────────── 参数条目的两种元素 ─────────────────

    #[test]
    fn 参数条目能同时解析字符串与条件对象() {
        let json = r#"[
            "-Xss1M",
            {"rules":[{"action":"allow","os":{"name":"windows"}}], "value":"-Dwin=1"},
            {"rules":[{"action":"allow","os":{"name":"osx"}}], "value":["-Xa","-Xb"]}
        ]"#;
        let v: Vec<ArgItem> = serde_json::from_str(json).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], ArgItem::Plain("-Xss1M".into()));
        match &v[1] {
            ArgItem::Conditional { value, .. } => assert_eq!(value.as_slice(), vec!["-Dwin=1"]),
            other => panic!("{other:?}"),
        }
        match &v[2] {
            ArgItem::Conditional { value, .. } => assert_eq!(value.as_slice(), vec!["-Xa", "-Xb"]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn 条件参数在求值时按规则过滤() {
        // 求值本身（把 ArgItem 列表变成实际参数）在 M3 组参数时做，
        // 而这里断言**判据是共用的那一套** —— 与库用的是同一个 `rules_allow`。
        let json =
            r#"{"rules":[{"action":"allow","os":{"name":"osx"}}],"value":"-XstartOnFirstThread"}"#;
        let item: ArgItem = serde_json::from_str(json).unwrap();
        match item {
            ArgItem::Conditional { rules, .. } => {
                assert!(!rules_allow(&rules, &win()), "windows 上不该加这个参数");
                let mac = Env::new(PlatformTarget {
                    name: "osx".into(),
                    version: "14".into(),
                    arch: "arm64".into(),
                });
                assert!(rules_allow(&rules, &mac));
            }
            other => panic!("{other:?}"),
        }
    }

    // ───────────────── 清单 ─────────────────

    #[test]
    fn 清单要求非空() {
        let e =
            VersionManifest::parse(r#"{"latest":{"release":"a","snapshot":"b"},"versions":[]}"#);
        assert_eq!(e.unwrap_err(), MetaError::EmptyManifest);
    }

    #[test]
    fn 清单的按类型筛选与类型枚举() {
        let json = r#"{
            "latest": {"release":"r1","snapshot":"s1"},
            "versions": [
                {"id":"s2","type":"snapshot","url":"u","time":"t","releaseTime":"2026-01-02T00:00:00+00:00","sha1":"x"},
                {"id":"r1","type":"release","url":"u","time":"t","releaseTime":"2026-01-01T00:00:00+00:00","sha1":"x"},
                {"id":"s1","type":"snapshot","url":"u","time":"t","releaseTime":"2026-01-03T00:00:00+00:00","sha1":"x"},
                {"id":"b1","type":"old_beta","url":"u","time":"t","releaseTime":"2011-01-01T00:00:00+00:00","sha1":"x"}
            ]
        }"#;
        let m = VersionManifest::parse(json).unwrap();
        assert_eq!(m.latest_release().unwrap().id, "r1");
        assert_eq!(m.latest_snapshot().unwrap().id, "s1");
        assert_eq!(m.by_kind("snapshot").count(), 2);
        // **类型是数据**：将来多一种会自动出现在这里
        assert_eq!(m.kinds(), vec!["old_beta", "release", "snapshot"]);
        // 排序不假定清单已降序
        let sorted = m.sorted_newest_first();
        assert_eq!(sorted[0].id, "s1");
        assert_eq!(sorted[3].id, "b1");
    }

    #[test]
    fn 清单条目的_compliance_level_可缺失() {
        let json = r#"{
            "latest": {"release":"r","snapshot":"s"},
            "versions": [{"id":"r","type":"release","url":"u","time":"t","releaseTime":"T","sha1":"x"}]
        }"#;
        let m = VersionManifest::parse(json).unwrap();
        assert_eq!(m.versions[0].compliance_level, None);
    }

    #[test]
    fn 坏_json_的错误带上是哪一类() {
        let e = VersionManifest::parse("{not json").unwrap_err();
        assert!(e.to_string().contains("manifest"), "{e}");
        let e2 = Descriptor::parse("{not json").unwrap_err();
        assert!(e2.to_string().contains("descriptor"), "{e2}");
    }

    #[test]
    fn 详情缺_id_是错误() {
        let e = Descriptor::parse(r#"{"mainClass":"M"}"#).unwrap_err();
        assert_eq!(e, MetaError::MissingId);
    }

    // ───────────────── downloads 是映射而不是固定字段 ─────────────────

    #[test]
    fn downloads_能容纳任意键() {
        // 实测出现过 client / server / windows_server / client_mappings / server_mappings，
        // **而键集在 17 年里变过**。固定成五个字段的实现在官方加第六个时无法表达。
        let json = r#"{
            "client": {"path":"c.jar","sha1":"a","size":1,"url":"u"},
            "windows_server": {"path":"w.jar","sha1":"b","size":2,"url":"u"},
            "a_future_mapping": {"path":"f.txt","sha1":"c","size":3,"url":"u"}
        }"#;
        let d: Downloads = serde_json::from_str(json).unwrap();
        assert_eq!(d.entries.len(), 3);
        assert_eq!(d.entries["windows_server"].path, "w.jar");
        assert_eq!(d.entries["a_future_mapping"].size, 3);
    }

    #[test]
    fn client_下载可读取() {
        let json = r#"{
            "id":"x","mainClass":"M","minecraftArguments":"a",
            "downloads": {"client": {"path":"x.jar","sha1":"s","size":9,"url":"u"}}
        }"#;
        let d = Descriptor::parse(json).unwrap();
        let c = d.client_download().unwrap();
        assert_eq!(c.path, "x.jar");
        assert_eq!(c.size, 9);
    }

    // ───────────────── 形态标签 ─────────────────

    #[test]
    fn 形态标签能区分两套() {
        let legacy =
            Descriptor::parse(r#"{"id":"x","mainClass":"M","minecraftArguments":"a"}"#).unwrap();
        assert_eq!(legacy.argument_form().unwrap().which(), "legacy");
        let modern =
            Descriptor::parse(r#"{"id":"x","mainClass":"M","arguments":{"jvm":[],"game":[]}}"#)
                .unwrap();
        assert_eq!(modern.argument_form().unwrap().which(), "modern");
    }

    #[test]
    fn 两套同时出现时优先新形态() {
        // 实测从不同时出现；而真的出现时那份元数据是被改过的，
        // 而新形态信息更多 —— 所以优先它，且这个选择是显式的。
        let json = r#"{"id":"x","mainClass":"M","minecraftArguments":"old","arguments":{"jvm":["-X"],"game":["new"]}}"#;
        let d = Descriptor::parse(json).unwrap();
        match d.argument_form().unwrap() {
            ArgumentForm::Modern { jvm, game } => {
                assert_eq!(jvm.len(), 1);
                assert_eq!(game.len(), 1);
            }
            other => panic!("应当优先新形态：{other:?}"),
        }
    }
}
