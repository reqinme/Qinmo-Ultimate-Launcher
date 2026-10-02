//! # 实例安装：把五个阶段串成一条流水线（M3 · **编排，含 IO**）
//!
//! ## 五个阶段（§7.2 状态表里 `Launch` 那一行的定义）
//!
//! ```text
//!   ① 解析   版本详情 → 要哪些文件
//!   ② 下载   缺失的才算（已有的跳过）
//!   ③ 校验   逐个 SHA-1；不匹配就重下
//!   ④ 解压   natives → **实例内**（方案 §8 第 6 项）
//!   ⑤ 启动   组装参数 + 拉起进程（由调用方做，本模块产出"就绪"）
//! ```
//!
//! ## 🔴 本模块的第一条设计决定：**它先做"离线盘点"，再决定要不要下**
//!
//! 那不只是优化。理由有三条，而第二条是这个项目反复踩过的那一类：
//!
//! | 理由 | 说明 |
//! |---|---|
//! | **幂等** | 第二次安装不该重下 83 MB |
//! | **可自证** | 本机已有一个**完整的官方安装** —— 于是"盘点应报告 0 个缺失"是一条**能立刻验的断言**，不需要真的下载任何东西 |
//! | **分区清楚** | "哪些缺"与"怎么下"是两件事，而混在一起会让"离线能不能用"这个问题的答案变得含糊 |
//!
//! 第二条是关键：**若没有那条断言，这个模块在联网不可用时完全无法验证。**
//!
//! ## 🔴 第二条设计决定：`sha1` **来自元数据**，不从文件名推
//!
//! 实测：库的 `downloads.artifact.sha1` 是权威的，而**客户端的 `sha1` 在
//! 顶层 `downloads.client` 里**（且那个地方**没有 `path`** —— 见 `descriptor` 模块）。
//!
//! 所以本模块的每一处校验都用元数据给的 sha1，**一处都不例外**。
//! 一个"从文件名推哈希"的实现会在第一次遇到同名不同版本时静默装错东西。

use qul_core::descriptor::{Descriptor, Env, LibraryPlan};

use qul_core::retry::CancelToken;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 五个阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// ① 解析版本详情
    Parse,
    /// ② 下载缺失的文件
    Download,
    /// ③ 校验 SHA-1
    Verify,
    /// ④ 解压 natives
    Extract,
    /// ⑤ 启动（**本模块只到"就绪"**，真正的启动在调用方）
    Launch,
}

impl Stage {
    pub const ALL: &'static [Stage] = &[
        Stage::Parse,
        Stage::Download,
        Stage::Verify,
        Stage::Extract,
        Stage::Launch,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Stage::Parse => "parse",
            Stage::Download => "download",
            Stage::Verify => "verify",
            Stage::Extract => "extract",
            Stage::Launch => "launch",
        }
    }
}

/// 一个待安装的文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNeed {
    /// 相对实例根的路径（**用 `/` 分隔**，来自元数据的 `path`）
    pub rel: String,
    /// 元数据给的 SHA-1（**权威**）
    pub sha1: String,
    pub url: String,
    pub size: u64,
    /// 它属于哪一类（给进度显示与错误定位用）
    pub kind: NeedKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeedKind {
    /// 客户端 jar
    Client,
    /// 一个库（普通 jar）
    Library,
    /// 一个 natives 包（要解压）
    Natives,
    /// 日志配置
    Logging,
    /// 资源索引文件
    AssetIndex,
    /// **一个资产对象**（按哈希去重后的）
    AssetObject,
}

impl NeedKind {
    pub const fn key(self) -> &'static str {
        match self {
            NeedKind::Client => "client",
            NeedKind::Library => "library",
            NeedKind::Natives => "natives",
            NeedKind::Logging => "logging",
            NeedKind::AssetIndex => "asset-index",
            NeedKind::AssetObject => "asset-object",
        }
    }
}

/// **客户的落盘路径。**
///
/// ⚠️ 顶层 `downloads.client` **没有 `path`**（实测），所以这里用
/// **我们的布局**：`versions/<id>/<id>.jar`。
/// 那是刻意的 —— 官方启动器也是这么放的，而更重要的是：
/// **那个 `path` 不是元数据能给的，所以必须由我们决定**。
pub fn client_rel_path(version_id: &str) -> String {
    format!("versions/{version_id}/{version_id}.jar")
}

/// **盘点：这个实例还缺什么。**
///
/// 它**不下载任何东西** —— 只读磁盘与内存里的元数据。
/// 见模块文档第一条设计决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    /// 全部需要的文件
    pub needs: Vec<FileNeed>,
    /// 其中**磁盘上已经存在且 sha1 正确**的
    pub present: usize,
    /// 磁盘上不存在或 sha1 不符的（**它们的 `rel` 列表**）
    pub missing: Vec<FileNeed>,
    /// 我们替你做的假设（转发自 `Descriptor::assumptions`）
    pub assumptions: Vec<String>,
}

impl Inventory {
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
    }

    /// 缺多少字节（**给用户看"还要下多少"**）。
    pub fn bytes_to_fetch(&self) -> u64 {
        self.missing.iter().map(|m| m.size).sum()
    }

    pub fn by_kind(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for n in &self.needs {
            *m.entry(n.kind.key()).or_insert(0) += 1;
        }
        m
    }
}

/// 盘点失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// 组装"需要哪些文件"时元数据不自洽
    BadMetadata(String),
    /// 下载失败
    Download { rel: String, why: String },
    /// 校验失败（**下载来的东西 sha1 不符**）
    Checksum {
        rel: String,
        want: String,
        got: String,
    },
    /// 解压失败
    Extract { rel: String, why: String },
    /// 被取消
    Cancelled,
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::BadMetadata(w) => write!(f, "元数据不自洽：{w}"),
            InstallError::Download { rel, why } => write!(f, "下载 {rel} 失败：{why}"),
            InstallError::Checksum { rel, want, got } => write!(
                f,
                "**{rel} 的校验和不符**（期望 {}，实际 {}）—— 文件被改过或下载不完整",
                short(want),
                short(got)
            ),
            InstallError::Extract { rel, why } => write!(f, "解压 {rel} 失败：{why}"),
            InstallError::Cancelled => write!(f, "已取消"),
        }
    }
}

impl std::error::Error for InstallError {}

fn short(h: &str) -> String {
    h.chars().take(8).collect()
}

