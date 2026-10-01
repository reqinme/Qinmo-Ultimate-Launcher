//! # i18n 框架（M1 交付物 · **纯规则，零 IO**）
//!
//! 方案 §5 与 v3 变更都写明了它的形态：
//!
//! > **主题 / 图标 / 语言做成数据**（JSON + 资源目录），
//! > 这是唯一值得运行时加载的东西 —— 因为它**零代码风险**。
//!
//! ## 三条纪律（都在本模块里被强制）
//!
//! | # | 纪律 | 落点 |
//! |---|---|---|
//! | 1 | **键必须是稳定 ASCII 标识**（不许是中文，也不许是那句文案本身） | [`is_valid_key`] + [`LangPack::validate`] |
//! | 2 | **缺键不许静默显示键名** —— 必须可见、且能被统计出来 | [`Translator::missing`] |
//! | 3 | **语言包之间必须可对比**（缺了哪些键、多了哪些键） | [`LangPack::diff`] |
//!
//! ## 为什么键不能是中文，也不能是那句文案本身
//!
//! 三条理由，而第三条是决定性的：
//!
//! 1. **改文案不该改代码。** 若键就是中文，那么"把'启动'改成'开始游戏'"
//!    会同时是**文案改动**与**键改动** —— 而后者会牵动所有引用它的地方。
//! 2. **同一句话在不同语言里会分裂。** 中文的"启动"在英文里可能对应 `Launch`
//!    与 `Start` 两个不同语境 —— 用中文当键会让那种分裂无处安放。
//! 3. **键会进日志、进诊断包、进测试断言。** 一个含中文的键在那些地方
//!    会遭遇编码问题（本项目已经踩过一次：`.ps1` 必须纯 ASCII）。
//!
//! ## 为什么缺键必须"可见"而不是"优雅降级"
//!
//! 一个"缺键就显示空格"的实现会让**漏翻的文案在界面上完全看不出来** ——
//! 而它通常是这样被发现的：**用户截图说你这里空了一块**。
//!
//! 本模块的选择是：
//!
//! | 场合 | 行为 |
//! |---|---|
//! | **开发 / CI** | **直接可见地标注**成 `⟦key⟧`，让人一眼看到 —— 并且 [`Translator::missing`] 能列出全部 |
//! | **发布** | 仍标注（因为"空一块"更糟），但**同一个键只记一次**，不会把日志刷爆 |
//!
//! **关键是它不静默** —— 而"可被统计"这一条让"这一版漏了 37 个键"
//! 成为一个**可以在 CI 里断言的事实**。
//!
//! ## 参数占位：与 `plan.rs` 同一套写法，但**不共用实现**
//!
//! 文案里的占位是 `{name}` —— 与启动计划的占位符**语法相同**。
//! 而它们**刻意不共用实现**：启动计划的占位符关系到**参数正确性**
//! （一个没被替换的 `{JAVA}` 会被当成路径），而文案占位只关系到**显示**。
//! 两者的失败代价差三个数量级，共用一个实现会让"为了界面文案调整语法"
//! 变成一件有风险的事。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 一个语言包（**数据，不是代码**）。
///
/// 它的 JSON 形态就是磁盘上的样子：
///
/// ```json
/// {
///   "locale": "zh-CN",
///   "name": "简体中文",
///   "entries": { "app.name": "秦墨", "action.launch": "启动" }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LangPack {
    /// 语言标识（**BCP-47 风格**，如 `zh-CN` / `en-US`）。
    ///
    /// **不许用"简体中文"这种名字当标识** —— 它是给机器看的，
    /// 而 `name` 才是给人看的。
    pub locale: String,
    /// 给人看的语言名（**用它自己的语言写** —— 这是通行做法：
    /// 用户在错误的语言下也能找到自己的那一项）
    pub name: String,
    /// 键 → 文案
    pub entries: BTreeMap<String, String>,
}

