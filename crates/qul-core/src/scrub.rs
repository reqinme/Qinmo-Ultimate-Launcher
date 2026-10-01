//! # 脱敏管道（**纯规则，零 IO**）
//!
//! ## 为什么它从 M1 就要有
//!
//! 方案 §5.8 的原话：
//!
//! > 崩溃日志**必须走同一套脱敏管道**（令牌 / UUID / 用户名 / 路径 / IP / 服务器地址折叠）
//!
//! **"同一套"是这句话的重点。** 诊断报告、会话日志、崩溃日志、以及"一键分享外链"
//! （§12 第 8 条：*"默认脱敏后再上传，与内核脱敏管道同源"*）——**四处必须是同一个函数**。
//! 只要有第二处实现，迟早会有一处漏掉某一类，而**漏掉的那一类正好是被上传的那一份**。
//!
//! 所以本模块只暴露**一个入口**（[`Scrubber::scrub`]），所有脱敏都必须经过它。
//!
//! ## 六类敏感内容（方案 §5.8 的清单，逐条对应）
//!
//! | 类别 | 掩码 | 怎么识别 |
//! |---|---|---|
//! | **令牌** | `<redacted>` | **按已知值替换**（调用方把真令牌给它） |
//! | **UUID** | `<uuid>` | 形状识别：8-4-4-4-12 或 32 位十六进制 |
//! | **用户名** | `<user>` | **按已知值替换** + 邮箱形状 |
//! | **路径** | `%USERPROFILE%` / `%DATA%` | **按已知值替换**（用户目录、数据根） |
//! | **IP** | `<ip>` | 形状识别：点分四段 |
//! | **服务器地址** | `<server>` | **按已知值替换**（连过的服务器） |
//!
//! **三条按已知值替换，三条形状识别** —— 这个划分不是随意的：
//!
//! - **令牌、用户名、路径、服务器地址**在文本里**没有可识别的形状**
//!   （令牌是随机串，用户名是任意词，路径是任意层级）。
//!   想靠形状认出它们，只能靠宽泛的猜测，而**猜测必然误伤**。
//! - **UUID 与 IP 有明确形状**，可以识别；但**形状识别也会误伤**，
//!   而那个误伤在封存项目里**真实发生过**（见下）。
//!
//! ## ⚠️ 一条来自封存项目的真实缺陷：**版本号被当成了 IP**
//!
//! 封存项目的注释原文：
//!
//! > 先前版本号也走 `Line`，于是 `0.1.0.0` 被 IPv4 规则当成地址抹成了 `<ip>`
//! > ——诊断报告里**最该被看到的一个字段就这么没了**。
//!
//! 所以本模块有一条**专门的排除规则**：[`looks_like_version`]。
//! 而它要处理的形态比 `0.1.0.0` 多得多 —— 凡是"点分数字且段值都小"的都可能被误伤：
//!
//! | 形态 | 是不是 IP |
//! |---|---|
//! | `0.1.0.0`、`1.2.3.4`、`26.3.0.0` | ❌ **版本号**（段值小、有版本词或 `v` 前缀） |
//! | `192.168.1.1`、`8.8.8.8`、`127.0.0.1` | ✅ IP |
//!
//! **判据不能只看段值**（`1.2.3.4` 段值都 ≤255 也可能是 IP），
//! 所以本模块的做法是：**先看有没有版本语境**（前一个词是 version/v/构建等），
//! **再看段值是否全都 ≤ 一个小阈值**。两者结合，误伤率远低于任一单独使用。
//!
//! ## 一条纪律：**脱敏的条目数必须被记录**
//!
//! "脱敏了什么"如果不记录，那么"报告里少了东西"与"本来就没有"**无法区分**。
//! 所以 [`ScrubReport`] 会统计每一类替换了几次 —— 而它**只记类别与次数，不记内容**。
//! 这是刻意的：让脱敏过程本身**不可能泄露它抹掉的东西**。

/// 掩码常量。**与封存项目一致**，因为它们是**用户已经见过的形式**
/// （诊断包里出现 `%USERPROFILE%` 时，用户能看懂那是"我的用户目录"）。
pub const MASK_SECRET: &str = "<redacted>";
pub const MASK_UUID: &str = "<uuid>";
pub const MASK_IP: &str = "<ip>";
pub const MASK_USER: &str = "<user>";
pub const MASK_SERVER: &str = "<server>";
pub const MASK_USERPROFILE: &str = "%USERPROFILE%";
pub const MASK_DATA_ROOT: &str = "%DATA%";
pub const MASK_EMAIL: &str = "<email>";