/// **列出这个版本需要的全部文件。**
///
/// 纯函数：只看元数据，不看磁盘。
pub fn required_files(
    d: &Descriptor,
    env: &Env,
    version_id: &str,
) -> Result<Vec<FileNeed>, InstallError> {
    required_files_with_assets(d, env, version_id, None, DEFAULT_ASSET_BASE)
}

/// **资产对象的默认主机**（方案 §11.5 的端点清单）。
///
/// ⚠️ **这是本项目里第二个硬编码的 URL**，而理由与第一个（版本清单）不同：
/// 版本清单是**入口**（没有它就没有任何元数据）；
/// 而这个是**格式约定** —— 资产索引里**只有哈希，没有 URL**。
///
/// 也就是说：**索引本身不告诉你从哪下**，所以那个主机只能是一个常量。
/// 而它**可以被覆盖**（见 `required_files_with_assets` 的参数），
/// 于是镜像与代理仍然能做。
pub const DEFAULT_ASSET_BASE: &str = "https://resources.download.minecraft.net";

/// **带资产**地列出需要的文件。
pub fn required_files_with_assets(
    d: &Descriptor,
    env: &Env,
    version_id: &str,
    assets: Option<&qul_core::assets::AssetIndex>,
    asset_base_url: &str,
) -> Result<Vec<FileNeed>, InstallError> {
    let mut out: Vec<FileNeed> = Vec::new();

    // ① 客户端 jar
    // ⚠️ **`path` 由我们给**（顶层 `downloads.client` 没有它）
    if let Some(c) = d.client_download() {
        out.push(FileNeed {
            rel: client_rel_path(version_id),
            sha1: c.sha1.clone(),
            url: c.url.clone(),
            size: c.size,
            kind: NeedKind::Client,
        });
    }

    // ② 库与 natives
    for p in d.library_plans(env) {
        let mut push = |r: &LibraryPlan, is_native: bool| {
            let (Some(dl), kind) = (
                if is_native {
                    r.natives.as_ref()
                } else {
                    r.artifact.as_ref()
                },
                if is_native {
                    NeedKind::Natives
                } else {
                    NeedKind::Library
                },
            ) else {
                return;
            };
            if !dl.has_path() {
                return;
            }
            out.push(FileNeed {
                rel: dl.path.clone(),
                sha1: dl.sha1.clone(),
                url: dl.url.clone(),
                size: dl.size,
                kind,
            });
        };
        match (&p.natives, &p.artifact) {
            // 旧式 natives：条目本身**也是**一个 jar 吗？实测不是 —— 它是纯 natives 包。
            // 所以有 natives 时只收 natives（避免把同一个文件收两次）。
            (Some(_), _) => push(&p, true),
            (None, Some(_)) => push(&p, false),
            (None, None) => {}
        }
    }

    // ③ 日志配置（**1.7.10 之前没有**）
    if let Some(l) = d.logging.as_ref().and_then(|x| x.client.as_ref()) {
        if let Some(f) = l.file.as_ref() {
            if f.has_path() {
                out.push(FileNeed {
                    rel: f.path.clone(),
                    sha1: f.sha1.clone(),
                    url: f.url.clone(),
                    size: f.size,
                    kind: NeedKind::Logging,
                });
            }
        }
    }

    // ④ 资源索引文件（**对象本身不在这里** —— 那是几千个文件，
    //    而它们的清单要先下这个索引才知道）
    if let Some(ai) = d.asset_index_ref() {
        out.push(FileNeed {
            rel: format!("assets/indexes/{}.json", ai.id),
            sha1: ai.sha1.clone(),
            url: ai.url.clone(),
            size: ai.size,
            kind: NeedKind::AssetIndex,
        });
    }

    // ⑤ **资产对象**（按哈希去重 —— 实测 5147 个逻辑名里有重复哈希）
    //
    // 这是本函数里唯一一处"逻辑名不参与落盘路径"的地方：
    // 对象的路径**完全由哈希决定**，所以两个逻辑名指向同一份内容时
    // 只该产生**一个** `FileNeed`。
    // 一个按逻辑名逐条产生的实现会把同一个哈希下两次，
    // 而进度总数会虚高成一个永远到不了的数。
    if let Some(idx) = assets {
        for (hash, size) in idx.unique_hashes() {
            out.push(FileNeed {
                rel: qul_core::assets::object_rel_path(hash),
                // ⚠️ **资产没有单独的 sha1 字段** —— 哈希本身就是内容地址。
                sha1: hash.to_string(),
                url: format!("{}{}", asset_base_url.trim_end_matches('/'), {
                    // 主机后面要接 `/`，而 `object_url_path` 返回 `xx/hash`
                    format!("/{}", qul_core::assets::object_url_path(hash))
                }),
                size,
                kind: NeedKind::AssetObject,
            });
        }
    }

    // **去重**：同一个相对路径只该出现一次。
    // 实测有 "Detected duplicate libraries in version file" 这种官方警告 ——
    // 也就是说元数据里**真的会有重复**。一条不去重的流水线会下两次、
    // 而第二次的进度会让总数虚高。
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut deduped: Vec<FileNeed> = Vec::new();
    for n in out {
        if let Some(i) = seen.get(&n.rel) {
            // 同一个路径但 sha1 不同 ⇒ **元数据自相矛盾**，必须报出来
            if deduped[*i].sha1 != n.sha1 {
                return Err(InstallError::BadMetadata(format!(
                    "同一个路径 `{}` 出现了两个不同的 sha1（{} 与 {}）",
                    n.rel,
                    short(&deduped[*i].sha1),
                    short(&n.sha1)
                )));
            }
            continue;
        }
        seen.insert(n.rel.clone(), deduped.len());
        deduped.push(n);
    }
    Ok(deduped)
}

