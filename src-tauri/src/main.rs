//! # Tauri 命令层（**薄壳**）
//!
//! ## 它是什么，以及它**不是**什么
//!
//! 方案 §3.5：*"Tauri 命令层（**薄壳**）+ 界面外壳"*。
//!
//! | 它做 | 它不做 |
//! |---|---|
//! | 把前端的请求翻译成内核调用 | **任何业务判断**（那属于 `qul-core` / Provider） |
//! | 把内核的结论序列化成契约 | 任何文件路径拼接（那属于 `qul-infra` 的 `fsx`） |
//! | **能力投影**（决定界面能调什么） | 任何产品知识（§1.4：界面不认识具体游戏） |
//!
//! ## 🔴 三条纪律，逐条落在这里
//!
//! ### ① 命令**必须返回可序列化的结论**，而不是"让前端自己判断"
//!
//! 方案 §3.3 的能力描述符：界面拿到的不是"该不该显示"的**判断依据**，
//! 而是**一组已经算好的结论**（`{ enabled, reason }`）。
//! 所以 `capabilities` 这条命令返回的是**结论**。
//!
//! ### ② 没有通用 fs / shell / http 插件（§5.8 第 2 条）
//!
//! 所以这里**一条通用文件操作的命令都没有** —— 将来加"打开实例目录"这类功能时，
//! 那条命令要在**内部**走 `qul_infra` 的 `fsx::ensure_within`（它拒绝越界路径），
//! 而不是把 `tauri_plugin_fs` 挂上让前端自己拼路径。
//!
//! ### ③ 窗口材质走 `window-vibrancy`，而不是 CSS
//!
//! 方案 §3.4 的降级链（Mica → Acrylic → Tabbed → 纯色）**必须在原生窗口上做** ——
//! 而 §4.5 那条实测约束写着：*"Windows WebView2 上**窗口透明与
//! `backdrop-filter` 不能共存**"*。所以材质是**原生**的，而界面层零 CSS 模糊。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use qul_core::island::{IslandState, LaunchStage};
use qul_core::retry::CancelToken;
use qul_app::install_plan::{
    self, default_data_root, instance_dir, InstallError, InstallOutcome,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::ipc::Channel;

/// 一个能力的结论。
///
/// ⚠️ **`reason` 在 `enabled == false` 时必定存在** —— 这是与前端
/// `web/src/api/contract.ts` 的契约，而那个契约在**两侧**都被校验
/// （Rust 的类型系统 + 前端的 `assertCapabilitiesValid`）。
#[derive(Serialize)]
struct Capability {
    enabled: bool,
    /// **面向用户的原因**（不是错误码）。禁用态缺它 = 界面只能显示一句"不可用"。
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

fn on() -> Capability {
    Capability {
        enabled: true,
        reason: None,
    }
}

fn off(reason: &str) -> Capability {
    Capability {
        enabled: false,
        reason: Some(reason.to_owned()),
    }
}

/// 取能力表。
///
/// ## ⚠️ 它现在是**骨架**
///
/// 真实的能力要由 Provider 决定（§3.3：*"产品差异由后端表达"*），
/// 而 M5 的微软身份与 M9 的基岩版还没到。
///
/// 所以下面这一份是**当前形态的最小事实** —— 而它的**形状**（`{enabled, reason}`）
/// 已经是对的，于是接上 Provider 时只换这个函数体。
///
/// **而它刻意保留了真实的禁用态**（带原因）：一个"全部 enabled: true"的骨架
/// 会让"禁用必须带原因"这条规则**在开发时看不到**。
#[tauri::command]
fn capabilities() -> BTreeMap<String, Capability> {
    let mut m = BTreeMap::new();
    m.insert("launch".to_owned(), on());
    m.insert("preflight".to_owned(), on());
    m.insert("mods".to_owned(), on());
    m.insert("worlds".to_owned(), on());
    m.insert("configs".to_owned(), on());
    m.insert("crash_analysis".to_owned(), on());
    m.insert("log_filtering".to_owned(), on());
    m.insert("offline_play".to_owned(), on());
    // ↓ 两条真实的禁用态：原因**面向用户**，而不是错误码。
    m.insert("shaders".to_owned(), off("该形态不支持光影"));
    m.insert(
        "isolation".to_owned(),
        off("该形态无法隔离实例，与官方启动器共用账户与数据"),
    );
    m
}

/// 取一个实例的摘要。
///
/// ⚠️ **它现在只回显 id** —— 真实的实例记录要 M6。
/// 而**它的签名是对的**：`instance_id: String`（内核的实例标识，
/// 而不是路径 —— 路径属于实例记录内部）。
#[tauri::command]
fn instance_summary(instance_id: String) -> InstanceSummary {
    InstanceSummary {
        id: instance_id.clone(),
        name: instance_id,
    }
}

#[derive(Serialize)]
struct InstanceSummary {
    id: String,
    name: String,
}

/// **把内核的进度翻译成灵动岛的状态**（`StageSink` → `Channel<IslandState>`）。
///
/// ## 🔴 而这个结构体是"薄壳"这个词最具体的一次体现
///
/// 它**只做翻译**：
///
/// | 它做 | 它不做 |
/// |---|---|
/// | 把 `Stage` 映射成 `LaunchStage` | 判断"该不该进入某个阶段" |
/// | 把每条进度 `send` 出去 | 节流（那是 [`install_plan::ThrottledSink`] 的事） |
/// | 记住当前阶段（用于 `files` 那条） | 算百分比（那是内核的事） |
///
/// ## ⚠️ 而 `files` 那一条**必须自己知道当前阶段**
///
/// 因为 `StageSink` 的两个方法是**分开**的：`stage()` 说"到哪一步了"，
/// 而 `files()` 只说"下了几个"。于是"下载进度"这条消息**需要把两者拼起来**
/// —— 而拼的工作只能在这里做（内核不该知道界面要什么形状）。
///
/// 一个"`files` 时也发 `Launch { stage: Download }`"的实现会在**校验**阶段里
/// 显示"下载中" —— 而那是**错的阶段**，且它看起来像下载没结束。
struct ChannelSink {
    ch: Channel<IslandState>,
    /// 当前阶段。`files` 那条消息需要它。
    ///
    /// ⚠️ 用 `Mutex` 而不是 `Cell`：`StageSink` 的方法签名是 `&self`，
    /// 而 `ChannelSink` 需要在多线程下被 `&` 共享（下载线程会调它）。
    stage: std::sync::Mutex<LaunchStage>,
}

impl ChannelSink {
    fn new(ch: Channel<IslandState>) -> Self {
        Self {
            ch,
            stage: std::sync::Mutex::new(LaunchStage::Parse),
        }
    }
}

/// `qul_infra::install::Stage` → `qul_core::island::LaunchStage`。
///
/// ⚠️ **而这是一次恒等映射** —— 两侧的五个变体名逐字相同
///（`parse` / `download` / `verify` / `extract` / `launch`）。
///
/// 那件事**不是巧合**：内核的 `LaunchStage` 就是照 `install` 的五阶段定的。
/// 而这里仍然写成一个**函数**而不是 `as` 转换，因为：
/// ① 两侧是**不同的类型**（一个在 `qul-infra`，一个在 `qul-core`）；
/// ② 哪天某一侧加了一个阶段，这里会**编译失败** —— 而那正是我们想要的。
fn to_launch_stage(s: install_plan::Stage) -> LaunchStage {
    match s {
        install_plan::Stage::Parse => LaunchStage::Parse,
        install_plan::Stage::Download => LaunchStage::Download,
        install_plan::Stage::Verify => LaunchStage::Verify,
        install_plan::Stage::Extract => LaunchStage::Extract,
        install_plan::Stage::Launch => LaunchStage::Launch,
    }
}

impl install_plan::StageSink for ChannelSink {
    fn stage(&self, stage: install_plan::Stage, _message: &str) {
        let ls = to_launch_stage(stage);
        if let Ok(mut g) = self.stage.lock() {
            *g = ls;
        }
        // ⚠️ **发送失败是静默的** —— 而这是对的。
        //
        // "发送失败"只意味着一件事：**前端已经不在了**（窗口关了）。
        // 而那时安装**不该**中断：它是用户明确要的，而"关掉窗口"
        // 不等于"取消安装"（§7.3 约束 4：**可关闭但任务不丢失**）。
        //
        // 真正的取消只走 `cancel_install` 那条命令 —— 它调内核的 `CancelToken`。
        let _ = self.ch.send(IslandState::Launch { stage: ls });
    }

    fn files(&self, done: usize, total: usize, _current: &str) {
        // ⚠️ 只有**下载**阶段才把"下了几个"翻成岛的进度。
        //
        // 在别的阶段里，`files` 的含义不同（校验时它是"校验了几个"，
        // 解压时是"解压了几个"），而把那些都显示成"下载 x/y"
        // 会让用户在**校验**时以为还在下载。
        let stage = self
            .stage
            .lock()
            .map(|g| *g)
            .unwrap_or(LaunchStage::Parse);
        if stage != LaunchStage::Download {
            return;
        }
        let _ = self.ch.send(IslandState::Download {
            done_bytes: done as u64,
            total_bytes: total as u64,
            // ⚠️ **`None` 而不是 `0`。**
            //
            // `speed_bps` / `eta_secs` 是 `Option`，而它们的理由写在
            // `qul-core/src/island.rs`：**速度要在有足够样本之后才算得出来**。
            // 而这里给它 `None` 是**诚实**的 —— 命令层不测速
            //（那一层没有那个信息，而编一个 0 会让界面显示"0 B/s"，
            // 而那与"下载卡住了"在视觉上无法区分）。
            speed_bps: None,
            eta_secs: None,
        });
    }
}

/// **开始安装一个版本，并把进度流给前端。**
///
/// ## 🔴 它用 `Channel<IslandState>` 而**不是**全局事件
///
/// | | 全局事件（`app.emit`） | **`Channel`** |
/// |---|---|---|
/// | 作用域 | **所有窗口** | **这一次调用的这一个前端** |
/// | 多个安装同时跑 | 事件会**混在一起** | 各自一条流 |
///
/// §7.1 的硬规则是「**任意时刻只有一个岛、一个主状态**」——
/// 而"两个安装的进度混在一条全局事件流里"会让那条规则**在消息层就被破坏**。
///
/// ## 而它**不阻塞 UI**
///
/// 安装是同步的重活（下载 + 校验 + 解压），所以它跑在阻塞线程池上 ——
/// 于是界面在装的时候照旧能响应（灵动岛的动画、取消按钮）。
///
/// ## 而**失败也走同一个 channel**
///
/// 一个"失败就 `Err`"的实现会让岛**停在最后那个进度上** ——
/// 因为 §7.2 的 `Error` 是**灵动岛的一个状态**，而"失败"该出现的位置是**岛上**。
/// 所以这里先 `send(Error { .. })`，然后才返回 `Err`（那个 `Err` 是给退出码与日志的）。
#[tauri::command]
async fn install(
    version_id: String,
    on_event: Channel<IslandState>,
    running: tauri::State<'_, Arc<Running>>,
) -> Result<InstallSummary, String> {
    let cancel = Arc::new(CancelToken::new());
    running.cancel.with(|c| *c = Some(Arc::clone(&cancel)));

    // 第一条：让岛**立刻**从"无事发生"变成"在做某事"。
    //
    // ⚠️ 这不是装饰 —— 一个"等第一份进度才推第一条"的实现会让岛
    // 在**最不确定的那几秒**里显示空闲，而那正是用户在看它的时候。
    let _ = on_event.send(IslandState::Launch {
        stage: LaunchStage::Parse,
    });

    let sink = ChannelSink::new(on_event.clone());
    let vid = version_id.clone();

    let result = tauri::async_runtime::spawn_blocking(move || -> Result<InstallOutcome, String> {
        let data_root = default_data_root();
        let inst = instance_dir(&data_root, &vid);
        // ⚠️ `Env::new(PlatformTarget)` 而**不是** `Env::detect()` ——
        // 后者不存在。而"当前平台是什么"由 `PlatformTarget` 决定，
        // 于是它是一次**显式的构造**，而不是一次隐式的环境探测。
        // 那让"在 Windows 上模拟 Linux"成为一个**参数**（M9 会用到）。
        // ⚠️ **`PlatformTarget::windows(version, arch)` 要两个参数，而它们是
        // 元数据里的 `rules` 会去问的东西**（§2：`rules` 按 os.version / os.arch 判断）。
        //
        // 所以这里传的是**真实的 Windows 版本与架构**，而**不是常量**：
        //
        // - `arch`：用 `std::env::consts::ARCH` —— 它是**编译目标的架构**，
        //   而那正是这个二进制会跑在什么上（`x86_64` / `aarch64`）。
        // - `version`：⚠️ **这里传 `"10.0"` 是一个已知的简化。**
        //   元数据里的规则会问 `os.version`（例如"只在 10.0 以上放行"），
        //   而"这台机器到底是什么版本"要调 `RtlGetVersion` 之类的东西。
        //   本项目的基线上写的是 Windows 10 1809+，所以 `"10.0"` 是**保守且正确**
        //   的那个取值。而**它该由 `qul-infra` 在一个函数里给出来**
        //   （那是一件 IO 事实，不是编排决策）—— 记在这里，M5 起补。
        let env = qul_core::descriptor::Env::new(qul_core::descriptor::PlatformTarget::windows(
            "10.0",
            std::env::consts::ARCH,
        ));

        // ① 版本详情：**先本机缓存，再联网**（编排层的那条策略）。
        //
        // ⚠️ 这里传的 `cache_root` 是**官方启动器那个** `.minecraft`
        // —— 而它是"别人的事实"，所以它作为一个**参数**出现，
        // 而"我们自己的数据根"由 `default_data_root()` 决定。
        let minecraft = std::path::Path::new(
            &std::env::var("APPDATA").unwrap_or_default(),
        )
        .join(".minecraft");
        // ⚠️ transport 现在恒为 WinHTTP —— 而"离线模式"在 M4 的界面里
        // 还没有开关。而**这不是一个缺口**：`offline` 是一个参数，
        // 接上那个开关时只改这一行。
        let transport = qul_infra::winhttp::WinHttpTransport::new();
        let (descriptor, _src) = install_plan::descriptor_for(
            &vid,
            &minecraft,
            &transport,
            true,
            MANIFEST_URL,
        )?;

        // ② 五阶段流水线。**节流在编排层**（CLI 那边也用它）。
        let throttled = install_plan::ThrottledSink::wrap(&sink);
        let r = install_plan::install_to_instance(
            &descriptor,
            &env,
            &vid,
            &inst,
            false,
            true,
            None,
            &transport,
            &cancel,
            &throttled,
        );
        r.map_err(|e| match e {
            InstallError::Checksum { want, got, .. } => format!(
                "校验失败：期望 {}… 实际 {}…",
                &want[..8.min(want.len())],
                &got[..8.min(got.len())]
            ),
            other => other.to_string(),
        })
    })
    .await
    .map_err(|e| format!("安装线程没能启动：{e}"))?;

    running.cancel.with(|c| *c = None);

    match result {
        Ok(out) => {
            let _ = on_event.send(IslandState::Idle);
            // ⚠️ **三个数都来自那个真实的盘点结果**，而不是编的：
            //   `needs.len()` = 这个版本一共需要几个文件
            //   `present`     = 其中磁盘上已有且 sha1 正确的
            //   `downloaded`  = 这一次真的下了几个
            // 一个只报"总数"的实现会让用户看不出"这次装了什么"。
            Ok(InstallSummary {
                version: version_id,
                needed: out.inventory.needs.len(),
                present: out.inventory.present,
                downloaded: out.downloaded,
                migrated: out.migrated,
            })
        }
        Err(human) => {
            // 🔴 **失败也走同一个 channel** —— 见上面那段。
            let _ = on_event.send(IslandState::Error {
                // ⚠️ `NetHttpStatus` 是那一族里最接近的一个 —— 而
                // **它不是"网络错误"的通称**（那正是这套错误码要避免的东西）。
                //
                // 真实做法（M5 起）应当是**从 `InstallError` 分类出错误码**
                //（`Network` / `IoDiskFull` / `IoDataRootNotWritable` / …）——
                // 而那需要 `InstallError` 携带一个错误码，而它现在不带。
                //
                // 所以这里**诚实地用最接近的那个**，并留下这条：
                // 一个完整的实现要能从失败反推出**可分类**的错误码。
                code: qul_core::error::ErrorCode::NetHttpStatus,
                human: human.clone(),
            });
            Err(human)
        }
    }
}

/// 版本清单的入口 URL。
///
/// ⚠️ **方案 §11.5 的纪律是"所有 URL 取自元数据"，而它是那个纪律的唯一例外**
/// —— 因为它是**入口**：没有它就没有元数据。
///
/// 而它**由命令层（调用方）持有**，不由编排层硬编码 ——
/// 那样测编排层时才能指向一个假的清单。
const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Serialize)]
struct InstallSummary {
    version: String,
    /// 这个版本一共需要几个文件
    needed: usize,
    /// 其中**磁盘上已有且 sha1 正确**的
    present: usize,
    /// 这一次真的下了几个
    downloaded: usize,
    /// 其中从已有安装迁移过来的（它们与下载的一样过了 SHA-1）
    migrated: usize,
}