/// 键的合法性。
///
/// 规则（**刻意严格**）：
/// - **ASCII 小写字母、数字、点、下划线、连字符**
/// - 至少一个点（`app.name` 这种**分组**形式）
/// - 每段非空、不以点或连字符开头
///
/// ## 为什么强制"至少一个点"
///
/// 因为**键会随功能增长到几百个**（PCL 有 3030 个），而**平铺的键表
/// 在几百个之后就不再是可读的了**。强制分组让"这一组文案属于哪个功能"
/// 从键名上就能看出来 —— 而那正是"漏翻了哪些"这个问题的前提。
pub fn is_valid_key(key: &str) -> bool {
    if key.is_empty() || key.len() > 120 {
        return false;
    }
    if !key.is_ascii() {
        return false;
    }
    if !key.contains('.') {
        return false;
    }
    key.split('.').all(|seg| {
        !seg.is_empty()
            && !seg.starts_with('-')
            && !seg.ends_with('-')
            && seg
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    })
}

/// 语言包校验的错误。**每一条都可被穷举。**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    /// `locale` 形态不对
    BadLocale { locale: String },
    /// 键不合法（含"键是中文"这种）
    BadKey { key: String },
    /// 文案是空串
    EmptyText { key: String },
    /// 同一份包里有重复键（JSON 层面不可能，但别的来源可能）
    DuplicateKey { key: String },
}

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PackError::BadLocale { locale } => write!(
                f,
                "language tag {locale:?} is not valid (expect like zh-CN / en-US)"
            ),
            PackError::BadKey { key } => write!(
                f,
                "key {key:?} is not valid (expect ascii a-z 0-9 . _ - with at least one dot)"
            ),
            PackError::EmptyText { key } => write!(f, "key {key:?} has empty text"),
            PackError::DuplicateKey { key } => write!(f, "key {key:?} appears more than once"),
        }
    }
}

impl std::error::Error for PackError {}

/// `locale` 合不合法（**BCP-47 的宽松子集**）。
///
/// 只要"语言-地区"这一层：`zh` / `zh-CN` / `en-US`。
/// **刻意不支持** `zh-Hans-CN` 那种三重形式 —— 一期的语言包只有几个，
/// 而多支持一层就多一层"两个包算不算同一个"的判定。
pub fn is_valid_locale(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.is_empty() || parts.len() > 2 {
        return false;
    }
    for (i, p) in parts.iter().enumerate() {
        if i == 0 {
            // 语言：2–3 个小写字母
            if p.len() < 2 || p.len() > 3 || !p.chars().all(|c| c.is_ascii_lowercase()) {
                return false;
            }
        } else {
            // 地区：2 个大写字母，或 3 位数字
            let ok = (p.len() == 2 && p.chars().all(|c| c.is_ascii_uppercase()))
                || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()));
            if !ok {
                return false;
            }
        }
    }
    true
}

impl LangPack {
    pub fn new(locale: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            locale: locale.into(),
            name: name.into(),
            entries: BTreeMap::new(),
        }
    }

    pub fn set(mut self, key: impl Into<String>, text: impl Into<String>) -> Self {
        self.entries.insert(key.into(), text.into());
        self
    }

    /// 从 JSON 解析。
    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str::<LangPack>(s).map_err(|e| e.to_string())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// **校验整份包。** 返回**全部**问题（不是第一个）——
    /// 只报一个的话，修一个再跑一次又冒出一个，把一次能说完的事变成 N 轮。
    pub fn validate(&self) -> Vec<PackError> {
        let mut errs = Vec::new();
        if !is_valid_locale(&self.locale) {
            errs.push(PackError::BadLocale {
                locale: self.locale.clone(),
            });
        }
        for (k, v) in &self.entries {
            if !is_valid_key(k) {
                errs.push(PackError::BadKey { key: k.clone() });
            }
            if v.trim().is_empty() {
                errs.push(PackError::EmptyText { key: k.clone() });
            }
        }
        errs
    }

    /// **与另一份包对比。** 这是"漏翻了哪些"的答案。
    ///
    /// 它**双向**报：`missing_in_other` 与 `extra_in_other` ——
    /// 因为"英文包多了一个键"与"中文包少了那个键"是**同一件事的两种说法**，
    /// 而只报一边会让"哪边该改"变得含糊。
    pub fn diff(&self, other: &LangPack) -> PackDiff {
        let mut missing = Vec::new();
        let mut extra = Vec::new();
        for k in self.entries.keys() {
            if !other.entries.contains_key(k) {
                missing.push(k.clone());
            }
        }
        for k in other.entries.keys() {
            if !self.entries.contains_key(k) {
                extra.push(k.clone());
            }
        }
        // **按分组汇总**：一个"整组都没翻"的包比"散了 30 个键"更需要被看清
        let mut by_group: BTreeMap<String, usize> = BTreeMap::new();
        for k in &missing {
            let g = k.split('.').next().unwrap_or("(no group)").to_string();
            *by_group.entry(g).or_insert(0) += 1;
        }
        PackDiff {
            missing_in_other: missing,
            extra_in_other: extra,
            missing_by_group: by_group,
        }
    }
}