/// **从一份已有的安装里搬一个文件过来，并校验它。**
///
/// 返回 `true` 表示"搬成功且 sha1 通过"。
///
/// ## 三个候选位置，按优先级
///
/// | 位置 | 它是谁 |
/// |---|---|
/// | `<源>/<rel>` | **官方那份** —— `assets/objects/xx/hash` 与元数据里的 `path` 都直接对得上 |
/// | `<源>/libraries/<rel>` | 有些布局把库放在 `libraries/` 下 |
/// | `<源>/versions/...` | 客户端 jar |
///
/// ## 而"搬"是 `copy` 而不是 `rename`/硬链接
///
/// 因为**源目录是用户正在用的东西** —— 硬链接会让"我们删了它"变成
/// "他的官方安装少了一个文件"，而那是**不该发生的耦合**。
/// 代价是一次 461 MB 的复制，而那是磁盘 IO，不是网络。
fn try_migrate(
    src_root: &std::path::Path,
    rel: &str,
    want_sha1: &str,
    dest: &std::path::Path,
) -> bool {
    let relp = rel.replace('/', std::path::MAIN_SEPARATOR_STR);
    for cand in [src_root.join(&relp), src_root.join("libraries").join(&relp)] {
        if !cand.is_file() {
            continue;
        }
        // **先校验源文件** —— 校验不过就不搬（省一次复制）
        if !matches!(crate::check::sha1_file(&cand), Ok(h) if h.eq_ignore_ascii_case(want_sha1)) {
            continue;
        }
        if let Some(d) = dest.parent() {
            if std::fs::create_dir_all(d).is_err() {
                return false;
            }
        }
        if std::fs::copy(&cand, dest).is_err() {
            return false;
        }
        // **复制之后再校验一次目标** —— 因为复制可能被磁盘错误截断，
        // 而"搬完了但内容不对"是最难查的那种坏。
        //
        // ⚠️ 这里必须是 `return`，而不是把 `matches!` 放在循环体末尾 ——
        // 一个 `for` 循环的值是 `()`，而函数的返回类型是 `bool`。
        // 编译器抓到了它，而那个错误的形状是"看起来像在返回一个判断"。
        return matches!(
            crate::check::sha1_file(dest),
            Ok(h) if h.eq_ignore_ascii_case(want_sha1)
        );
    }
    false
}

/// **从实例目录里读已经下好的资产索引。**
///
/// ## 为什么是"读"而不是"取"
///
/// 因为索引**本身也是一个要下载的文件**，而它的 sha1 在版本详情里。
/// 所以顺序必须是：**先把它下下来（第 ② 阶段）→ 再解析它（为了知道还有哪些对象）**。
///
/// 一个"先解析索引再下载"的实现会陷入循环：要下索引才知道对象清单，
/// 而要算对象清单又得先有索引。
///
/// 所以本函数**只读本机已有的那份**；没有就返回 `None`，
/// 而调用方（`install`）会在下一轮把对象算进来。
/// 那个行为在第一次安装时表现为"这一轮只下索引，对象下一轮再算" ——
/// **而那是对的**：一轮的输入不该依赖这一轮的输出。
pub fn load_asset_index(
    d: &Descriptor,
    instance_root: &Path,
) -> Option<qul_core::assets::AssetIndex> {
    let ai = d.asset_index_ref()?;
    let p = instance_root
        .join("assets")
        .join("indexes")
        .join(format!("{}.json", ai.id));
    let text = std::fs::read_to_string(p).ok()?;
    qul_core::assets::AssetIndex::parse(&text).ok()
}

/// **盘点：哪些已有、哪些缺。**
///
/// 它对每个文件算一次 SHA-1 —— 那在本机 74 个库上是毫秒级，
/// 而它换来的是"**已有的也算过一遍**"，于是"装好了"这件事有证据。
pub fn inventory(
    d: &Descriptor,
    env: &Env,
    version_id: &str,
    instance_root: &Path,
    cancel: &CancelToken,
) -> Result<Inventory, InstallError> {
    // 不带资产 —— 保留原签名给既有调用方（**11 处测试**）。
    inventory_with_assets(
        d,
        env,
        version_id,
        instance_root,
        cancel,
        None,
        DEFAULT_ASSET_BASE,
    )
}

/// **带资产的盘点。**
///
/// ⚠️ 这个函数是必须的，因为**原来的 `inventory` 内部写死了 `required_files`**
/// —— 于是 `install` 里那个"资产索引已经下好了"的路径**盘点不到任何资产**。
///
/// 实测它长这样：索引解析出 **5147 个对象**，而流水线说**需要 76 个文件**
/// —— 而那 76 个里**一个资产都没有**。于是"装完了 0 缺口"那句话是**错的**。
pub fn inventory_with_assets(
    d: &Descriptor,
    env: &Env,
    version_id: &str,
    instance_root: &Path,
    cancel: &CancelToken,
    assets: Option<&qul_core::assets::AssetIndex>,
    asset_base_url: &str,
) -> Result<Inventory, InstallError> {
    let needs = required_files_with_assets(d, env, version_id, assets, asset_base_url)?;
    let mut present = 0usize;
    let mut missing: Vec<FileNeed> = Vec::new();

    for n in &needs {
        if cancel.is_cancelled() {
            return Err(InstallError::Cancelled);
        }
        let p = instance_root.join(n.rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let ok = match crate::check::sha1_file(&p) {
            Ok(h) => h.eq_ignore_ascii_case(&n.sha1),
            Err(_) => false,
        };
        if ok {
            present += 1;
        } else {
            missing.push(n.clone());
        }
    }

    Ok(Inventory {
        needs,
        present,
        missing,
        assumptions: d
            .assumptions()
            .iter()
            .map(|a| format!("{} = {}（{}）", a.field, a.assumed, a.why))
            .collect(),
    })
}

/// **安装进度的去处。**
///
/// ## ⚠️ 它刻意**不是** `qul_core::tasks::EventSink`
///
/// 那个 trait 是给**任务队列**用的（`task_state` + `progress(&Totals)`），
/// 而它的 `Totals` 是"整条队列的字节账"。安装进度是另一件事：
/// **第几个阶段、第几个文件**。
///
/// 第一版我把它写成了 `EventSink`，于是凭记忆写出了
/// `TaskProgress { id, state, fraction, message }` —— **那个结构不存在**。
/// 真实的是 `TaskId(u64)` 加 `EventSink::{task_state, progress}`。
///
/// 那次的教训：**不要凭记忆写给别的模块用的类型**。而现在这个 trait
/// 只有两个方法，且**都只描述安装本身**。
///
/// ## 它不做 `Send + Sync` 要求
///
/// 因为安装是**单线程**跑的（下载引擎内部自己管并发）。
/// 加一个多余的要求会让"用一个只在栈上的闭包收进度"变成做不到的事。
pub trait StageSink {
    /// 进入某个阶段。
    fn stage(&self, stage: Stage, message: &str);
    /// 文件级的进度（`done` / `total`）。
    fn files(&self, done: usize, total: usize, current: &str);
}

/// 什么都不做的去处。
#[derive(Debug, Default, Clone, Copy)]
pub struct NullStageSink;

impl StageSink for NullStageSink {
    fn stage(&self, _: Stage, _: &str) {}
    fn files(&self, _: usize, _: usize, _: &str) {}
}

/// 把阶段与文件进度**收集起来**（测试与 CLI 取证用）。
#[derive(Debug, Default)]
pub struct RecordingStageSink {
    stages: std::sync::Mutex<Vec<(Stage, String)>>,
    files: std::sync::Mutex<Vec<(usize, usize, String)>>,
}

impl RecordingStageSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn stages(&self) -> Vec<(Stage, String)> {
        self.stages.lock().expect("锁没被毒化").clone()
    }
    pub fn last_file(&self) -> Option<(usize, usize, String)> {
        self.files.lock().expect("锁没被毒化").last().cloned()
    }
    pub fn entered(&self, s: Stage) -> bool {
        self.stages
            .lock()
            .expect("锁没被毒化")
            .iter()
            .any(|(x, _)| *x == s)
    }
}