/// **取消正在跑的那次安装。**
///
/// ⚠️ 它是 §7.3 约束 4（"可关闭但**任务不丢失**"）的另一半：
/// "收起"由前端做（它不碰队列），而"真的停下来"必须是**内核的 `CancelToken`**
/// —— 一个在命令层自己加 `AtomicBool` 的实现会让下载线程
/// **不会**在下一个检查点停下来。
#[tauri::command]
fn cancel_install(running: tauri::State<'_, Arc<Running>>) -> bool {
    running.cancel.with(|c| {
        if let Some(t) = c {
            t.cancel();
            true
        } else {
            false
        }
    })
}

/// **窗口控制：最小化 / 最大化切换 / 关闭。**
///
/// ## 🔴 为什么是三条自定义命令，而不是 `core:window` 那套权限
///
/// `capabilities/main-window.json` 的 `permissions` 是**空**的，
/// 而那是 §5.8 的要求。窗口控制本来可以走 Tauri 内建的
/// `core:window:allow-minimize` / `allow-close` —— 而**那会把
/// "前端能做什么"从"我们显式导出的命令"变成"一份插件权限表"**。
///
/// §5.8 的原文是：
///
/// > 需要文件操作的功能走**自定义命令**而不是通用 fs 插件。
///
/// 而这里把同一条推理用在了**窗口**上 —— 理由相同：
/// **一条自定义命令是一个具名的、可审计的入口，而一份插件权限是一族。**
/// 三个按钮只需要三个动作，不需要一族。
///
/// ## ⚠️ 而"最大化"是一条命令而不是两条
///
/// 一个 `maximize()` + `unmaximize()` 的实现会让**前端**决定"现在该做哪一个" ——
/// 而那个决定要读窗口当前状态（一次 IPC），于是两次 IPC 之间有竞态
///（用户连点两下 ⇒ 状态与按钮不一致）。
///
/// 所以语义是 **toggle**：前端只管说"切换"，而真相在窗口那边。
/// 返回值是**切换之后**是否最大化 —— 于是前端不必为了**自己刚点的那一下**
/// 再发一次 IPC 去问。
///
/// ⚠️ 而这只覆盖"我们自己点"那一路 —— 见下面的 `window_is_maximized`。
#[tauri::command]
fn window_minimize(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
fn window_toggle_maximize(window: tauri::Window) -> Result<bool, String> {
    let now = window.is_maximized().map_err(|e| e.to_string())?;
    if now {
        window.unmaximize().map_err(|e| e.to_string())?;
    } else {
        window.maximize().map_err(|e| e.to_string())?;
    }
    Ok(!now)
}

/// **窗口现在是不是最大化。**
///
/// ## ⚠️ 它为什么必需：`window_toggle_maximize` 的返回值只覆盖了一半的路径
///
/// 上面那条 toggle 解决的是"**我们自己点**完之后，按钮会不会与窗口不一致"。
/// 而**最大化不只由我们改变**：
///
/// | 谁改的 | 经过那三条命令吗 |
/// |---|---|
/// | 标题栏那个按钮、标题栏空白处双击 | ✅ |
/// | `Win + ↑`、把窗口拖到屏幕顶端 | ❌ |
///
/// 后者会让按钮上那个图形变成一个**谎**（窗口已经最大化，而它还画着
/// "点了能最大化"那个方框）。所以前端要有一个**问**的入口。
///
/// ⚠️ 而"在 `resize` 里自己翻一个布尔值"是错的：贴靠到屏幕左半边**也会**
/// 改变尺寸，而它**不是**最大化 —— 那会把图形翻成"还原"，同样是个谎。
/// 真相只能由窗口回答。
#[tauri::command]
fn window_is_maximized(window: tauri::Window) -> Result<bool, String> {
    window.is_maximized().map_err(|e| e.to_string())
}

/// **关闭窗口。**
///
/// ⚠️ 它走的是 `window.close()`，而那会触发 Tauri 的"关闭请求"流程
///（`CloseRequested` 事件）—— 于是**将来**要加"有任务在跑，确认吗"
/// 那道拦截时，这里是**唯一**要改的地方。
///
/// 一个直接 `std::process::exit()` 的实现会**跳过**那道流程 ——
/// 而那道流程正是"关闭窗口 ≠ 取消安装"（§7.3 约束 4）落地的位置。
#[tauri::command]
fn window_close(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

/// **开始拖动窗口。**
///
/// ## ⚠️ 它为什么必需：`decorations: false` 意味着**没有标题栏**
///
/// 而标题栏是**系统提供的拖动区**。关掉它之后，如果界面里不画一个，
/// 窗口就**移不动** —— 而"移不动"不是一个视觉问题，是一个
/// **只能靠任务栏或 Alt+F4 收场**的可用性问题。
///
/// ## 而它为什么是一条命令，而不是 `data-tauri-drag-region`
///
/// Tauri 有一个 `data-tauri-drag-region` 属性（界面里加一个属性就行）——
/// 而它**在 Tauri 2 里走的是 `__TAURI_INTERNALS__.invoke`**，也就是说
/// 那条路仍然要一次 IPC。
///
/// 两者都要一次 IPC，而**自定义命令那一条是可审计的**（它在命令表里，
/// 而 `data-tauri-drag-region` 是"某个属性被赋予了魔法"）。
/// 所以这里选前者 —— 与 `window_minimize` 那三条同一个理由（§5.8）。
#[tauri::command]
fn window_start_dragging(window: tauri::Window) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

/// 一个**极小的**互斥包装。
///
/// ⚠️ 它为什么不直接用 `std::sync::Mutex`：`CancelToken` 的持有不会 panic，
/// 所以"锁被毒化"在这里不可能发生，而 `unwrap_or_else(|e| e.into_inner())`
/// 把那件事表达成一次（而不是每处 `lock()` 写一遍）。
mod parking_lot_lite {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> {
        pub const fn new(v: T) -> Self {
            Self(std::sync::Mutex::new(v))
        }
        pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
            let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut g)
        }
    }
    impl<T: Default> Default for Mutex<T> {
        fn default() -> Self {
            Self::new(T::default())
        }
    }
}

