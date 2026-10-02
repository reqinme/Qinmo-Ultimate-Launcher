//! # 安装编排（**界面与 CLI 共用的那一份**）
//!
//! ## 🔴 这个模块为什么存在：一次"撞墙"的产物
//!
//! 原本这份编排住在 `qul-cli/src/install_cmd.rs` 里。而在 M4 要把它接到
//! Tauri 命令层时撞到了一面墙：
//!
//! > Tauri 命令层的定义是**薄壳**（方案 §3.5），而 `install()` 要的三样东西
//! > ——**版本详情从哪来**、**用哪个 transport**、**要不要装资产**——全是**决策**。
//!
//! 于是二选一：
//!
//! | 做法 | 后果 |
//! |---|---|
//! | 把 CLI 里那份**复制**进命令层 | 两份会漂移的"先缓存再联网"策略 |
//! | **把它搬到这里** | 两个调用方共享同一份 |
//!
//! **第二条是唯一不制造分叉的那个。** 而这正是 `qul-app` 文档里那句
//! *"编排层是'界面与命令行共用的那一份逻辑'的落点"* 的字面要求。
//!
//! ## 它内含的四条策略
//!
//! | # | 策略 | 为什么它是策略而不是细节 |
//! |---|---|---|
//! | 1 | **先本机缓存，再联网** | 它决定"离线能不能用"，而那是产品行为 |
//! | 2 | **transport 由调用方给** | 那让"离线模式"成为**一个参数**而不是一份分支 |
//! | 3 | **要不要装资产** | 实测 461 MB / 5147 个对象 —— 它是一次真实的取舍 |
//! | 4 | **失败要走同一个 sink** | 见下 |
//!
//! ## ⚠️ 第 4 条值得单独说
//!
//! 一个"失败就 `return Err`"的实现会让界面**停在最后那个进度上** ——
//! 因为 §7.2 的 `Error` 是**灵动岛的一个状态**，而"失败"该出现的位置是**岛上**。
//!
//! 所以这里的形状是：**失败也经 `sink` 报一次**，然后才返回 `Err`。
//! 于是调用方（CLI / 命令层）拿到的 `Err` 是**给日志与退出码的**，
//! 而用户看到的那一句来自 `sink`。

use qul_core::descriptor::{Descriptor, Env};
use qul_core::http::Transport;
use qul_core::retry::CancelToken;

// 让调用方**不必直接 import `qul_infra`**：它们从这一层拿到这些东西。
//
// ⚠️ 这不是"包装" —— 它让"编排层是这一类事情的唯一入口"在**源码上**成立：
// CLI 与命令层写的是 `qul_app::install_plan::StageSink`，而不是
// `qul_infra::install::StageSink`。
pub use qul_infra::install::{
    install, InstallConfig, InstallError, InstallOutcome, Inventory, Stage, StageSink,
};

/// 一次安装编排的全部输入。
///
/// ⚠️ **它是 `pub struct`，而 `tests/layering.rs` 断言编排层只暴露 `AppService`。**
/// 所以这里**不新增结构体** —— 参数用一个元组式的显式列表（见
/// [`install_plan`]），而那让那条断言继续成立。
///
/// 而"参数有六个"这件事本身是可接受的：它们**每一个都是调用方必须决定的**，
/// 而打包成一个结构体只是把决定藏进一个名字里。
///
/// ## 而它为什么是一个函数而不是 `AppService` 的方法
///
/// 因为 `AppService` 是 `Clone + Default` 的（M0 的链路样板依赖那一点），
/// 而"一次安装"是有状态的、不可克隆的、一次性的。
/// 把它挂在一个 `Default` 的服务上会让"我 new 了一个服务就能装东西"变成
/// 一个不真的印象。
///
/// # 参数
///
/// | 参数 | 谁决定 |
/// |---|---|
/// | `version_id` | 调用方（用户选了哪个版本） |
/// | `instance_root` | 调用方（哪个实例） |
/// | `cfg` | 调用方（要不要装资产、并发多少） |
/// | `transport` | **调用方** —— 传 `NoNetwork` 就是离线模式（策略 2） |
/// | `cancel` | 调用方（用户在界面上点了取消） |
/// | `sink` | 调用方（进度去哪） |
///
/// # 返回
///
/// `Ok(InstallOutcome)` ⇒ 走到哪一步、下了几个、缺几个。
/// `Err(InstallError)` ⇒ **而失败已经经 `sink` 报过一次**（策略 4）。
#[allow(clippy::too_many_arguments)]
pub fn install_plan(
    descriptor: &Descriptor,
    env: &Env,
    version_id: &str,
    instance_root: &std::path::Path,
    cfg: &InstallConfig,
    transport: &dyn Transport,
    cancel: &CancelToken,
    sink: &dyn StageSink,
) -> Result<InstallOutcome, InstallError> {
    // ⚠️ **它现在只是转接，而那个转接是有意的。**
    //
    // 一个"直接把 `qul_infra::install::install` 暴露出去"的做法会让
    // 调用方**绕过这一层** —— 而那时"策略将来要变"（例如加一条"先试镜像"）
    // 就要改所有调用点。
    //
    // 而**转接本身没有内容**这件事是诚实的：这一层的价值在
    // "版本详情从哪来"那一条（见 [`descriptor_for`]），而不是在这一行。
    install(
        descriptor,
        env,
        version_id,
        instance_root,
        cfg,
        transport,
        cancel,
        sink,
    )
}