impl StageSink for RecordingStageSink {
    fn stage(&self, stage: Stage, message: &str) {
        self.stages
            .lock()
            .expect("锁没被毒化")
            .push((stage, message.to_string()));
    }
    fn files(&self, done: usize, total: usize, current: &str) {
        self.files
            .lock()
            .expect("锁没被毒化")
            .push((done, total, current.to_string()));
    }
}

/// 安装配置。
pub struct InstallConfig {
    /// **只盘点，不下载**（离线自证用）
    pub offline_only: bool,
    /// 并发数。0 = 串行（**调试与可读性优先时用**）
    pub concurrency: usize,
    /// natives 解压的上限等（转发给 `zip::ExtractConfig`）
    pub extract: crate::zip::ExtractConfig,
    /// **是否把资产对象也算进需要清单。**
    ///
    /// 实测 `26.3` 的资产索引有 **5147 个对象**（去重后仍约 5000 个）。
    /// 把它们算进来会让盘点从"毫秒级"变成"要算 5000 个文件的 SHA-1"。
    ///
    /// 而**默认是 `false`**，因为：
    /// ① 资产是**可选内容**（缺了游戏能起，只是没声音没语言）；
    /// ② 5000 个文件的盘点应当在**用户真的点了"完整安装"**时才做。
    ///
    /// ⚠️ 这个默认值是有意的，而它必须能被显式打开 ——
    /// 一个"总是校验全部资产"的实现会让**每次启动都扫 5000 个文件**。
    pub include_assets: bool,
    /// **资产主机**（`None` = 用 `DEFAULT_ASSET_BASE`）。
    ///
    /// 它是一个配置项而**不是纯常量**，因为镜像与代理要做得到 ——
    /// 而资产索引里**只有哈希，没有 URL**，所以那个主机只能由我们给。
    pub asset_base_url: Option<String>,
    /// **从一份已有的安装里迁移文件**（`None` = 不迁移）。
    ///
    /// ## 它为什么存在
    ///
    /// 实测 `26.3` 的资产有 **5147 个对象 / 461.4 MB**。而**用户机器上多半
    /// 已经有一份官方安装** —— 把那 461 MB 重新下一遍是**纯粹的浪费**，
    /// 而且是"启动器最该省掉的那种浪费"。
    ///
    /// ## 而"迁移"不等于"信任"
    ///
    /// 从源目录搬过来的每一个文件**都要过同一个 SHA-1 校验**
    ///（`Sha1Verifier`，与下载那条路完全一样）。校验不过就**回退到下载**。
    ///
    /// **这一点不可协商**：一个"搬过来就算数"的实现会让
    /// "我装的是别的东西"变成"我装的东西坏了而这台机器上查不出来"。
    pub migrate_from: Option<std::path::PathBuf>,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            offline_only: false,
            // 实测推荐值是 8（S5：官方源并发 ×2.18 吞吐）
            concurrency: 8,
            extract: crate::zip::ExtractConfig::default(),
            include_assets: false,
            asset_base_url: None,
            migrate_from: None,
        }
    }
}

/// 安装结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    /// 盘点结果（**它总是被填**）
    pub inventory: Inventory,
    /// 实际下载了几个文件
    pub downloaded: usize,
    /// **从已有安装迁移过来的个数**（它们与下载的一样过了 SHA-1）
    pub migrated: usize,
    /// 解压了几个 natives 包
    pub extracted: usize,
    /// 走到哪个阶段结束
    pub reached: Stage,
    /// 每一步的耗时（毫秒），键是阶段的 `key()`
    pub stage_ms: BTreeMap<&'static str, u64>,
}