/// 一次正在跑的安装（**给取消用**）。
#[derive(Default)]
struct Running {
    cancel: parking_lot_lite::Mutex<Option<Arc<CancelToken>>>,
}

fn main() {
    tauri::Builder::default()
        // 🔴 **命令表就是"界面能做什么"的完整清单。**
        //
        // 而它与 `capabilities/main-window.json` 的关系是：
        // 那里授予的是**插件权限**（我们一个都没要），而这里是**我们自己导出的命令**。
        // 所以"前端能做什么"的答案是这两者的交集。
        .manage(Arc::new(Running::default()))
        .invoke_handler(tauri::generate_handler![
            capabilities,
            instance_summary,
            install,
            cancel_install,
            // ⚠️ 窗口控制是**自定义命令**而不是 `core:window` 权限 ——
            // 理由见 `window_minimize` 上面那一段（§5.8 的同一条推理）。
            window_minimize,
            window_toggle_maximize,
            // ⚠️ 这一条不是"按钮需要它"，是"按钮**不许撒谎**"需要它 ——
            // 系统也能最大化窗口（`Win + ↑`），而那条路不经过我们。
            window_is_maximized,
            window_close,
            window_start_dragging
        ])
        .setup(|app| {
            // ── 窗口材质（方案 §3.4 的降级链的第 1 与第 2 档）──────────────
            //
            // ⚠️ **它必须在原生窗口上做**：§4.5 的实测约束写着
            // "Windows WebView2 上窗口透明与 `backdrop-filter` 不能共存"。
            // 所以材质来自 DWM，而界面层**零 CSS 模糊**。
            //
            // 而"降级到纯色"那一档（高对比度 / 省电 / 远程桌面 / Win10）
            // **由前端的 `appearance.ts` 决定** —— 它已经把那四条触发条件
            // 做成了可测试的纯函数。所以这里的失败**不是错误**：
            // 材质用不上时窗口照旧是一个普通窗口，而界面会走纯色底色。
            #[cfg(target_os = "windows")]
            {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    // Mica 优先，失败再试 Acrylic。**两次都失败就算了** ——
                    // 那不是错误，只是这一档材质在这台机器上不可用。
                    if window_vibrancy::apply_mica(&win, None).is_err() {
                        let _ = window_vibrancy::apply_acrylic(&win, None);
                    }
                }
            }
            let _ = app;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tauri 应用启动失败");
}