/// **段值全都不超过这个数**时，点分数字更可能是版本号而不是 IP。
///
/// 取 31 的理由：**没有任何常用 IP 的四段全 ≤31** ——
/// 而版本号（`1.20.5.0`、`26.3.0.0`、`0.1.0.0`）**几乎总是**如此。
/// 这个阈值是**启发式**，所以它只作为**第二个**判据，
/// 第一个判据是"有没有版本语境"（见 [`looks_like_version`]）。
const VERSION_SEGMENT_MAX: u16 = 31;

/// 脱敏统计：**只记类别与次数，不记内容**。
///
/// 这条纪律是刻意的：让脱敏过程本身**不可能泄露它抹掉的东西**。
/// 若这里存下"抹掉的那个令牌是什么"，那么诊断包就成了新的泄露源。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScrubReport {
    pub secrets: usize,
    pub uuids: usize,
    pub users: usize,
    pub paths: usize,
    pub ips: usize,
    pub servers: usize,
    pub emails: usize,
}

impl ScrubReport {
    pub fn total(&self) -> usize {
        self.secrets + self.uuids + self.users + self.paths + self.ips + self.servers + self.emails
    }

    /// 一行摘要（**给日志头用**：让读日志的人知道哪些东西被折叠过）。
    pub fn summary(&self) -> String {
        if self.total() == 0 {
            return "本次脱敏：无替换".to_string();
        }
        let mut parts = Vec::new();
        let mut push = |n: usize, name: &str| {
            if n > 0 {
                parts.push(format!("{name}×{n}"));
            }
        };
        push(self.secrets, "机密");
        push(self.uuids, "UUID");
        push(self.users, "用户名");
        push(self.paths, "路径");
        push(self.ips, "IP");
        push(self.servers, "服务器地址");
        push(self.emails, "邮箱");
        format!("本次脱敏：{}", parts.join("、"))
    }

    fn add(&mut self, other: &ScrubReport) {
        self.secrets += other.secrets;
        self.uuids += other.uuids;
        self.users += other.users;
        self.paths += other.paths;
        self.ips += other.ips;
        self.servers += other.servers;
        self.emails += other.emails;
    }
}

/// **脱敏器**：持有"已知的值"，把它们从文本里折叠掉。
///
/// ## 为什么要显式喂"已知值"，而不是全靠形状识别
///
/// 因为**令牌、用户名、路径、服务器地址在文本里没有可识别的形状**。
/// 想靠形状认出它们只能靠宽泛的猜测，而**猜测必然误伤** ——
/// 误伤一个版本号（像封存项目那样）已经够糟，误伤一段正常的日志更糟。
///
/// 所以设计是：**调用方知道什么该被抹掉，就把那些值交给本结构。**
/// 这个分工还有一个好处：**"该抹什么"变成了一份可审查的清单** ——
/// 它就在调用方构造 `Scrubber` 的那几行里，而不是散在一堆正则里。
#[derive(Debug, Clone, Default)]
pub struct Scrubber {
    /// 单次出现的机密（令牌）。**替换发生在第一时间**，避免它们被后续规则切片。
    secrets: Vec<String>,
    /// 用户名（可能多个：本机账户名、登录名）
    users: Vec<String>,
    /// 路径（替换成各自的掩码）。**长路径必须排在短路径之前**，
    /// 否则 `C:\Users\x\AppData\Roaming\Qinmo` 会先被 `C:\Users\x` 的规则切成 `%USERPROFILE%\AppData\...`，
    /// 而那**并不是错的**，只是我们更想看到 `%DATA%`（信息量更大）。
    paths: Vec<(String, &'static str)>,
    /// 连过的服务器地址
    servers: Vec<String>,
}

impl Scrubber {
    pub fn new() -> Self {
        Self::default()
    }

    /// 加一个机密（令牌 / 密码 / 授权码）。
    ///
    /// **空值与过短的值会被忽略**：长度 < 8 的串在文本里会大量偶然出现，
    /// 把它们全局替换会把日志打成一堆 `<redacted>`，反而看不出发生了什么。
    pub fn secret(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.trim().is_empty() && s.chars().count() >= 8 {
            self.secrets.push(s);
        }
        self
    }

    /// 加一个用户名。同样忽略空值，但**不设长度下限**：
    /// 用户名可能很短（两三个字符），而它是**确切的**值，不是猜测。
    pub fn user(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.trim().is_empty() {
            self.users.push(s);
        }
        self
    }