/// **取版本详情：先本机缓存，再联网。**（策略 1）
///
/// ## 🔴 它为什么在编排层，而不是在 CLI 里
///
/// 因为**界面也要它**。原版在 `install_cmd.rs` 里，而一个"界面也照抄一遍"
/// 的做法会让两条路在"缓存没有时怎么办"上**必然漂移**。
///
/// ## 它内含的判断
///
/// | 情况 | 它做什么 | 为什么 |
/// |---|---|---|
/// | 本机缓存里有 | **用它，零网络** | 那是官方启动器写下的，而它**权威且离线可用** |
/// | 缓存没有 + `allow_network` | 联网取清单与详情 | — |
/// | 缓存没有 + 不许联网 | **明确报错，并说清怎么办** | 不猜、不静默失败 |
///
/// # 参数
///
/// - `cache_root`：本机那份 `.minecraft` 的位置（**由调用方给** ——
///   编排层不读 `%APPDATA%`，那是一条让它不可测的环境依赖）
/// - `allow_network`：`false` 就是"不许联网"
/// - `manifest_url`：清单的 URL（**由调用方给** —— 方案 §11.5 的纪律是
///   "所有 URL 取自元数据"，而清单 URL 是那个纪律的**唯一例外**：
///   它是入口。**而那个例外由调用方持有，不由编排层硬编码。**）
pub fn descriptor_for(
    version_id: &str,
    cache_root: &std::path::Path,
    transport: &dyn Transport,
    allow_network: bool,
    manifest_url: &str,
) -> Result<(Descriptor, String), String> {
    // ① 本机缓存（官方启动器写下的那份）
    let cached = cache_root
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"));
    if cached.is_file() {
        let text = std::fs::read_to_string(&cached).map_err(|e| format!("读不到缓存：{e}"))?;
        let d = Descriptor::parse(&text).map_err(|e| format!("缓存那份解析失败：{e}"))?;
        return Ok((d, format!("本机缓存 {}", cached.display())));
    }

    if !allow_network {
        return Err(format!(
            "本机没有 `{version_id}` 的详情（{}），而 `--offline` 不许联网。\n  \
             要拿到它，用官方启动器启动一次那个版本，或去掉 `--offline`。",
            cached.display()
        ));
    }

    // ② 联网：先取清单，再按 url 取详情
    let m = transport
        .fetch(&qul_core::http::FetchRequest::get(manifest_url))
        .map_err(|e| format!("取清单失败：{e}"))?;
    if !m.is_success() {
        return Err(format!("取清单得到状态码 {}", m.status));
    }
    let text = String::from_utf8_lossy(&m.body).to_string();
    let manifest = qul_core::descriptor::VersionManifest::parse(&text)
        .map_err(|e| format!("清单解析失败：{e}"))?;
    let entry = manifest
        .find(version_id)
        .ok_or_else(|| format!("清单里没有 `{version_id}`"))?;
    let r = transport
        .fetch(&qul_core::http::FetchRequest::get(&entry.url))
        .map_err(|e| format!("取详情失败：{e}"))?;
    if !r.is_success() {
        return Err(format!("取详情得到状态码 {}", r.status));
    }
    let t2 = String::from_utf8_lossy(&r.body).to_string();
    let d = Descriptor::parse(&t2).map_err(|e| format!("详情解析失败：{e}"))?;
    Ok((
        d,
        format!(
            "联网 {}（清单声明的 sha1 {}）",
            entry.url,
            &entry.sha1[..8.min(entry.sha1.len())]
        ),
    ))
}

