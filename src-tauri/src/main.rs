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

use serde::Serialize;

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
fn capabilities() -> std::collections::BTreeMap<String, Capability> {
    let mut m = std::collections::BTreeMap::new();
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

fn main() {
    tauri::Builder::default()
        // 🔴 **命令表就是"界面能做什么"的完整清单。**
        //
        // 而它与 `capabilities/main-window.json` 的关系是：
        // 那里授予的是**插件权限**（我们一个都没要），而这里是**我们自己导出的命令**。
        // 所以"前端能做什么"的答案是这两者的交集 —— 而它就是下面这两个。
        .invoke_handler(tauri::generate_handler![capabilities, instance_summary])
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