    /// 加一个要折叠的路径。
    ///
    /// ## ⚠️ 它自动把**转义形式**也加进去（这条是被测试逼出来的）
    ///
    /// 因为**同一个路径在不同承载里的字面量不同**：
    ///
    /// | 承载 | 路径实际长什么样 |
    /// |---|---|
    /// | 纯文本日志 | `C:\Users\x\AppData` |
    /// | **JSON** | `C:\\Users\\x\\AppData` |
    ///
    /// 而"按已知值替换"是**字面量匹配** —— 不把转义形式也喂进去，
    /// 就会出现**"日志里抹掉了、JSON 里没抹掉"**：一处泄漏，另一处看起来很正常。
    ///
    /// 放在这里而不是让每个调用点自己记得加两次，理由很直接：
    /// **调用点会忘。** 而遗忘的表现是"某一种承载里静默不脱敏"——
    /// 那种缺陷不会报错，只会泄漏。
    pub fn path(mut self, s: impl Into<String>, mask: &'static str) -> Self {
        let s = s.into();
        if s.trim().is_empty() {
            return self;
        }
        let escaped = s.replace('\\', "\\\\");
        if escaped != s {
            self.paths.push((escaped, mask));
        }
        self.paths.push((s, mask));
        self
    }

    /// 只加**原样**的路径（不加转义形式）。给"确定承载不是 JSON"的场合用。
    pub fn path_raw(mut self, s: impl Into<String>, mask: &'static str) -> Self {
        let s = s.into();
        if !s.trim().is_empty() {
            self.paths.push((s, mask));
        }
        self
    }

    /// 加一个服务器地址。
    pub fn server(mut self, s: impl Into<String>) -> Self {
        let s = s.into();
        if !s.trim().is_empty() {
            self.servers.push(s);
        }
        self
    }

    /// 常用的两个路径种子（用户目录 → `%USERPROFILE%`，数据根 → `%DATA%`）。
    pub fn with_home_and_data(self, home: impl Into<String>, data: impl Into<String>) -> Self {
        self.path(home, MASK_USERPROFILE).path(data, MASK_DATA_ROOT)
    }

    /// **唯一的脱敏入口。四处（会话日志 / 诊断报告 / 崩溃日志 / 外链分享）都必须走它。**
    pub fn scrub(&self, text: &str) -> (String, ScrubReport) {
        let mut rep = ScrubReport::default();
        let mut s = text.to_string();

        // ── ① 机密最先 ──
        // 为什么最先：令牌里可能**恰好包含**一个看起来像 UUID 或 IP 的子串，
        // 若先跑形状识别，令牌会被切成两段，而后面的"按已知值替换"就再也匹配不到了。
        for v in &self.secrets {
            let n = replace_count(&mut s, v, MASK_SECRET);
            rep.secrets += n;
        }

        // ── ② 路径（长到短）──
        let mut paths = self.paths.clone();
        paths.sort_by_key(|(p, _)| std::cmp::Reverse(p.chars().count()));
        for (p, mask) in &paths {
            let n = replace_count(&mut s, p, mask);
            rep.paths += n;
        }

        // ── ③ 服务器地址（长到短，避免 `a.example.com` 先被 `example.com` 吃掉）──
        let mut servers = self.servers.clone();
        servers.sort_by_key(|p| std::cmp::Reverse(p.chars().count()));
        for v in &servers {
            let n = replace_count(&mut s, v, MASK_SERVER);
            rep.servers += n;
        }

        // ── ④ 用户名（在服务器地址之后：路径里可能含用户名，已被路径规则折叠）──
        for v in &self.users {
            let n = replace_count(&mut s, v, MASK_USER);
            rep.users += n;
        }

        // ── ⑤ 邮箱（形状）──
        let (s2, n) = scrub_emails(&s);
        s = s2;
        rep.emails += n;

        // ── ⑥ UUID（形状）──
        let (s2, n) = scrub_uuids(&s);
        s = s2;
        rep.uuids += n;

        // ── ⑦ IP（形状，**带版本号排除**）──
        let (s2, n) = scrub_ips(&s);
        s = s2;
        rep.ips += n;

        (s, rep)
    }