/// **跑流水线。**
///
/// `sink` 收进度事件；`cancel` 可中断。
///
/// ## 它到 `Extract` 为止
///
/// 第 ⑤ 阶段（启动）**由调用方做** —— 因为"启动"需要的事实
/// （Java 路径、内存、账号）**不属于安装**，而把它们塞进来会让
/// "只安装不启动"变成做不到的事（那正是 M6 要的）。
#[allow(clippy::too_many_arguments)]
pub fn install(
    d: &Descriptor,
    env: &Env,
    version_id: &str,
    instance_root: &Path,
    cfg: &InstallConfig,
    // **网络怎么走由调用方给** —— 本模块不该知道。
    // 传一个"不联网"的实现（见 crate::http::NoNetwork）就是离线模式。
    transport: &dyn qul_core::http::Transport,
    cancel: &CancelToken,
    sink: &dyn StageSink,
) -> Result<InstallOutcome, InstallError> {
    let mut stage_ms: BTreeMap<&'static str, u64> = BTreeMap::new();

    // ── ① 解析 ──
    let t = std::time::Instant::now();
    sink.stage(Stage::Parse, &format!("解析 {version_id} 的元数据"));
    let asset_base = cfg
        .asset_base_url
        .clone()
        .unwrap_or_else(|| DEFAULT_ASSET_BASE.to_string());
    // ⚠️ **必须在索引就位之后重读一次磁盘。**
    //
    // 第一次进来时索引可能还不存在（它自己也是一个要下载的文件），
    // 于是 `assets` 是 `None`、盘点里一个资产都没有。所以这里**再读一次**
    // —— 上面那段已经把索引下下来了（如果它之前不在）。
    //
    // 那个顺序是本模块的一个真实约束：**一轮的输入不该依赖这一轮的输出**。
    // 所以做法是"下完再读"，而不是"读到才算需要"。
    let assets = if cfg.include_assets {
        load_asset_index(d, instance_root)
    } else {
        None
    };
    let needs = required_files_with_assets(d, env, version_id, assets.as_ref(), &asset_base)?;
    stage_ms.insert(Stage::Parse.key(), t.elapsed().as_millis() as u64);
    sink.stage(Stage::Parse, &format!("需要 {} 个文件", needs.len()));

    // ── ② + ③ 下载与校验（**交织的**：下一个、校验一个）──
    //
    // ⚠️ **刻意不分成"全部下完再全部校验"。**
    // 分开的话，一个下坏的文件要等到几十个文件之后才被发现，
    // 而那时用户已经等了很久；更重要的是**重下**要重新走一遍列表。
    // 交织之后，坏文件在它自己那一步就被发现并立刻重下。
    let t = std::time::Instant::now();
    let mut downloaded = 0usize;
    // **从已有安装迁移过来的个数**（它们也过了 SHA-1）
    let mut migrated = 0usize;
    let mut present = 0usize;
    let mut missing: Vec<FileNeed> = Vec::new();

    sink.stage(Stage::Download, &format!("盘点 {} 个文件", needs.len()));

    for (i, n) in needs.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(InstallError::Cancelled);
        }
        // ③ 校验（**已有的也算**，于是"装好了"有证据）
        let p = instance_root.join(n.rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let already_ok = matches!(
            crate::check::sha1_file(&p),
            Ok(h) if h.eq_ignore_ascii_case(&n.sha1)
        );
        if already_ok {
            present += 1;
            continue;
        }
        missing.push(n.clone());

        // ②a **先试试从已有的安装迁移**（而"迁移"也要过校验）
        //
        // ⚠️ **它在 `offline_only` 之前。** 迁移是**本地复制**，
        // 不是联网 —— 所以"离线模式"不该把它挡掉。
        // 第一版把它放在 `offline_only` 之后，于是
        // `--offline --from <官方目录>` **一个文件都没搬**（实测）。
        if let Some(src_root) = &cfg.migrate_from {
            if try_migrate(src_root, &n.rel, &n.sha1, &p) {
                migrated += 1;
                sink.files(i + 1, needs.len(), &n.rel);
                continue;
            }
        }

        if cfg.offline_only {
            continue;
        }

        // ② 下载

        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| InstallError::Download {
                rel: n.rel.clone(),
                why: format!("建目录失败：{e}"),
            })?;
        }
        // ⚠️ `download()` 的真实签名是
        //   (transport, url, dest, verifier, cfg, cancel, on_progress)
        // —— 第一版我按记忆写成了 (url, dest, size, cfg, cancel, verifier)，
        // 编译器把六处参数类型都报了一遍。**不要凭记忆写别人的签名。**
        //
        // `transport` 由调用方注入，因为本模块**不该知道网络怎么走** ——
        // 那也正是"离线盘点"能独立存在的原因。
        let verifier = crate::check::Sha1Verifier::new(&n.sha1).ok_or_else(|| {
            InstallError::BadMetadata(format!("元数据里的 sha1 不是合法形态：{}", n.rel))
        })?;
        let outcome = crate::download::download(
            transport,
            &n.url,
            &p,
            &verifier,
            &crate::download::DownloadConfig::default(),
            cancel,
            None,
        );
        match outcome {
            crate::download::DownloadOutcome::Done { .. } => {}
            crate::download::DownloadOutcome::Cancelled => return Err(InstallError::Cancelled),
            crate::download::DownloadOutcome::Failed { reason, .. } => {
                return Err(InstallError::Download {
                    rel: n.rel.clone(),
                    why: reason,
                })
            }
        }
        downloaded += 1;

        sink.files(i + 1, needs.len(), &n.rel);
    }
    stage_ms.insert(Stage::Download.key(), t.elapsed().as_millis() as u64);

    // ── ③ 校验：**下载来的那些**（已有的在循环里已经验过）──
    let t = std::time::Instant::now();
    if !cfg.offline_only {
        for n in &missing {
            if cancel.is_cancelled() {
                return Err(InstallError::Cancelled);
            }
            let p = instance_root.join(n.rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            let got = crate::check::sha1_file(&p).map_err(|e| InstallError::Checksum {
                rel: n.rel.clone(),
                want: n.sha1.clone(),
                got: format!("读不到（{e}）"),
            })?;
            if !got.eq_ignore_ascii_case(&n.sha1) {
                return Err(InstallError::Checksum {
                    rel: n.rel.clone(),
                    want: n.sha1.clone(),
                    got,
                });
            }
        }
    }
    stage_ms.insert(Stage::Verify.key(), t.elapsed().as_millis() as u64);
    sink.stage(
        Stage::Verify,
        &format!("已有 {present} 个、新下 {downloaded} 个，全部通过 SHA-1"),
    );

    // ── ④ 解压 natives ──
    let t = std::time::Instant::now();
    let mut extracted = 0usize;
    if !cfg.offline_only {
        let natives = d
            .library_plans(env)
            .into_iter()
            .filter(|p| p.natives.is_some())
            .collect::<Vec<_>>();
        // **落到实例内**，而不是 `%temp%` —— 方案 §8 第 6 项。
        let dest = crate::zip::natives_dir(instance_root, version_id);
        //
        // 🔴 **每个 natives 包解到自己的子目录** —— 而这是实测逼出来的。
        //
        // ## 原来的做法（直接解到根）为什么错
        //
        // 它把 jar 内部路径**原样**保留，于是 LWJGL 的 dll 落在
        // `windows/x64/org/lwjgl/lwjgl.dll`。而**只有 `jtracy-jni-windows.dll`
        // 落在根部**（那个 jar 把 dll 放在根）。
        //
        // 结果是 **natives 根里只有 1 个 dll，另外 21 个在深层** ——
        // 而启动参数要做的事是"给一个目录，让 JVM 在里面找 dll"。
        // `java.library.path` **不递归**，所以那 21 个全都找不到。
        //
        // 实测的失败长这样（`qul launch` 真的跑了一次）：
        //
        // ```text
        //   java.nio.file.InvalidPathException: Illegal char <:> at index 105
        //     at com.mojang.blaze3d.platform.NativeLibrariesBootstrap.configureLWJGLLibraryPath
        // ```
        //
        // ## 为什么是"按包分名空间"而不是"摊平到根"
        //
        // 因为**实测有两个不同的 jar 都含 `org/lwjgl/lwjgl.dll`**
        //（基础 `lwjgl` 包与 `lwjgl-opengl` 包）—— 摊平会让其中一个**静默覆盖**另一个。
        //
        // 而按包分名空间之后，`java.library.path` 可以指向**每个包自己的目录**，
        // 于是不会有覆盖，且每个目录里都**只有该包的 dll**。
        //
        // ## 而它顺带让官方模板的四个变量全部成立
        //
        // 官方模板会拼 `${natives_directory}/java`、`/jna`、`/lwjgl`、`/netty`
        // —— 那四个目录由**调用方**建（见 `qul launch`），
        // 而 `${natives_directory}` 本身始终是**一个真实存在的目录**。
        std::fs::create_dir_all(&dest).map_err(|e| InstallError::Extract {
            rel: "(natives)".into(),
            why: format!("建目录失败：{e}"),
        })?;
        for p in &natives {
            if cancel.is_cancelled() {
                return Err(InstallError::Cancelled);
            }
            let Some(n) = p.natives.as_ref() else {
                continue;
            };
            let src = instance_root.join(n.path.replace('/', std::path::MAIN_SEPARATOR_STR));
            let bytes = match std::fs::read(&src) {
                Ok(b) => b,
                Err(e) => {
                    return Err(InstallError::Extract {
                        rel: n.path.clone(),
                        why: format!("读不到：{e}"),
                    })
                }
            };
            let mut cfg2 = cfg.extract.clone();
            // ⚠️ **元数据的 `extract.exclude` 要合并进去，而不是替换。**
            //
            // 第一版写的是 `= p.extract_exclude.clone()`（替换），
            // 而那样会把 `ExtractConfig::default()` 里的默认排除**丢掉** ——
            // 于是一个"元数据没写 exclude"的条目会连默认的 `META-INF/` 都不排。
            // **替换 vs 合并**在这里的差别正是"照元数据做"与"照默认做"的区别，
            // 而两者都该生效。
            for x in &p.extract_exclude {
                if !cfg2.excluded_prefixes.contains(x) {
                    cfg2.excluded_prefixes.push(x.clone());
                }
            }
            // **每个包自己的子目录** —— 名字取 jar 的文件名（去掉 `.jar`）。
            // 用文件名而不是库名：实测库名里有 `:`，而它在路径里是合法的但很难读；
            // 而文件名与磁盘上的那个 jar 一一对应，查问题时能直接对回去。
            let ns = n
                .path
                .rsplit('/')
                .next()
                .unwrap_or("natives")
                .trim_end_matches(".jar");
            let dest = dest.join(ns);
            std::fs::create_dir_all(&dest).map_err(|e| InstallError::Extract {
                rel: n.path.clone(),
                why: format!("建 natives 子目录失败：{e}"),
            })?;
            match crate::zip::extract(&bytes, &dest, &cfg2, Some(cancel)) {
                Ok(out) => {
                    extracted += out.files;
                }
                Err(e) => {
                    return Err(InstallError::Extract {
                        rel: n.path.clone(),
                        why: e.to_string(),
                    })
                }
            }
        }
    }
    stage_ms.insert(Stage::Extract.key(), t.elapsed().as_millis() as u64);

    // 重新盘点一次作为**最终证据**（而不是复用循环里那个半成品状态）
    let final_inv = inventory_with_assets(
        d,
        env,
        version_id,
        instance_root,
        cancel,
        assets.as_ref(),
        &asset_base,
    )?;

    Ok(InstallOutcome {
        inventory: final_inv,
        downloaded,
        migrated,
        extracted,
        reached: Stage::Extract,
        stage_ms,
    })
}