/// 两份包的差异。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackDiff {
    /// 我有、对方没有的键
    pub missing_in_other: Vec<String>,
    /// 对方有、我没有的键
    pub extra_in_other: Vec<String>,
    /// `missing_in_other` 按首段分组计数
    pub missing_by_group: BTreeMap<String, usize>,
}

impl PackDiff {
    pub fn is_clean(&self) -> bool {
        self.missing_in_other.is_empty() && self.extra_in_other.is_empty()
    }

    /// 一行摘要（CI 与诊断都用它）。
    pub fn summary(&self) -> String {
        if self.is_clean() {
            return "两份语言包的键完全一致".to_string();
        }
        let groups: Vec<String> = self
            .missing_by_group
            .iter()
            .map(|(g, n)| format!("{g}×{n}"))
            .collect();
        format!(
            "缺 {} 个键（{}）、多 {} 个键",
            self.missing_in_other.len(),
            if groups.is_empty() {
                "无分组信息".to_string()
            } else {
                groups.join("、")
            },
            self.extra_in_other.len()
        )
    }
}

/// 一次翻译的结果（**含"它是不是回退来的"**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    pub text: String,
    /// `true` = 当前语言没有这个键，文案来自回退语言
    pub from_fallback: bool,
    /// `true` = 连回退语言也没有 —— `text` 是**可见的标注**而不是空白
    pub missing: bool,
}

/// **翻译器。**
///
/// 持有"当前语言包 + 回退语言包"，并**记录缺过的键**。
#[derive(Debug, Clone)]
pub struct Translator {
    active: LangPack,
    fallback: Option<LangPack>,
    /// 缺过的键 → 被查了几次。
    ///
    /// **为什么记次数而不是只记"缺过"**：一个被查了 500 次的缺键
    /// （例如主界面的标题）与一个只被查 1 次的（某个错误分支）
    /// **优先级完全不同**。而那个差别只有次数能告诉我们。
    missing_hits: BTreeMap<String, u64>,
}

/// 缺失标注的括号。用 **U+27E6/U+27E7**（数学白方括号）——
/// 选它是因为它**在文案里几乎不可能自然出现**，于是"这是标注"一眼可辨。
pub const MISSING_OPEN: char = '⟦';
pub const MISSING_CLOSE: char = '⟧';

impl Translator {
    /// 只用一份包（**没有回退**）。
    ///
    /// 它有用：一期的语言包只有中文，而"给中文配一个英文回退"
    /// 会让所有缺键**静默地变成英文** —— 那比看见标注更坏。
    pub fn single(active: LangPack) -> Self {
        Self {
            active,
            fallback: None,
            missing_hits: BTreeMap::new(),
        }
    }

    /// 带回退（例如 `zh-CN` 缺键时用 `en-US`）。
    pub fn with_fallback(active: LangPack, fallback: LangPack) -> Self {
        Self {
            active,
            fallback: Some(fallback),
            missing_hits: BTreeMap::new(),
        }
    }