    /// 批量脱敏多行（诊断报告用）。统计会累加。
    pub fn scrub_lines<'a, I: IntoIterator<Item = &'a str>>(
        &self,
        lines: I,
    ) -> (Vec<String>, ScrubReport) {
        let mut out = Vec::new();
        let mut total = ScrubReport::default();
        for l in lines {
            let (s, r) = self.scrub(l);
            total.add(&r);
            out.push(s);
        }
        (out, total)
    }
}

/// 大小写**不敏感**的计数替换（Windows 路径大小写不敏感，而日志里大小写会变）。
///
/// 返回替换次数。**不用 `to_lowercase` 再比**：那会改变原文长度与内容，
/// 而我们要保留原文（只抹掉敏感段）。
fn replace_count(hay: &mut String, needle: &str, mask: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let hay_l = hay.to_lowercase();
    let needle_l = needle.to_lowercase();
    if !hay_l.contains(&needle_l) {
        return 0;
    }
    // 用字节下标做替换：ASCII 的大小写转换不改变字节布局，
    // 而 Windows 路径与令牌都是 ASCII 起头的，所以这个前提成立。
    // 若有非 ASCII 前缀导致下标错位，下面的边界检查会拦住（宁可少替换也不错替换）。
    let mut out = String::with_capacity(hay.len());
    let mut count = 0usize;
    let mut i = 0usize;
    let hb = hay.as_bytes();
    let hlb = hay_l.as_bytes();
    let nl = needle_l.as_bytes();
    while i < hb.len() {
        if i + nl.len() <= hb.len()
            && &hlb[i..i + nl.len()] == nl
            && hay.is_char_boundary(i)
            && hay.is_char_boundary(i + needle.len())
        {
            out.push_str(mask);
            count += 1;
            i += needle.len();
        } else {
            // 推进一个完整的字符
            let mut j = i + 1;
            while j < hb.len() && !hay.is_char_boundary(j) {
                j += 1;
            }
            out.push_str(&hay[i..j]);
            i = j;
        }
    }
    if count > 0 {
        *hay = out;
    }
    count
}

/// 邮箱形状：`local@domain.tld`。
///
/// **只在两侧都是"像标识符的字符"时才认**，避免把
/// `[Error: ...@...]` 这类夹在标点里的东西误当成邮箱。
fn scrub_emails(s: &str) -> (String, usize) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut count = 0usize;
    let mut i = 0usize;
    let is_local = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let is_domain = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
    while i < chars.len() {
        if chars[i].is_ascii_alphanumeric() {
            let start = i;
            while i < chars.len() && is_local(chars[i]) {
                i += 1;
            }
            // 必须恰好停在 @，且本地部分非空
            if i < chars.len() && chars[i] == '@' && i > start {
                let at = i;
                let mut j = at + 1;
                while j < chars.len() && is_domain(chars[j]) {
                    j += 1;
                }
                // 域名里必须有点，且点两侧都有东西（排除 `a@b`）
                let domain: String = chars[at + 1..j].iter().collect();
                let dot_ok = domain.split('.').filter(|p| !p.is_empty()).count() >= 2;
                if dot_ok {
                    out.push_str(MASK_EMAIL);
                    count += 1;
                    i = j;
                    continue;
                }
            }
            // 不是邮箱：原样输出已扫过的部分
            for c in &chars[start..i] {
                out.push(*c);
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, count)
}

/// UUID 形状：`8-4-4-4-12` 或紧邻的 32 位十六进制。
///
/// **紧邻形式必须两侧都不是十六进制字符**，否则会把
/// 更长的十六进制串（如 sha1 的 40 位）切出一个假的 32 位"UUID"。
fn scrub_uuids(s: &str) -> (String, usize) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut count = 0usize;
    let mut i = 0usize;
    let is_hex = |c: char| c.is_ascii_hexdigit();
    while i < chars.len() {
        // 形式一：8-4-4-4-12
        if i + 36 <= chars.len() {
            let seg = [8usize, 4, 4, 4, 12];
            let mut p = i;
            let mut ok = true;
            let mut hits = 0usize;
            for (k, len) in seg.iter().enumerate() {
                for _ in 0..*len {
                    if p >= chars.len() || !is_hex(chars[p]) {
                        ok = false;
                        break;
                    }
                    p += 1;
                    hits += 1;
                }
                if !ok {
                    break;
                }
                if k + 1 < seg.len() {
                    if p >= chars.len() || chars[p] != '-' {
                        ok = false;
                        break;
                    }
                    p += 1;
                }
            }
            if ok && hits == 32 {
                let before_ok = i == 0 || !is_hex(chars[i - 1]);
                let after_ok = p >= chars.len() || !is_hex(chars[p]);
                if before_ok && after_ok {
                    out.push_str(MASK_UUID);
                    count += 1;
                    i = p;
                    continue;
                }
            }
        }
        // 形式二：紧邻 32 位
        if i + 32 <= chars.len() && is_hex(chars[i]) {
            let run_ok = (i..i + 32).all(|k| is_hex(chars[k]));
            if run_ok {
                let before_ok = i == 0 || !is_hex(chars[i - 1]);
                let after_ok = i + 32 >= chars.len() || !is_hex(chars[i + 32]);
                if before_ok && after_ok {
                    out.push_str(MASK_UUID);
                    count += 1;
                    i += 32;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, count)
}

/// 点分四段是否"看起来像版本号"。
///
/// `before` 是紧邻之前的非空白文本（用于找版本语境）。
///
/// **两个判据缺一不可**：
/// ① **版本语境**：前面有 `version` / `ver` / `v` / `build` / `launcher` / `app` 之类
///    的提示词，或者紧邻一个 `v`；
/// ② **段值全都 ≤ [`VERSION_SEGMENT_MAX`]**。
///
/// 为什么不能只用 ②：`1.2.3.4` 段值都很小，但它**可能**真的是 IP。
/// 为什么不能只用 ①：日志里 `version=` 后面也可能跟一个真的 IP（少见但可能）。
/// **两者都满足才判为版本号**，于是误伤率远低于任一单独使用。
fn looks_like_version(before: &str, segs: [u16; 4]) -> bool {
    // ② 段值全小
    let all_small = segs.iter().all(|v| *v <= VERSION_SEGMENT_MAX);
    if !all_small {
        return false;
    }
    // ① 版本语境
    let b = before.to_lowercase();
    // 去掉尾部的键值分隔符与空白：`version=1.2.3.4` 与 `version: 1.2.3.4` 都该命中。
    //
    // ⚠️ **不能把裸 `=` 当成版本提示** —— 那是本条规则的第一版，
    // 于是 `ip=10.0.0.1` 被判成版本号、**真 IP 被漏抹**。
    // 这个缺陷是 `段值大的真_ip_一律抹掉` 那条测试抓出来的。
    // 教训与"检测与匹配必须用不同思路"同源：**过于宽松的铺垫词会吃掉真阳性。**
    //
    // ⚠️ **还要剥掉引号与括号** —— 这是本条规则的第二版才修好的，
    // 而它的表现与封存项目那个缺陷**一模一样**（版本号被抹成 `<ip>`），
    // 只是入口从纯文本换成了 JSON：
    //
    // | 承载 | 写法 | 剥掉分隔符后 | 还需剥掉 |
    // |---|---|---|---|
    // | 纯文本 | `启动器版本=0.1.0.0` | `启动器版本` | — |
    // | **JSON** | `{"version":"0.1.0.0"}` | `{"version":"` | **引号与花括号** |
    //
    // 第二行剥完是 `{"version`，**以 `"` 结尾而不是 `version`**，判据 ① 完全不命中。
    // 也就是说：**我们并没有"修好"那个缺陷，只是把它在一个承载里修好了。**
    // 这一条是 `完整生命周期_启动写_退出删_下次干净` 那条测试抓出来的
    // （崩溃标记是 JSON，于是版本号在写标记时被抹成 `<ip>`）。
    let b = b.trim_end_matches(['=', ':', ' ', '\t', '-', '"', '\'', '{', '[', '(', ',']);
    if b.ends_with('v') {
        return true;
    }
    const HINTS: &[&str] = &[
        "version",
        "ver",
        "build",
        "buildid",
        "launcher",
        "release",
        "client",
        "protocol",
        "app",
        // **中文提示必须在内**：我们的日志与诊断报告是中文的，
        // 而只认英文提示会让"启动器版本=0.1.0.0"被抹成 `<ip>`
        // —— 那正好复现了封存项目那个缺陷，只是换了个语言。
        // 这一条是测试 `版本号不会被当成_ip_抹掉` 抓出来的。
        "版本",
        "构建",
        "协议",
        "启动器",
        "客户端",
    ];
    HINTS.iter().any(|h| b.ends_with(h))
}

/// 点分四段（形状识别）。
///
/// ## 关键：**版本号必须被排除**（封存项目在这里丢过字段）
///
/// 见模块文档。判据是 [`looks_like_version`]：**版本语境 + 段值全小**，两者都要满足。
fn scrub_ips(s: &str) -> (String, usize) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut count = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            // 尝试从 i 起解析 a.b.c.d
            if let Some((end, segs, valid)) = parse_dotted_quad(&chars, i) {
                // before = 本行里 i 之前的内容（用于版本语境判断）
                let before: String = chars[..i].iter().collect();
                let before = before.rsplit(['\n', '\r']).next().unwrap_or("");
                let before_ok = i == 0 || !chars[i - 1].is_ascii_digit();
                let after_ok = end >= chars.len() || !chars[end].is_ascii_digit();
                if valid && before_ok && after_ok && !looks_like_version(before, segs) {
                    out.push_str(MASK_IP);
                    count += 1;
                    i = end;
                    continue;
                }
                // 判为版本号或非法：**原样保留**（宁可少抹，也不错抹）
                if valid && before_ok && after_ok {
                    for c in &chars[i..end] {
                        out.push(*c);
                    }
                    i = end;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, count)
}

/// 从 `start` 起尝试解析 `a.b.c.d`。
///
/// 返回 `(结束下标, 四个段值, 段值是否都 ≤255)`。
/// **段值超范围时仍然返回**（`valid=false`），因为调用方要原样保留它 ——
/// `999.999.999.999` 不是 IP，也不该被替换。
fn parse_dotted_quad(chars: &[char], start: usize) -> Option<(usize, [u16; 4], bool)> {
    let mut p = start;
    let mut segs = [0u16; 4];
    for (k, slot) in segs.iter_mut().enumerate() {
        // 一段：1..=3 位数字
        let seg_start = p;
        while p < chars.len() && chars[p].is_ascii_digit() && p - seg_start < 3 {
            p += 1;
        }
        let len = p - seg_start;
        if len == 0 {
            return None;
        }
        // 前导零：`01` 不像 IP 段（而且 `2026.10.01` 那类日期会被误判）
        if len > 1 && chars[seg_start] == '0' {
            return None;
        }
        let txt: String = chars[seg_start..p].iter().collect();
        let v: u32 = txt.parse().ok()?;
        *slot = if v > u16::MAX as u32 {
            u16::MAX
        } else {
            v as u16
        };
        if k < 3 {
            if p >= chars.len() || chars[p] != '.' {
                return None;
            }
            p += 1;
        }
    }
    let valid = segs.iter().all(|v| *v <= 255);
    Some((p, segs, valid))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn scrubber() -> Scrubber {
        Scrubber::new()
            .secret("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature")
            .user("hjc20")
            .with_home_and_data(r"C:\Users\hjc20", r"C:\Users\hjc20\AppData\Roaming\Qinmo")
            .server("play.example.com")
    }

    // ───────────────── 六类逐一验证（方案 §5.8 的清单）─────────────────

    #[test]
    fn 令牌被折叠() {
        let (s, r) = scrubber()
            .scrub("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature");
        assert!(!s.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"), "{s}");
        assert!(s.contains(MASK_SECRET), "{s}");
        assert_eq!(r.secrets, 1);
    }

    #[test]
    fn uuid_被折叠且两种形式都认() {
        let (s, r) = scrubber().scrub(
            "account=069a79f4-44e9-4726-a5be-fca90e38aaf5 compact=069a79f444e94726a5befca90e38aaf5",
        );
        assert!(!s.contains("069a79f4-44e9-4726-a5be-fca90e38aaf5"), "{s}");
        assert!(!s.contains("069a79f444e94726a5befca90e38aaf5"), "{s}");
        assert_eq!(r.uuids, 2);
        assert_eq!(s.matches(MASK_UUID).count(), 2);
    }

    #[test]
    fn 用户名被折叠() {
        let (s, r) = scrubber().scrub("玩家 hjc20 已登录");
        assert!(!s.contains("hjc20"), "{s}");
        assert!(s.contains(MASK_USER), "{s}");
        assert!(r.users >= 1);
    }

    #[test]
    fn 路径被折叠且数据根优先于用户目录() {
        // 这两个种子**重叠**：数据根在用户目录里面。
        // 若先替用户目录，数据根会变成 `%USERPROFILE%\AppData\Roaming\Qinmo` ——
        // 那不算错，但**信息量更低**（看不出那是我们的数据根）。
        let (s, r) = scrubber().scrub(r"log at C:\Users\hjc20\AppData\Roaming\Qinmo\logs\a.log");
        assert!(s.contains(MASK_DATA_ROOT), "应当优先匹配更长的数据根：{s}");
        assert!(!s.contains("hjc20"), "{s}");
        assert_eq!(r.paths, 1);
    }

    #[test]
    fn 单独的路径也认() {
        let (s, _) = scrubber().scrub(r"home is C:\Users\hjc20\Desktop");
        assert!(s.contains(MASK_USERPROFILE), "{s}");
        assert!(!s.contains(r"C:\Users\hjc20"), "{s}");
    }

    #[test]
    fn ip_被折叠() {
        let (s, r) = scrubber().scrub("connecting to 192.168.1.100:25565");
        assert!(!s.contains("192.168.1.100"), "{s}");
        assert!(s.contains(MASK_IP), "{s}");
        assert_eq!(r.ips, 1);
    }

    #[test]
    fn 服务器地址被折叠() {
        let (s, r) = scrubber().scrub("joined play.example.com:25565");
        assert!(!s.contains("play.example.com"), "{s}");
        assert!(s.contains(MASK_SERVER), "{s}");
        assert_eq!(r.servers, 1);
    }

    // ───────────────── 封存项目那个真实缺陷 ─────────────────

    #[test]
    fn 版本号不会被当成_ip_抹掉() {
        // 封存项目的注释原文：`0.1.0.0` 被 IPv4 规则当成地址抹成了 `<ip>`
        // ——「诊断报告里最该被看到的一个字段就这么没了」。
        // 这条测试就是防它复发。
        let cases = [
            "启动器版本=0.1.0.0",
            "version=1.2.3.4",
            "launcher 26.3.0.0",
            "v1.0.0.1",
            "client 1.20.5.0",
            "build 2.0.0.0",
        ];
        for c in cases {
            let (s, r) = scrubber().scrub(c);
            assert_eq!(s, c, "版本号被改动了：{c} -> {s}");
            assert_eq!(r.ips, 0, "{c} 不该被当成 IP");
        }
    }

    #[test]
    fn 真_ip_仍然会被抹掉即使段值都小() {
        // 1.2.3.4 段值全小，但它前面没有版本语境 —— 必须仍然被抹掉。
        // 否则"段值全小就放过"会漏掉大量真实内网地址。
        let (s, r) = scrubber().scrub("ping 1.2.3.4 ok");
        assert!(!s.contains("1.2.3.4"), "{s}");
        assert_eq!(r.ips, 1);
    }

    #[test]
    fn 段值超范围的点分数字不被抹也不被吃掉() {
        // 999.999.999.999 不是 IP。**既不该替换，也不该被吞掉**。
        let c = "bad 999.999.999.999 here";
        let (s, r) = scrubber().scrub(c);
        assert_eq!(s, c, "非法四段应当原样保留");
        assert_eq!(r.ips, 0);
    }

    #[test]
    fn 前导零的段不像_ip() {
        // `2026.10.01` 这类日期形式不该被当 IP（它也不是版本号，原样保留）。
        let c = "date 2026.10.01";
        let (s, _) = scrubber().scrub(c);
        assert_eq!(s, c);
    }

    #[test]
    fn 段值大的真_ip_一律抹掉() {
        for ip in ["8.8.8.8", "10.0.0.1", "203.0.113.7"] {
            let (s, r) = scrubber().scrub(&format!("ip={ip}"));
            assert!(!s.contains(ip), "{ip} 该被抹掉：{s}");
            assert_eq!(r.ips, 1, "{ip}");
        }
    }

    // ───────────────── 顺序与交叉 ─────────────────

    #[test]
    fn 令牌里的_uuid_形状不会破坏令牌替换() {
        // 令牌可能恰好含一个看起来像 UUID 的子串。
        // 若先跑形状识别，令牌会被切成两段，后面的按值替换就再也匹配不到。
        let tok = "abcdef01-2345-6789-abcd-ef0123456789";
        let sb = Scrubber::new().secret(tok);
        let (s, r) = sb.scrub(&format!("token={tok}"));
        assert_eq!(r.secrets, 1, "机密必须被完整匹配");
        assert_eq!(r.uuids, 0, "它不该被当成 UUID");
        assert!(s.contains(MASK_SECRET), "{s}");
    }

    #[test]
    fn 路径含用户名时不会二次替换出怪东西() {
        let (s, _) = scrubber().scrub(r"C:\Users\hjc20\Documents\a.txt");
        // 路径先被折叠，之后用户名规则再跑也不会把掩码改坏
        assert!(s.contains(MASK_USERPROFILE), "{s}");
        assert!(!s.contains("<user>"), "掩码里不该再嵌一个掩码：{s}");
    }

    #[test]
    fn 邮箱被折叠且不是邮箱的不动() {
        let (s, r) = scrubber().scrub("mail a.b+tag@example.co.uk and not an@addr");
        assert!(!s.contains("a.b+tag@example.co.uk"), "{s}");
        assert!(s.contains(MASK_EMAIL), "{s}");
        assert_eq!(r.emails, 1);
        assert!(r.emails == 1, "a@b 没有点，不算邮箱");
    }

    #[test]
    fn 路径的转义形式也会被折叠() {
        // 同一个路径在不同承载里的字面量不同：
        //   纯文本日志 → `C:\Users\x`
        //   JSON       → `C:\\Users\\x`
        // 而"按已知值替换"是**字面量匹配** —— 不把转义形式也喂进去，
        // 就会出现"日志里抹掉了、JSON 里没抹掉"：一处泄漏，另一处看起来很正常。
        //
        // 这条**放在 `Scrubber::path` 里做**，而不是让调用点自己记得加两次，
        // 理由很直接：**调用点会忘**，而遗忘的表现是"某一种承载里静默不脱敏"。
        let sb = Scrubber::new().path(r"C:\Users\x", MASK_USERPROFILE);

        // 纯文本形式
        let (a, r1) = sb.scrub(r"at C:\Users\x\a.log");
        assert_eq!(r1.paths, 1, "{a}");
        assert!(a.contains(MASK_USERPROFILE), "{a}");

        // JSON 形式（反斜杠被转义）
        let (b, r2) = sb.scrub(r#"{"p":"C:\\Users\\x\\a.log"}"#);
        assert_eq!(r2.paths, 1, "JSON 里的转义形式也必须命中：{b}");
        assert!(b.contains(MASK_USERPROFILE), "{b}");
        assert!(!b.contains("Users"), "JSON 里不该残留路径：{b}");
    }

    #[test]
    fn 没有反斜杠的路径不会重复加种子() {
        // 去重是为了让统计数字有意义：同一个路径被算两次会让
        // "本次脱敏：路径×2"看起来像抹了两处。
        let sb = Scrubber::new().path("/home/x", MASK_DATA_ROOT);
        let (s, r) = sb.scrub("at /home/x/a");
        assert_eq!(r.paths, 1, "{s}");
    }

    #[test]
    fn 分段替换不会把掩码本身再脱一次() {
        // 掩码里含 `%`、`<`、`>` —— 不该被任何规则再次匹配。
        let sb = scrubber();
        let (once, _) = sb.scrub(r"home C:\Users\hjc20");
        let (twice, r2) = sb.scrub(&once);
        assert_eq!(once, twice, "脱敏必须是幂等的");
        assert_eq!(r2.total(), 0, "第二次不该再有替换");
    }

    #[test]
    fn 脱敏是幂等的() {
        let input = "tok eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature \
                     id 069a79f4-44e9-4726-a5be-fca90e38aaf5 \
                     ip 192.168.1.1 user hjc20 mail a@b.co \
                     path C:\\Users\\hjc20\\AppData\\Roaming\\Qinmo\\x \
                     srv play.example.com";
        let sb = scrubber();
        let (a, _) = sb.scrub(input);
        let (b, _) = sb.scrub(&a);
        assert_eq!(a, b, "幂等：脱敏过的文本再脱一次应当不变");
    }

    // ───────────────── 统计与摘要 ─────────────────

    #[test]
    fn 统计只记次数不记内容() {
        // 这条纪律的用意：让脱敏过程本身**不可能泄露它抹掉的东西**。
        // 若 report 里存了"抹掉的是什么"，诊断包就成了新的泄露源。
        let (_, r) =
            scrubber().scrub("a 192.168.1.1 b 10.0.0.1 c 069a79f4-44e9-4726-a5be-fca90e38aaf5");
        assert_eq!(r.ips, 2);
        assert_eq!(r.uuids, 1);
        let j = serde_json::to_string(&r).unwrap();
        assert!(!j.contains("192.168"), "统计里不该有原值：{j}");
        assert!(!j.contains("069a79f4"), "统计里不该有原值：{j}");
    }

    #[test]
    fn 摘要能说清抹了什么类别() {
        let (_, r) = scrubber().scrub("ip 192.168.1.1 id 069a79f4-44e9-4726-a5be-fca90e38aaf5");
        let s = r.summary();
        assert!(s.contains("IP"), "{s}");
        assert!(s.contains("UUID"), "{s}");
        assert_eq!(ScrubReport::default().summary(), "本次脱敏：无替换");
    }

    #[test]
    fn 空白文本与无敏感内容时不改变原文() {
        let sb = scrubber();
        for c in ["", "hello world", "没有敏感内容的一行"] {
            let (s, r) = sb.scrub(c);
            assert_eq!(s, c, "不该改动无敏感内容的文本");
            assert_eq!(r.total(), 0, "{c}");
        }
    }

    #[test]
    fn 多行脱敏会累加统计() {
        let lines = vec!["ip 10.0.0.1", "ip 10.0.0.2", "nothing"];
        let (out, r) = scrubber().scrub_lines(lines);
        assert_eq!(out.len(), 3);
        assert_eq!(r.ips, 2);
        assert!(out[0].contains(MASK_IP));
        assert_eq!(out[2], "nothing");
    }

    // ───────────────── 构造器的边界 ─────────────────

    #[test]
    fn 过短的机密被忽略以免把日志打满() {
        // 长度 < 8 的串在文本里会大量偶然出现，
        // 全局替换会把日志打成一堆 <redacted>，反而看不出发生了什么。
        let sb = Scrubber::new().secret("abc");
        let (s, r) = sb.scrub("abcabc some abc text");
        assert_eq!(s, "abcabc some abc text", "过短的机密不该被采纳");
        assert_eq!(r.secrets, 0);
    }

    #[test]
    fn 空的种子一律被忽略() {
        let sb = Scrubber::new()
            .secret("")
            .user("")
            .path("", MASK_DATA_ROOT)
            .server("   ");
        let (s, r) = sb.scrub("nothing to do");
        assert_eq!(s, "nothing to do");
        assert_eq!(r.total(), 0);
    }

    #[test]
    fn 大小写不敏感替换() {
        // Windows 路径大小写不敏感，而日志里大小写会变。
        let sb = Scrubber::new().path(r"c:\users\HJC20", MASK_USERPROFILE);
        let (s, r) = sb.scrub(r"at C:\Users\hjc20\x");
        assert_eq!(r.paths, 1);
        assert!(s.contains(MASK_USERPROFILE), "{s}");
        assert!(!s.to_lowercase().contains("hjc20"), "{s}");
    }

    #[test]
    fn 中文不影响替换边界() {
        // 前一个字符是多字节时不能把替换位置算错。
        let sb = Scrubber::new().secret("supersecretvalue123");
        let (s, r) = sb.scrub("令牌=supersecretvalue123 结束");
        assert_eq!(r.secrets, 1, "多字节前缀不该影响匹配");
        assert!(s.starts_with("令牌="), "{s}");
        assert!(!s.contains("supersecretvalue123"), "{s}");
        assert!(s.ends_with("结束"), "{s}");
    }
}