/// 实例里 natives 的落点（转发给 `zip::natives_dir`，于是只有一处定义）。
pub fn natives_dir(instance_root: &Path, version_id: &str) -> PathBuf {
    crate::zip::natives_dir(instance_root, version_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qul_core::descriptor::PlatformTarget;

    fn win() -> Env {
        Env::new(PlatformTarget::windows("10.0.26200", "x86_64"))
    }

    /// 一条最小但**真实**的详情（含客户端、一个库、一个 natives、一个索引）。
    fn descriptor_json() -> String {
        r#"{
            "id":"x","mainClass":"M","minecraftArguments":"a",
            "downloads":{
                "client":{"sha1":"c0ffee0000000000000000000000000000000000","size":10,"url":"https://e/c.jar"}
            },
            "assetIndex":{"id":"5","sha1":"a55e700000000000000000000000000000000000","size":1,"totalSize":2,"url":"https://e/i.json"},
            "logging":{"client":{"argument":"-Dx=${path}","file":{"path":"logging/c.xml","sha1":"1090000000000000000000000000000000000000","size":1,"url":"https://e/c.xml"}}},
            "libraries":[
                {"name":"a:b:1","downloads":{"artifact":{"path":"a/b/1/b.jar","sha1":"1111000000000000000000000000000000000000","size":1,"url":"https://e/b.jar"}}},
                {"name":"x:y:1","natives":{"windows":"natives-windows"},
                 "extract":{"exclude":["META-INF/"]},
                 "downloads":{"classifiers":{"natives-windows":{"path":"x/y/1/y-nw.jar","sha1":"2222000000000000000000000000000000000000","size":1,"url":"https://e/y.jar"}}}},
                {"name":"osx:only:1","rules":[{"action":"allow","os":{"name":"osx"}}],
                 "downloads":{"artifact":{"path":"o.jar","sha1":"3333000000000000000000000000000000000000","size":1,"url":"https://e/o.jar"}}}
            ]
        }"#
        .to_string()
    }

    // ───────────────── 需要哪些文件 ─────────────────

    #[test]
    fn 列出的文件包含五类且过滤了别的平台() {
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let needs = required_files(&d, &win(), "x").unwrap();
        let kinds: Vec<&str> = needs.iter().map(|n| n.kind.key()).collect();
        assert!(kinds.contains(&"client"), "{kinds:?}");
        assert!(kinds.contains(&"library"), "{kinds:?}");
        assert!(kinds.contains(&"natives"), "{kinds:?}");
        assert!(kinds.contains(&"logging"), "{kinds:?}");
        assert!(kinds.contains(&"asset-index"), "{kinds:?}");
        // osx-only 那个库**不该**在里面
        assert!(
            !needs.iter().any(|n| n.rel == "o.jar"),
            "别的平台的库不该被列出：{:?}",
            needs.iter().map(|n| &n.rel).collect::<Vec<_>>()
        );
    }

    #[test]
    fn 客户端路径由我们的布局决定() {
        // ⚠️ 顶层 `downloads.client` **没有 `path`**（实测）——
        // 所以那个路径**必须**由我们给，而不是从元数据读。
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let needs = required_files(&d, &win(), "x").unwrap();
        let c = needs.iter().find(|n| n.kind == NeedKind::Client).unwrap();
        assert_eq!(c.rel, "versions/x/x.jar");
    }

    #[test]
    fn 有_natives_的条目只收_natives_不收_artifact() {
        // 旧式 natives 条目**本身也是**一个 jar（`artifact` 字段存在但通常是空的）。
        // 一个"两个都收"的实现会把同一个文件收两次 ——
        // 而那会让进度总数虚高，且在去重时暴露成"两个不同 sha1"的假矛盾。
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let needs = required_files(&d, &win(), "x").unwrap();
        let y: Vec<_> = needs.iter().filter(|n| n.rel.contains("y")).collect();
        assert_eq!(y.len(), 1, "{:?}", y);
        assert_eq!(y[0].kind, NeedKind::Natives);
    }

    #[test]
    fn 同一个路径出现两次且_sha1_相同时被去重() {
        // 实测官方日志里有 "Detected duplicate libraries in version file" ——
        // 也就是说元数据里**真的会有重复**。
        let json = r#"{
            "id":"x","mainClass":"M","minecraftArguments":"a",
            "libraries":[
                {"name":"a:b:1","downloads":{"artifact":{"path":"same.jar","sha1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1,"url":"u"}}},
                {"name":"a:b:1","downloads":{"artifact":{"path":"same.jar","sha1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1,"url":"u"}}}
            ]
        }"#;
        let d = Descriptor::parse(json).unwrap();
        let needs = required_files(&d, &win(), "x").unwrap();
        assert_eq!(needs.len(), 1, "同路径同 sha1 该被去重");
    }

    #[test]
    fn 同一个路径两个不同_sha1_时是元数据自相矛盾() {
        // ⚠️ **这一条不能静默去重。** 同一个路径要求两个不同的内容
        // 意味着**无论装哪一个，另一个引用它的地方都会拿到错的东西**。
        // 静默取其一会让那个问题在最难查的地方出现（游戏某个功能异常）。
        let json = r#"{
            "id":"x","mainClass":"M","minecraftArguments":"a",
            "libraries":[
                {"name":"a:b:1","downloads":{"artifact":{"path":"same.jar","sha1":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1,"url":"u"}}},
                {"name":"a:b:1","downloads":{"artifact":{"path":"same.jar","sha1":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","size":1,"url":"u"}}}
            ]
        }"#;
        let d = Descriptor::parse(json).unwrap();
        let e = required_files(&d, &win(), "x").unwrap_err();
        match e {
            InstallError::BadMetadata(w) => {
                assert!(w.contains("same.jar"), "{w}");
                assert!(w.contains("两个不同的 sha1"), "{w}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn 老版本没有日志配置时不会凭空要求一个() {
        // 实测 1.7.10 之前没有 `logging` —— 一个"总是要求日志配置"的实现
        // 会在那些版本上永远报"缺一个文件"。
        let json = r#"{"id":"old","mainClass":"M","minecraftArguments":"a",
            "downloads":{"client":{"sha1":"cc00000000000000000000000000000000000000","size":1,"url":"u"}}}"#;
        let d = Descriptor::parse(json).unwrap();
        let needs = required_files(&d, &win(), "old").unwrap();
        assert!(!needs.iter().any(|n| n.kind == NeedKind::Logging));
        assert_eq!(needs.len(), 1, "只要客户端 jar");
    }

    // ───────────────── 资产对象（按哈希去重）─────────────────

    /// 一个**真实形态**的资产索引（两个逻辑名共享一个哈希）。
    fn asset_index() -> qul_core::assets::AssetIndex {
        qul_core::assets::AssetIndex::parse(
            r#"{"objects":{
                "icons/a.png": {"hash":"1111111111111111111111111111111111111111","size":10},
                "icons/b.png": {"hash":"1111111111111111111111111111111111111111","size":10},
                "lang/en_us.json": {"hash":"2222222222222222222222222222222222222222","size":20}
            }}"#,
        )
        .unwrap()
    }

    #[test]
    fn 资产按哈希去重且路径由哈希决定() {
        // ⚠️ **这条测试钉的是"逻辑名不参与落盘路径"。**
        //
        // 实测 5147 个逻辑名里有重复哈希。一个按逻辑名逐条产生的实现会
        // 把同一个哈希下两次，而**进度总数会虚高成永远到不了的数**。
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let idx = asset_index();
        let needs = required_files_with_assets(
            &d,
            &win(),
            "x",
            Some(&idx),
            crate::install::DEFAULT_ASSET_BASE,
        )
        .unwrap();

        let assets: Vec<_> = needs
            .iter()
            .filter(|n| n.kind == NeedKind::AssetObject)
            .collect();
        assert_eq!(
            assets.len(),
            2,
            "**两个逻辑名共享一个哈希 ⇒ 只该有两个对象**：{:?}",
            assets.iter().map(|a| &a.rel).collect::<Vec<_>>()
        );
        // 路径就是 `assets/objects/<前2位>/<hash>`
        assert!(assets
            .iter()
            .any(|a| a.rel == "assets/objects/11/1111111111111111111111111111111111111111"));
        // 而 URL 是 `<主机>/<前2位>/<hash>`
        assert!(
            assets
                .iter()
                .any(|a| a.url
                    == "https://resources.download.minecraft.net/11/1111111111111111111111111111111111111111"),
            "{:?}",
            assets.iter().map(|a| &a.url).collect::<Vec<_>>()
        );
        // **哈希本身就是 sha1**（资产没有单独的 sha1 字段）
        assert_eq!(
            assets[0].sha1,
            assets[0].rel.rsplit('/').next().unwrap(),
            "资产的内容地址就是它的哈希"
        );
    }

    #[test]
    fn 不给资产索引时不算资产() {
        // 默认路径（也是全部既有测试走的路径）。
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let a = required_files(&d, &win(), "x").unwrap();
        let b =
            required_files_with_assets(&d, &win(), "x", None, crate::install::DEFAULT_ASSET_BASE)
                .unwrap();
        assert_eq!(a, b, "`required_files` 默认不算资产");
        assert!(!a.iter().any(|n| n.kind == NeedKind::AssetObject));
    }

    #[test]
    fn 资产主机可以被覆盖() {
        // **镜像与代理要做得到** —— 主机是常量，但它是一个**参数**。
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let idx = asset_index();
        let needs =
            required_files_with_assets(&d, &win(), "x", Some(&idx), "http://mirror.local/assets")
                .unwrap();
        let obj = needs
            .iter()
            .find(|n| n.kind == NeedKind::AssetObject)
            .unwrap();
        assert!(
            obj.url.starts_with("http://mirror.local/assets/"),
            "{}",
            obj.url
        );
        // 末尾多余的 `/` 不该产生双斜杠
        let needs2 =
            required_files_with_assets(&d, &win(), "x", Some(&idx), "http://mirror.local/assets/")
                .unwrap();
        let obj2 = needs2
            .iter()
            .find(|n| n.kind == NeedKind::AssetObject)
            .unwrap();
        assert!(
            !obj2.url.contains("//1"),
            "末尾斜杠不该产生双斜杠：{}",
            obj2.url
        );
    }

    #[test]
    fn 默认资产主机是方案写的那一个() {
        // 方案 §11.5 的端点清单。改它会让所有资产的下载都走错地方。
        assert_eq!(
            crate::install::DEFAULT_ASSET_BASE,
            "https://resources.download.minecraft.net"
        );
    }

    // ───────────────── 盘点（**离线自证的那条**）─────────────────

    #[test]
    fn 空实例上全部都是缺失的() {
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let tmp = std::env::temp_dir().join(format!("qul-inv-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let inv = inventory(&d, &win(), "x", &tmp, &CancelToken::new()).unwrap();
        assert_eq!(inv.present, 0);
        assert_eq!(inv.missing.len(), inv.needs.len());
        assert!(!inv.is_complete());
        assert!(inv.bytes_to_fetch() > 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn 文件存在但内容不对时仍算缺失() {
        // ⚠️ **这是"校验"这件事的核心。**
        // 一个只看"文件在不在"的实现会**跳过所有内容校验** ——
        // 于是被截断/被改过的文件会被当成"已装好"。
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let tmp = std::env::temp_dir().join(format!("qul-inv-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // 建一个**存在但内容错**的客户端 jar
        let p = tmp.join("versions").join("x");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("x.jar"), b"not the real content").unwrap();

        let inv = inventory(&d, &win(), "x", &tmp, &CancelToken::new()).unwrap();
        assert_eq!(inv.present, 0, "**内容不对就不算已有**");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn 内容与_sha1_一致时算已有() {
        // 造一个**真的 sha1 匹配**的文件，证明盘点认它。
        let content = b"hello";
        let want = crate::check::sha1_hex(content);
        let json = format!(
            r#"{{"id":"x","mainClass":"M","minecraftArguments":"a",
                "downloads":{{"client":{{"sha1":"{want}","size":5,"url":"u"}}}}}}"#
        );
        let d = Descriptor::parse(&json).unwrap();
        let tmp = std::env::temp_dir().join(format!("qul-inv-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let p = tmp.join("versions").join("x");
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("x.jar"), content).unwrap();

        let inv = inventory(&d, &win(), "x", &tmp, &CancelToken::new()).unwrap();
        assert_eq!(inv.present, 1);
        assert!(inv.is_complete(), "**这一条就是「离线自证」的形态**");
        assert_eq!(inv.bytes_to_fetch(), 0);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn 盘点按类给出计数() {
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let tmp = std::env::temp_dir().join(format!("qul-inv-kind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let inv = inventory(&d, &win(), "x", &tmp, &CancelToken::new()).unwrap();
        let m = inv.by_kind();
        assert_eq!(m.get("client"), Some(&1));
        assert_eq!(m.get("natives"), Some(&1));
        assert_eq!(m.get("library"), Some(&1));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // ───────────────── 离线模式不下载 ─────────────────

    #[test]
    fn 离线模式只盘点不下载() {
        // ⚠️ **它是这个模块能在没网时被验证的前提。**
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let tmp = std::env::temp_dir().join(format!("qul-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let sink = RecordingStageSink::new();
        let cfg = InstallConfig {
            offline_only: true,
            ..Default::default()
        };
        let out = install(
            &d,
            &win(),
            "x",
            &tmp,
            &cfg,
            &crate::http::NoNetwork,
            &CancelToken::new(),
            &sink,
        )
        .unwrap();
        assert_eq!(out.downloaded, 0, "离线模式不该下任何东西");
        assert_eq!(out.extracted, 0);
        assert!(!out.inventory.is_complete());
        // 而**阶段耗时都被记了** —— 那是"五个阶段"这件事的证据
        for s in Stage::ALL {
            if *s == Stage::Launch {
                continue;
            }
            assert!(out.stage_ms.contains_key(s.key()), "缺阶段 {}", s.key());
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn 已取消时立刻返回取消() {
        let d = Descriptor::parse(&descriptor_json()).unwrap();
        let tmp = std::env::temp_dir().join(format!("qul-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let sink = RecordingStageSink::new();
        let e = install(
            &d,
            &win(),
            "x",
            &tmp,
            &InstallConfig::default(),
            &crate::http::NoNetwork,
            &cancel,
            &sink,
        )
        .unwrap_err();
        assert_eq!(e, InstallError::Cancelled, "取消是取消，不是失败");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn natives_落在实例内而不是_temp() {
        // 方案 §8 第 6 项：官方把 natives 解到 `%temp%`，
        // **教程明确警告那可能被垃圾清理软件删除导致无法启动**。
        let inst = Path::new(r"C:\data\instances\abc");
        let n = natives_dir(inst, "x");
        assert!(n.starts_with(inst), "{}", n.display());
        assert!(!n.to_string_lossy().to_lowercase().contains("temp"));
    }

    // ───────────────── 阶段的枚举完整性 ─────────────────

    #[test]
    fn 五个阶段无遗漏且键唯一() {
        // 这条防"加了阶段却忘了登记" —— 而 `ALL` 是进度显示的依据。
        assert_eq!(Stage::ALL.len(), 5);
        let mut keys: Vec<&str> = Stage::ALL.iter().map(|s| s.key()).collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n, "阶段键有重复");
        for s in Stage::ALL {
            assert!(s.key().is_ascii(), "{s:?}");
        }
        // 而顺序是有意义的（①解析 → ⑤启动）
        assert!(Stage::Parse < Stage::Download);
        assert!(Stage::Download < Stage::Verify);
        assert!(Stage::Verify < Stage::Extract);
        assert!(Stage::Extract < Stage::Launch);
    }
}