    pub fn locale(&self) -> &str {
        &self.active.locale
    }

    /// **缺过的键及次数**（按次数降序）。
    pub fn missing(&self) -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = self
            .missing_hits
            .iter()
            .map(|(k, n)| (k.clone(), *n))
            .collect();
        // 次数降序，并列时按键名 —— **让结果确定**
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// 缺过的键数。
    pub fn missing_count(&self) -> usize {
        self.missing_hits.len()
    }

    /// 一行摘要（诊断包与 CI 都用它）。
    pub fn missing_summary(&self) -> String {
        if self.missing_hits.is_empty() {
            return format!("{}：没有缺失的文案键", self.active.locale);
        }
        let top: Vec<String> = self
            .missing()
            .iter()
            .take(8)
            .map(|(k, n)| format!("{k}×{n}"))
            .collect();
        format!(
            "{}：缺 {} 个键（最高频：{}）",
            self.active.locale,
            self.missing_hits.len(),
            top.join("、")
        )
    }

    /// **翻译一个键**（不带参数）。
    pub fn t(&mut self, key: &str) -> String {
        self.t_full(key).text
    }

    /// 带参数：把文案里的 `{name}` 替换掉。
    ///
    /// **`args` 里缺的占位符会原样保留** —— 与启动计划的策略**刻意相反**：
    ///
    /// | | 缺占位符时 |
    /// |---|---|
    /// | 启动计划（`plan.rs`） | **失败**（一个没替换的 `{JAVA}` 会被当成路径） |
    /// | **文案** | **原样保留**（用户看到 `{count}` 也知道是文案没配上，而"显示 0"会让用户以为真的是 0） |
    pub fn tp(&mut self, key: &str, args: &[(&str, &str)]) -> String {
        let mut r = self.t_full(key);
        for (k, v) in args {
            r.text = r.text.replace(&format!("{{{k}}}"), v);
        }
        r.text
    }

    /// 完整结果（含"是否来自回退"与"是否缺失"）。
    pub fn t_full(&mut self, key: &str) -> Translation {
        if let Some(text) = self.active.entries.get(key) {
            return Translation {
                text: text.clone(),
                from_fallback: false,
                missing: false,
            };
        }
        if let Some(fb) = &self.fallback {
            if let Some(text) = fb.entries.get(key) {
                *self.missing_hits.entry(key.to_string()).or_insert(0) += 1;
                return Translation {
                    text: text.clone(),
                    from_fallback: true,
                    missing: false,
                };
            }
        }
        *self.missing_hits.entry(key.to_string()).or_insert(0) += 1;
        Translation {
            // **可见的标注，而不是空白。**
            // 空白会让漏翻的文案在界面上完全看不出来 ——
            // 而它通常是这样被发现的：**用户截图说你这里空了一块**。
            text: format!("{MISSING_OPEN}{key}{MISSING_CLOSE}"),
            from_fallback: false,
            missing: true,
        }
    }

    /// 有没有这个键（**不看回退**）。
    pub fn has(&self, key: &str) -> bool {
        self.active.entries.contains_key(key)
    }

    /// 清掉缺失记录（用于"换语言之后重新统计"）。
    pub fn reset_missing(&mut self) {
        self.missing_hits.clear();
    }

    /// 换一份当前语言包。
    pub fn set_active(&mut self, pack: LangPack) {
        self.active = pack;
        self.reset_missing();
    }

    /// 枚举全部键（**含回退包里的**）—— 给"文案覆盖率"用。
    pub fn all_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.active.entries.keys().cloned().collect();
        if let Some(fb) = &self.fallback {
            for k in fb.entries.keys() {
                if !self.active.entries.contains_key(k) {
                    keys.push(k.clone());
                }
            }
        }
        keys.sort();
        keys
    }
}