/// **取资产索引**（策略 3 的**前半**）。
///
/// ## 🔴 它为什么在编排层：**"要不要装资产"是一个决定，而它有四个分支**
///
/// | 情况 | 它做什么 |
/// |---|---|
/// | 这个版本没有资产索引 | `Ok(None)` —— **老版本用 `assets` 字符串**，那不是错误 |
/// | 本机已有那份索引 | 零网络，直接读 |
/// | 本机没有 + 不许联网 | `Ok(None)` —— **跳过资产**，而不是失败 |
/// | 本机没有 + 可以联网 | 按元数据的 URL 与 sha1 下它 |
///
/// ⚠️ **第三条是 `Ok(None)` 而不是 `Err`** —— 而那是一个**产品判断**：
/// "离线时没有资产"是**一个可接受的状态**（游戏能起，只是没声音没语言），
/// 而不是一次失败。
///
/// ## 而"要先拿到那个文件才能解析它"这件事决定了它的形状
///
/// 资产索引**自己也是一个要下载的文件**，而它的 URL 与 sha1 在版本详情里。
/// 所以顺序不能反：**先把它下下来，再解析它**。
/// 一个"先解析索引再下载"的实现会陷入循环 ——
/// 而那正是这里的返回值是**解析结果**（而不是"要不要下"）的原因。
///
/// ## 返回的那个三元组
///
/// `(逻辑名数, 不同哈希数, 去重后字节数)` —— 三个都要，因为
/// **"逻辑名数 ≠ 文件数"**这个实测事实只能靠前两个数一起表达
///（`26.3` 上它们恰好相等，而协议允许不等）。
pub fn ensure_asset_index(
    descriptor: &Descriptor,
    instance_root: &std::path::Path,
    transport: &dyn Transport,
    allow_network: bool,
    cancel: &CancelToken,
) -> Result<Option<(usize, usize, u64)>, String> {
    let Some(ai) = descriptor.asset_index_ref() else {
        // 老版本用 `assets` 字符串 —— **那不是错误**。
        return Ok(None);
    };

    let dest = instance_root
        .join("assets")
        .join("indexes")
        .join(format!("{}.json", ai.id));

    if !dest.is_file() {
        if !allow_network {
            // ⚠️ **`Ok(None)` 而不是 `Err`** —— 见上面那段。
            return Ok(None);
        }
        let v = qul_infra::check::Sha1Verifier::new(&ai.sha1)
            .ok_or_else(|| "资产索引的 sha1 形态不对".to_string())?;
        match qul_infra::download::download(
            transport,
            &ai.url,
            &dest,
            &v,
            &qul_infra::download::DownloadConfig::default(),
            cancel,
            None,
        ) {
            qul_infra::download::DownloadOutcome::Done { .. } => {}
            other => return Err(format!("取资产索引失败：{other:?}")),
        }
    }

    let text = std::fs::read_to_string(&dest).map_err(|e| format!("读资产索引失败：{e}"))?;
    let idx =
        qul_core::assets::AssetIndex::parse(&text).map_err(|e| format!("资产索引解析失败：{e}"))?;
    Ok(Some((
        idx.objects.len(),
        idx.unique_hashes().len(),
        idx.unique_bytes(),
    )))
}

/// **进度去掉噪的一个包装。**
///
/// ## 它解决什么
///
/// 下载每 100 ms 报一次进度（§7.3 约束 5 的节流是**前端**那一侧的事），
/// 而一个把每条都往日志里写的 CLI 会刷出几千行。
///
/// 所以这个包装**只在整百分比变化时**转发 `files` —— 而 `stage` 一条不漏
///（阶段只有五条，而每一条都有信息）。
///
/// ⚠️ **它不改语义**：被跳过的那些是**同一条进度的更细粒度**，
/// 而不是"别的事件"。而"整百分比"这个判据与前端那个节流是**两个独立的口径**
///（前端的判据是"≥100 ms **或** ≥1%"，见 `web/src/island/islandMath.ts`），
/// 而这是对的：CLI 要的是**不刷屏**，界面要的是**不丢帧**。
pub struct ThrottledSink<'a> {
    inner: &'a dyn StageSink,
    last_pct: std::cell::Cell<i32>,
}