/// **一组语言包的校验汇总**（CI 用它）。
///
/// 它回答 CI 真正要问的那个问题：**这一版有没有漏翻的键。**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuiteReport {
    /// 基准语言（通常是 `zh-CN`，我们的第一语言）
    pub base_locale: String,
    /// 每份包的问题
    pub pack_errors: Vec<(String, Vec<PackError>)>,
    /// 每份包相对基准的差异
    pub diffs: Vec<(String, PackDiff)>,
}

impl SuiteReport {
    /// 校验一组包（**第一份是基准**）。
    pub fn check(packs: &[LangPack]) -> Self {
        let base_locale = packs
            .first()
            .map(|p| p.locale.clone())
            .unwrap_or_else(|| "(none)".to_string());
        let pack_errors: Vec<(String, Vec<PackError>)> = packs
            .iter()
            .map(|p| (p.locale.clone(), p.validate()))
            .filter(|(_, e)| !e.is_empty())
            .collect();
        let diffs: Vec<(String, PackDiff)> = match packs.first() {
            Some(base) => packs
                .iter()
                .skip(1)
                .map(|p| (p.locale.clone(), base.diff(p)))
                .collect(),
            None => Vec::new(),
        };
        Self {
            base_locale,
            pack_errors,
            diffs,
        }
    }