impl<'a> ThrottledSink<'a> {
    pub fn wrap(inner: &'a dyn StageSink) -> Self {
        Self {
            inner,
            // ⚠️ **初值是 -1 而不是 0** —— 否则"第一次就落在 0%"会被吞掉，
            // 而那正是用户最需要看到"它开始了"的那一条。
            //
            // 这正是方案 §1.1.5 那条纪律的一个实例：
            // **不要用 0 当"还没设过"的哨兵。**
            last_pct: std::cell::Cell::new(-1),
        }
    }
}

impl StageSink for ThrottledSink<'_> {
    fn stage(&self, stage: Stage, message: &str) {
        // 阶段**一条不漏** —— 只有五条，而每一条都有信息。
        self.inner.stage(stage, message);
    }

    fn files(&self, done: usize, total: usize, current: &str) {
        // ⚠️ `checked_div` 而不是手写 `if total == 0 { 0 } else { … }` ——
        // clippy 的 `manual_checked_ops` 会拦后者，而**它是对的**：
        // 手写那版在 `done * 100` 上还可能溢出，而 `checked_div`
        // 把"除数非零"表达成一个类型上的事实。
        //
        // （同一个错我在 `install_cmd.rs` 里犯过一次 —— 而这个模式
        // 说明它值得写成一句注释，而不是靠下次记得。）
        let pct = done.saturating_mul(100).checked_div(total).unwrap_or(0) as i32;
        if pct != self.last_pct.get() {
            self.last_pct.set(pct);
            self.inner.files(done, total, current);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 一个把收到的东西记下来的 sink。
    #[derive(Default)]
    struct Rec {
        stages: Mutex<Vec<String>>,
        files: Mutex<Vec<(usize, usize)>>,
    }

    impl StageSink for Rec {
        fn stage(&self, stage: Stage, message: &str) {
            self.stages
                .lock()
                .expect("锁没被毒化")
                .push(format!("{}: {message}", stage.key()));
        }
        fn files(&self, done: usize, total: usize, _current: &str) {
            self.files.lock().expect("锁没被毒化").push((done, total));
        }
    }

    #[test]
    fn 节流只在整百分比变化时转发文件进度() {
        let rec = Rec::default();
        let t = ThrottledSink::wrap(&rec);
        // 100 个文件，逐条都报 —— 而只有 1% 的台阶该被转发
        for i in 0..=100 {
            t.files(i, 100, "x");
        }
        let got = rec.files.lock().expect("锁没被毒化").clone();
        // 0..=100 每个整数都恰好是一个百分比台阶 ⇒ 101 条
        assert_eq!(got.len(), 101, "整百分比有 101 个台阶");
    }

    #[test]
    fn 百分号以下的抖动被吞掉() {
        let rec = Rec::default();
        let t = ThrottledSink::wrap(&rec);
        // 10000 个文件里，前 50 个都在 0% 那一档
        for i in 0..50 {
            t.files(i, 10_000, "x");
        }
        assert_eq!(
            rec.files.lock().expect("锁没被毒化").len(),
            1,
            "只有 0% 那一条"
        );
    }

    #[test]
    fn 第一次报的_0_不会被吞掉() {
        // ⚠️ 这条钉的是那个 `-1` 初值。
        // 一个用 `0` 当初值的实现会把第一条（`done = 0`）当成"没变"而丢掉 ——
        // 而那正是用户最需要看到"它开始了"的那一条。
        let rec = Rec::default();
        let t = ThrottledSink::wrap(&rec);
        t.files(0, 100, "x");
        assert_eq!(rec.files.lock().expect("锁没被毒化").len(), 1);
    }

    #[test]
    fn 阶段一条不漏() {
        let rec = Rec::default();
        let t = ThrottledSink::wrap(&rec);
        for s in Stage::ALL {
            t.stage(*s, "m");
        }
        assert_eq!(rec.stages.lock().expect("锁没被毒化").len(), 5);
    }

    #[test]
    fn 总量为零时不除零() {
        let rec = Rec::default();
        let t = ThrottledSink::wrap(&rec);
        t.files(0, 0, "x");
        assert_eq!(rec.files.lock().expect("锁没被毒化").len(), 1);
    }

    // ───────────────── `descriptor_for` 的策略 ─────────────────

    #[test]
    fn 缓存里有就走本机而一个字节都不联网() {
        let tmp = std::env::temp_dir().join(format!("qul-app-desc-{}", std::process::id()));
        let vdir = tmp.join("versions").join("x1");
        std::fs::create_dir_all(&vdir).expect("能建目录");
        // 一份**最小的**合法详情
        let json = r#"{"id":"x1","mainClass":"a.B","type":"release","libraries":[]}"#;
        std::fs::write(vdir.join("x1.json"), json).expect("能写");

        // 传一个**会 panic 的** transport：若它被调用，测试就红
        struct Boom;
        impl Transport for Boom {
            fn fetch(
                &self,
                _r: &qul_core::http::FetchRequest,
            ) -> Result<qul_core::http::FetchResponse, String> {
                panic!("缓存命中时不该联网");
            }
        }
        let (d, src) = descriptor_for("x1", &tmp, &Boom, true, "https://example.invalid/m.json")
            .expect("缓存命中应当成功");
        assert_eq!(d.id, "x1");
        assert!(src.contains("本机缓存"), "来源该说清是缓存：{src}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn 缓存没有且不许联网时报错_而消息说清怎么办() {
        let tmp = std::env::temp_dir().join(format!("qul-app-none-{}", std::process::id()));
        struct Boom;
        impl Transport for Boom {
            fn fetch(
                &self,
                _r: &qul_core::http::FetchRequest,
            ) -> Result<qul_core::http::FetchResponse, String> {
                panic!("不许联网时不该联网");
            }
        }
        let e = descriptor_for("nope", &tmp, &Boom, false, "https://example.invalid/m.json")
            .expect_err("该失败");
        // ⚠️ **消息必须说清"怎么办"** —— 一个只说"没有详情"的实现
        // 会让用户不知道下一步做什么。
        assert!(e.contains("不许联网"), "{e}");
        assert!(e.contains("官方启动器"), "该给出路：{e}");
    }
    // ───────────────── `ensure_asset_index` 的四个分支 ─────────────────

    /// 一个**会 panic 的** transport：若它被调用，测试就红。
    struct Boom;
    impl Transport for Boom {
        fn fetch(
            &self,
            _r: &qul_core::http::FetchRequest,
        ) -> Result<qul_core::http::FetchResponse, String> {
            panic!("这个分支不该联网");
        }
    }

    fn desc_with_assets(id: &str) -> Descriptor {
        let json = format!(
            r#"{{"id":"{id}","mainClass":"a.B","type":"release","libraries":[],
                 "assetIndex":{{"id":"34","sha1":"{}","url":"https://e.invalid/34.json"}}}}"#,
            "a".repeat(40)
        );
        Descriptor::parse(&json).expect("能解析")
    }

    #[test]
    fn 没有资产索引的版本返回空而不是错误() {
        // ⚠️ 老版本用 `assets` 字符串（一个索引名），**而那不是错误**。
        // 一个把它当成失败的实现会让"装老版本"直接红。
        let d =
            Descriptor::parse(r#"{"id":"x","mainClass":"a.B","libraries":[]}"#).expect("能解析");
        let tmp = std::env::temp_dir().join(format!("qul-ai-none-{}", std::process::id()));
        let r = ensure_asset_index(&d, &tmp, &Boom, false, &CancelToken::new());
        assert_eq!(r.expect("不该失败"), None);
    }

    #[test]
    fn 本机没有索引且不许联网时跳过而不失败() {
        // ⚠️ **这是一条产品判断，而不是一个容错。**
        // "离线时没有资产"是**一个可接受的状态**：游戏能起，只是没声音没语言。
        //
        // 而这条与上一条的区别值得说清：上一条是"这个版本**没有**索引"，
        // 这一条是"这个版本有索引，而**我们拿不到**"。两者都返回 `None`
        // —— 而它们的理由不同，所以这里是两条测试。
        let d = desc_with_assets("x2");
        let tmp = std::env::temp_dir().join(format!("qul-ai-off-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let r = ensure_asset_index(&d, &tmp, &Boom, false, &CancelToken::new());
        assert_eq!(r.expect("离线时该跳过而不是失败"), None);
    }
}