    /// CI 的判据：**全部包合法、且与基准的键完全一致。**
    pub fn is_clean(&self) -> bool {
        self.pack_errors.is_empty() && self.diffs.iter().all(|(_, d)| d.is_clean())
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![format!("基准语言：{}", self.base_locale)];
        for (loc, errs) in &self.pack_errors {
            for e in errs {
                lines.push(format!("{loc}：{e}"));
            }
        }
        for (loc, d) in &self.diffs {
            if !d.is_clean() {
                lines.push(format!("{loc} 相对基准：{}", d.summary()));
            }
        }
        if self.is_clean() {
            lines.push("全部语言包合法且键一致".to_string());
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zh() -> LangPack {
        LangPack::new("zh-CN", "简体中文")
            .set("app.name", "秦墨")
            .set("action.launch", "启动")
            .set("error.java.missing", "没有找到 Java {version}")
    }
    fn en() -> LangPack {
        LangPack::new("en-US", "English")
            .set("app.name", "Qinmo")
            .set("action.launch", "Launch")
            .set("error.java.missing", "Java {version} not found")
    }

    // ───────────────── 键的合法性 ─────────────────

    #[test]
    fn 合法键的形态() {
        for ok in [
            "app.name",
            "action.launch",
            "error.java.missing",
            "a.b",
            "ui.sidebar.width",
            "x1.y2",
            "a-b.c_d",
        ] {
            assert!(is_valid_key(ok), "{ok} 应当是合法的");
        }
    }

    #[test]
    fn 中文键被拒绝() {
        // ⚠️ **这是本模块最重要的一条规则。**
        // 键会进日志、进诊断包、进测试断言 —— 而一个含中文的键在那些地方
        // 会遭遇编码问题（本项目已经踩过一次：`.ps1` 必须纯 ASCII）。
        for bad in ["启动", "app.启动", "键", "操作.启动游戏"] {
            assert!(!is_valid_key(bad), "{bad} 不该被接受（键必须是 ASCII）");
        }
    }

    #[test]
    fn 没有分组的键被拒绝() {
        // 键会随功能增长到几百个，而**平铺的键表在几百个之后就不再可读**。
        for bad in ["appname", "launch", "x"] {
            assert!(!is_valid_key(bad), "{bad} 缺分组点");
        }
    }

    #[test]
    fn 各种畸形键被拒绝() {
        for bad in [
            "",       // 空
            ".a",     // 前导点
            "a.",     // 尾随点
            "a..b",   // 空段
            "A.b",    // 大写
            "a.B",    // 段里大写
            "a.-b",   // 段以连字符开头
            "a.b-",   // 段以连字符结尾
            "a b.c",  // 空格
            "a/b.c",  // 斜杠
            "a\\b.c", // 反斜杠
            "a{b}.c", // 花括号（那是占位符的语法）
        ] {
            assert!(!is_valid_key(bad), "{bad:?} 不该被接受");
        }
        // 过长
        let long = format!("{}.b", "a".repeat(200));
        assert!(!is_valid_key(&long));
    }

    // ───────────────── locale ─────────────────

    #[test]
    fn locale_的形态() {
        for ok in ["zh", "zh-CN", "en", "en-US", "ja", "pt-BR", "zh-419"] {
            assert!(is_valid_locale(ok), "{ok}");
        }
        for bad in ["", "ZH", "zh_CN", "zh-CN-extra", "c", "zh-cn", "123"] {
            assert!(!is_valid_locale(bad), "{bad} 不该被接受");
        }
    }

    // ───────────────── 校验 ─────────────────

    #[test]
    fn 校验一次报全部问题() {
        // 只报一个的话，修一个再跑一次又冒出一个 ——
        // 把一次能说完的事变成 N 轮。
        let p = LangPack::new("bad-locale", "x")
            .set("启动", "ok")
            .set("good.key", "  ")
            .set("UPPER.key", "x");
        let errs = p.validate();
        assert!(errs.len() >= 4, "应当一次报全部：{errs:?}");
        let kinds: Vec<String> = errs.iter().map(|e| e.to_string()).collect();
        assert!(kinds.iter().any(|s| s.contains("bad-locale")), "{kinds:?}");
        assert!(kinds.iter().any(|s| s.contains("启动")), "{kinds:?}");
        assert!(kinds.iter().any(|s| s.contains("good.key")), "{kinds:?}");
        assert!(kinds.iter().any(|s| s.contains("UPPER.key")), "{kinds:?}");
    }

    #[test]
    fn 合法的包没有校验问题() {
        assert!(zh().validate().is_empty());
        assert!(en().validate().is_empty());
    }

    #[test]
    fn json_往返() {
        let json = serde_json::to_string(&zh()).unwrap();
        let back = LangPack::from_json(&json).unwrap();
        assert_eq!(back, zh());
        // 形态与磁盘上的一致（这一条让"语言包是数据"成为可检查的事实）
        assert!(json.contains("\"locale\":\"zh-CN\""), "{json}");
        assert!(json.contains("app.name"), "{json}");
    }

    // ───────────────── 翻译 ─────────────────

    #[test]
    fn 正常翻译() {
        let mut t = Translator::single(zh());
        assert_eq!(t.t("app.name"), "秦墨");
        assert_eq!(t.missing_count(), 0);
    }

    #[test]
    fn 缺键时给出可见标注而不是空白() {
        // ⚠️ **这条测试钉住的是"不许静默"。**
        // 一个"缺键就显示空格"的实现会让漏翻的文案在界面上完全看不出来 ——
        // 而它通常是这样被发现的：**用户截图说你这里空了一块**。
        let mut t = Translator::single(zh());
        let got = t.t("nope.missing");
        assert_ne!(got, "", "**绝不能是空白**");
        assert!(
            got.contains("nope.missing"),
            "标注里必须能看出是哪个键：{got}"
        );
        assert!(
            got.starts_with(MISSING_OPEN) && got.ends_with(MISSING_CLOSE),
            "{got}"
        );

        let full = t.t_full("nope.missing");
        assert!(full.missing);
        assert!(!full.from_fallback);
    }

    #[test]
    fn 缺过的键会被统计且带次数() {
        let mut t = Translator::single(zh());
        for _ in 0..5 {
            let _ = t.t("hot.missing");
        }
        let _ = t.t("cold.missing");
        let m = t.missing();
        assert_eq!(m.len(), 2);
        // **次数降序** —— 一个被查 500 次的缺键（主界面标题）
        // 与只查 1 次的（错误分支）优先级完全不同。
        assert_eq!(m[0], ("hot.missing".to_string(), 5));
        assert_eq!(m[1], ("cold.missing".to_string(), 1));
        assert!(
            t.missing_summary().contains("hot.missing"),
            "{}",
            t.missing_summary()
        );
    }

    #[test]
    fn 回退语言被用上时不算缺失但会被记账() {
        let act = LangPack::new("zh-CN", "简体中文").set("app.name", "秦墨");
        let fb = en();
        let mut t = Translator::with_fallback(act, fb);
        // 有：直接用
        let a = t.t_full("app.name");
        assert!(!a.from_fallback && !a.missing);
        assert_eq!(a.text, "秦墨");
        // 没有但回退有：用回退的，**并且记账**（因为"中文缺了这个键"是要修的）
        let b = t.t_full("action.launch");
        assert!(b.from_fallback, "应当来自回退");
        assert!(!b.missing, "不是'完全缺失'");
        assert_eq!(b.text, "Launch");
        assert_eq!(t.missing_count(), 1, "中文缺了这个键要记账");
        // 两边都没有：标注
        let c = t.t_full("nope.nope");
        assert!(c.missing);
        assert!(!c.from_fallback);
    }

    #[test]
    fn 没有回退时不会静默变成别的语言() {
        // ⚠️ 一期的语言包只有中文，而"给中文配一个英文回退"
        // 会让所有缺键**静默地变成英文** —— 那比看见标注更坏。
        let mut t = Translator::single(zh());
        let r = t.t_full("action.launch2");
        assert!(r.missing, "没有回退时必须标注");
        assert!(!r.from_fallback);
    }

    // ───────────────── 参数占位 ─────────────────

    #[test]
    fn 参数会被替换() {
        let mut t = Translator::single(zh());
        assert_eq!(
            t.tp("error.java.missing", &[("version", "21")]),
            "没有找到 Java 21"
        );
    }

    #[test]
    fn 缺参数时占位符原样保留() {
        // ⚠️ 与启动计划**刻意相反**：
        // 启动计划缺占位符时**失败**（一个没替换的 `{JAVA}` 会被当成路径），
        // 而文案缺参数时**原样保留** —— 用户看到 `{version}` 就知道是文案没配上，
        // 而"显示 0"会让用户以为真的是 0。
        let mut t = Translator::single(zh());
        let got = t.tp("error.java.missing", &[]);
        assert!(got.contains("{version}"), "缺参数时该原样保留：{got}");
    }

    #[test]
    fn 多余的参数不影响结果() {
        let mut t = Translator::single(zh());
        let got = t.tp("error.java.missing", &[("version", "21"), ("unused", "x")]);
        assert_eq!(got, "没有找到 Java 21");
    }

    // ───────────────── 差异对比 ─────────────────

    #[test]
    fn 差异双向报出() {
        let a = LangPack::new("zh-CN", "中文")
            .set("x.a", "1")
            .set("x.b", "2");
        let b = LangPack::new("en-US", "English")
            .set("x.a", "1")
            .set("x.c", "3");
        let d = a.diff(&b);
        assert_eq!(d.missing_in_other, vec!["x.b".to_string()]);
        assert_eq!(d.extra_in_other, vec!["x.c".to_string()]);
        assert!(!d.is_clean());
        // **"英文包多了一个键"与"中文包少了那个键"是同一件事的两种说法** ——
        // 所以两边都要报，否则"哪边该改"会变得含糊。
        let s = d.summary();
        assert!(s.contains("缺 1"), "{s}");
        assert!(s.contains("多 1"), "{s}");
    }

    #[test]
    fn 差异按分组汇总() {
        // 一个"整组都没翻"的包比"散了 30 个键"更需要被看清。
        let a = LangPack::new("zh-CN", "中文")
            .set("crash.a", "1")
            .set("crash.b", "2")
            .set("ui.c", "3")
            .set("ok", "4");
        let b = LangPack::new("en-US", "English").set("ok", "4");
        let d = a.diff(&b);
        assert_eq!(d.missing_by_group.get("crash"), Some(&2));
        assert_eq!(d.missing_by_group.get("ui"), Some(&1));
        let s = d.summary();
        assert!(s.contains("crash×2"), "分组信息要出现在摘要里：{s}");
        assert!(s.contains("ui×1"), "{s}");
    }

    #[test]
    fn 键一致时差异干净() {
        let d = zh().diff(&zh());
        assert!(d.is_clean());
        assert!(d.summary().contains("完全一致"), "{}", d.summary());
    }

    // ───────────────── 一组包的 CI 校验 ─────────────────

    #[test]
    fn 一组包全部一致时干净() {
        // 这里用两份**键相同**的包 —— 而真实的多语言包通常键不同，
        // 那种情形由下一条测试覆盖。
        let a = zh();
        let mut b = LangPack::new("en-US", "English");
        for k in a.entries.keys() {
            b = b.set(k.clone(), "x");
        }
        let r = SuiteReport::check(&[a, b]);
        assert!(r.is_clean(), "{}", r.summary());
        assert!(r.summary().contains("键一致"), "{}", r.summary());
    }

    #[test]
    fn 一组包有漏翻时不干净且能说清哪一组() {
        let r = SuiteReport::check(&[zh(), en()]);
        // zh 与 en 的键其实一样 —— 所以差异应当是干净的
        assert!(r.diffs.iter().all(|(_, d)| d.is_clean()), "{:?}", r.diffs);
        assert!(r.is_clean(), "{}", r.summary());

        // 再拿一份缺键的
        let partial = LangPack::new("ja", "日本語").set("app.name", "秦墨");
        let r2 = SuiteReport::check(&[zh(), partial]);
        assert!(!r2.is_clean());
        let s = r2.summary();
        assert!(s.contains("ja"), "{s}");
        assert!(s.contains("缺"), "{s}");
    }

    #[test]
    fn 一组包里有非法包时不干净且列出原因() {
        let bad = LangPack::new("zh-CN", "中文").set("启动", "x");
        let r = SuiteReport::check(&[zh(), bad]);
        assert!(!r.is_clean());
        let s = r.summary();
        assert!(s.contains("启动"), "要指出是哪个键：{s}");
    }

    #[test]
    fn 空的一组包也能被处理() {
        let r = SuiteReport::check(&[]);
        assert_eq!(r.base_locale, "(none)");
        assert!(r.is_clean(), "空集合不该被算成「有问题」");
    }

    // ───────────────── 换语言 ─────────────────

    #[test]
    fn 换语言会重置缺失统计() {
        // 否则"英文包里缺的键"会与"中文包里缺的"混在一起，
        // 而那个统计的意义就没有了。
        let mut t = Translator::single(zh());
        let _ = t.t("nope.x");
        assert_eq!(t.missing_count(), 1);
        t.set_active(en());
        assert_eq!(t.missing_count(), 0, "换语言后统计该清零");
        assert_eq!(t.locale(), "en-US");
    }

    #[test]
    fn 枚举全部键含回退包的() {
        let act = LangPack::new("zh-CN", "中文").set("a.x", "1");
        let fb = LangPack::new("en-US", "English")
            .set("a.x", "1")
            .set("b.y", "2");
        let t = Translator::with_fallback(act, fb);
        assert_eq!(t.all_keys(), vec!["a.x".to_string(), "b.y".to_string()]);
        assert!(t.has("a.x"));
        assert!(!t.has("b.y"), "has 不看回退");
    }

    #[test]
    fn 空包不会崩且如实报缺失() {
        let mut t = Translator::single(LangPack::new("zh-CN", "中文"));
        assert!(t.t("a.b").contains("a.b"));
        assert!(
            t.missing_summary().contains("1 个键"),
            "{}",
            t.missing_summary()
        );
        // 没缺过时的摘要也要可读
        let t2 = Translator::single(zh());
        assert!(
            t2.missing_summary().contains("没有缺失"),
            "{}",
            t2.missing_summary()
        );
    }
}
