//! # S1 尖刺：DWM 材质 × 窗口边框 矩阵
//!
//! **这是一次性实验代码**，不属于产品。它的**结论**进文档，代码本身完成使命后即可丢弃。
//!
//! ## 它要回答的唯一问题
//!
//! 我们要自绘标题栏（把灵动岛浮在顶部）→ 必须 `decorations: false`
//! → 而**无边框正是 DWM 材质历史上不生效的高发场景**。
//!
//! 所以矩阵是 **`{Mica, Acrylic}` × `{有边框, 无边框}`**，
//! 另外单独测**明暗主题**与**第三种材质 Tabbed**。
//!
//! **结论怎么用**：`docs/UI设计规格.md` §11.2 已写死 A / B1 / B2 三档预案，
//! 本尖刺的输出直接对号入座，**不需要临场决策**。
//!
//! ## 用法
//!
//! ```text
//! s1-material --material <mica|acrylic|tabbed|none> [--dark|--light] [--decorations] [--result <path>]
//! ```
//!
//! 窗口出现后**不自动关闭**：由外部脚本截图（`SetForegroundWindow` + `CopyFromScreen`）
//! 再结束进程。这样截图与"材质已合成"之间不需要靠猜时间。
//!
//! ## 两个刻意的实现选择
//!
//! 1. **测试页写到磁盘、用 `file://` 加载**，不用 `window.eval` 注入。
//!    理由：`setup` 钩子里 webview 可能还没就绪，`eval` 会静默失败——
//!    **那正是"验证条件不可靠"的一类事故**（这个项目已经栽过两次）。
//!    写文件则没有时序问题：路径给出去，webview 自己去读。
//! 2. **材质在后台线程里延迟 1800ms 再应用**：`apply_mica` 依赖窗口已映射，
//!    且 DWM 合成需要时间。延迟是**为了让"打不开"和"还没合成"不会互相冒充**。
//!
//! ## 为什么还要写一份 JSON
//!
//! stdout 在 Windows 终端里会被按 ANSI 解码（中文与路径都可能乱码），
//! 而**结果要进文档、要被脚本解析**。所以真实结论写进 JSON 文件，stdout 只做人看的简报。

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

// ── S2: geometry of the blur test page, in CSS pixels from the page's top-left.
//
// These are emitted in the result JSON so the analysis script can locate the
// card and the control WITHOUT guessing. That matters because a guess that is
// slightly off would silently measure the checkerboard instead of the card --
// and then "no blur detected" would be an artifact of the script, not a fact
// about WebView2. (This project has already been burned twice by verification
// conditions that did not match the thing under test.)
const CARD_X: f64 = 60.0;
const CARD_Y: f64 = 60.0;
const CARD_W: f64 = 380.0;
const CARD_H: f64 = 300.0;

/// 本次运行用到的全部参数（原样进 JSON，便于事后核对"这张图当时是什么配置"）。
#[derive(Debug, Clone, Serialize)]
struct RunSpec {
    material: String,
    /// 实际调用的 window-vibrancy 函数名
    api_called: String,
    /// `Some(true)`=深 / `Some(false)`=浅 / `None`=该 API 不接受明暗参数
    dark_param: Option<bool>,
    decorations: bool,
    /// 进程启动时的系统主题（0=深色 1=浅色）
    system_light_theme: Option<u32>,
    /// 透明效果总开关（关掉时材质会被系统忽略）
    enable_transparency: Option<u32>,
    os_build: String,
    /// 应用材质前的等待毫秒数
    apply_delay_ms: u64,
    // ── S2 ────────────────────────────────────────────────────────────────
    /// 窗口是否开启了透明（S2 的自变量）
    transparent: bool,
    /// 测试页是否含 `backdrop-filter` 卡片
    blur: bool,
    /// 被测卡片在页面坐标系里的矩形（CSS 像素）
    card_rect: Rect,
    /// 对照卡片（**同样式但不带 backdrop-filter**）的矩形。
    /// 它防的是"别的东西也在模糊"这种误判。
    control_rect: Rect,
}

/// 页面坐标系里的一个矩形（CSS 像素）。
#[derive(Debug, Clone, Copy, Serialize)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// 一次调用的结果，含**成功与失败两种情形**——
/// 失败也是一种结论（"无边框下 Mica 调用报错"本身就是矩阵里的一格）。
#[derive(Debug, Serialize)]
struct RunResult {
    spec: RunSpec,
    apply_ok: bool,
    /// 失败时的原始错误（**不许被替换成人话**——本项目的既定纪律）
    apply_error: Option<String>,
    window_width: u32,
    window_height: u32,
    window_x: i32,
    window_y: i32,
    /// 窗口 HWND 十进制值（供事后用其它工具复核材质）
    hwnd: String,
    /// 同时写在窗口标题与页面上，便于人眼核对"这张图是哪一格"
    run_id: String,
    html_path: String,
}

const APPLY_DELAY_MS: u64 = 1800;

/// 读注册表 DWORD。用 `reg query` 而不是绑 Win32 API：尖刺要的是**依赖最少**。
fn read_theme_dword(name: &str) -> Option<u32> {
    use std::process::Command;
    let out = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
            "/v",
            name,
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if line.contains(name) {
            if let Some(idx) = line.rfind("0x") {
                return u32::from_str_radix(line[idx + 2..].trim(), 16).ok();
            }
        }
    }
    None
}

fn os_build() -> String {
    use std::process::Command;
    Command::new("cmd")
        .args(["/C", "ver"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1).cloned())
}

fn has_flag(args: &[String], key: &str) -> bool {
    args.iter().any(|a| a == key)
}

/// 把测试页写到磁盘，返回**file:// URL**（Windows 路径需转成斜杠）。
fn write_html(dir: &PathBuf, html: &str) -> std::io::Result<String> {
    fs::create_dir_all(dir)?;
    let path = dir.join("index.html");
    fs::write(&path, html)?;
    let abs = fs::canonicalize(&path).unwrap_or(path);
    // canonicalize 在 Windows 上给 `\\?\C:\...`，去掉前缀再转斜杠
    let s = abs.to_string_lossy().replace(r"\\?\", "");
    Ok(format!("file:///{}", s.replace('\\', "/")))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    let material = arg_value(&args, "--material").unwrap_or_else(|| "mica".into());
    let dark = if has_flag(&args, "--dark") {
        Some(true)
    } else if has_flag(&args, "--light") {
        Some(false)
    } else {
        None
    };
    let decorations = has_flag(&args, "--decorations");

    // ── S2: transparency is the variable under test ────────────────────────
    //
    // S1 hard-coded `transparent(true)` because that was the only way to see
    // DWM material. S2 needs it as a SWITCH, because the whole question is
    // whether a transparent window breaks CSS `backdrop-filter`.
    //
    // Default is OPAQUE (false): that is the configuration we actually intend
    // to ship (material via DWM, no CSS blur). Pass --transparent to test the
    // other side.
    // ── S3: the ready signal ──────────────────────────────────────────────
    //
    // S3 measures COLD START. To do that the harness needs one unambiguous
    // instant meaning "the window is up and its page has loaded", and it must
    // come from the app itself rather than from polling for a window title
    // (polling adds its own latency and can match the wrong window).
    //
    // The file is written from Tauri's on_page_load callback, so it exists only
    // once the webview has actually loaded the page.
    let ready_path: Option<PathBuf> = arg_value(&args, "--ready-json").map(PathBuf::from);

    // S3 phase A is the "bare window" baseline. Bare means no page content
    // beyond a title, so the harness passes --no-page to load a minimal page.
    let no_page = has_flag(&args, "--no-page");

    let transparent = has_flag(&args, "--transparent");

    // ── S2: does the page exercise `backdrop-filter`? ──────────────────────
    // Default false keeps S1's page byte-identical, so re-running S1 still
    // reproduces its original numbers.
    let blur = has_flag(&args, "--blur");
    let result_path: PathBuf = arg_value(&args, "--result")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("s1-result.json"));

    let run_id = format!(
        "{}-{}-{}-{}-{}",
        material,
        match dark {
            Some(true) => "dark",
            Some(false) => "light",
            None => "auto",
        },
        if decorations { "deco" } else { "bare" },
        if transparent { "trans" } else { "opaque" },
        if blur { "blur" } else { "noblur" }
    );

    // ── the page ──────────────────────────────────────────────────────────
    //
    // `--no-page` is S3 phase A's "bare window": the smallest page that still
    // loads, so the measurement is the PLATFORM cost (Tauri + WebView2 + one
    // paint) rather than the cost of our own first screen. Measuring our real
    // first screen here would conflate the platform with the app, which is the
    // same mistake S3's two-phase design exists to avoid.
    let html = if no_page {
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>S3 bare</title>\
         <style>html,body{margin:0;height:100%;background:#ffffff}</style></head>\
         <body></body></html>"
            .to_string()
    } else {
        build_html(&run_id, &material, dark, decorations, transparent, blur)
    };
    let html_dir = result_path
        .parent()
        .map(|p| p.join(format!("page-{run_id}")))
        .unwrap_or_else(|| PathBuf::from(format!("page-{run_id}")));

    let html_url = match write_html(&html_dir, &html) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("写测试页失败：{e}");
            std::process::exit(2);
        }
    };

    let spec = RunSpec {
        material: material.clone(),
        api_called: String::new(),
        dark_param: dark,
        decorations,
        system_light_theme: read_theme_dword("AppsUseLightTheme"),
        enable_transparency: read_theme_dword("EnableTransparency"),
        os_build: os_build(),
        apply_delay_ms: APPLY_DELAY_MS,
        transparent,
        blur,
        card_rect: Rect { x: CARD_X, y: CARD_Y, w: CARD_W, h: CARD_H },
        control_rect: Rect {
            x: CARD_X + CARD_W + 40.0,
            y: CARD_Y,
            w: CARD_W,
            h: CARD_H,
        },
    };

    let title = format!("S1 | {run_id}");
    let result_path_setup = result_path.clone();
    let html_path_display = html_url.clone();
    // Cloned for the on_page_load closure: it is `move`, so it would otherwise
    // take ownership and the later code could not read the path any more.
    let ready_path_for_load = ready_path.clone();

    tauri::Builder::default()
        .setup(move |app| {
            let window = tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External(html_url.parse().unwrap()),
            )
            .title(&title)
            .inner_size(1100.0, 760.0)
            .position(120.0, 90.0)
            .decorations(decorations)
            // ⚠️ **`transparent(true)` 是材质能不能看见的关键（S1 结论）。**
            //
            // S1 第一轮矩阵的像素分析显示：所有格子（含对照组）窗口内部
            // 都是纯白、标准差 0，且 `apply_mica` 那格与"完全不调材质"的对照组
            // 数值一模一样。也就是说 apply_* 都返回 Ok，但**没有任何可见效果**。
            //
            // 原因：WebView2 默认以不透明背景填充窗口，把 DWM 材质整个盖住。
            // 材质改的是**窗口**属性，而窗口被一层不透明内容填满 → 看不见。
            //
            // S2 把它变成**开关**：不透明才是我们要发货的形态（材质走 DWM、
            // 界面零 CSS 模糊）。透明那一侧是用来验"透明会不会破坏
            // backdrop-filter"的。
            .transparent(transparent)
            // ── S3: write the ready signal the moment the page has loaded ──
            //
            // on_page_load fires after the webview has loaded the URL, i.e. after
            // the window is up and its content exists. The harness waits for this
            // file and differences its mtime against the process START time
            // (reported below as wall_ms / perf_ms), which gives a cold-start
            // number that does not depend on how often the harness polls.
            .on_page_load(move |window, _payload| {
                let Some(path) = ready_path_for_load.clone() else { return };
                let wall_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let hwnd = window
                    .hwnd()
                    .map(|x| format!("{}", x.0 as isize))
                    .unwrap_or_else(|_| "unavailable".into());
                let payload = format!(
                    "{{\"event\":\"page-loaded\",\"wall_ms\":{wall_ms},\"pid\":{},\"hwnd\":\"{hwnd}\"}}",
                    std::process::id()
                );
                // A failure here must be loud: a missing ready file looks exactly
                // like "the app never became ready", which would corrupt S3.
                match fs::write(&path, payload) {
                    Ok(()) => println!("READY_JSON : {}", path.display()),
                    Err(e) => eprintln!("写 ready JSON 失败 —— {e}"),
                }
            })
            .build()?;

            // ── 延迟后在后台线程里应用材质 ─────────────────────────────
            let win = window.clone();
            let material_for_thread = material.clone();
            let spec_for_thread = spec.clone();
            let result_for_thread = result_path_setup.clone();
            let run_id_thread = run_id.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(APPLY_DELAY_MS));

                let (api_called, apply_result) = match material_for_thread.as_str() {
                    "mica" => (
                        "apply_mica".to_string(),
                        window_vibrancy::apply_mica(&win, dark),
                    ),
                    "acrylic" => (
                        "apply_acrylic".to_string(),
                        window_vibrancy::apply_acrylic(&win, Some((18, 18, 18, 40))),
                    ),
                    "tabbed" => (
                        "apply_tabbed".to_string(),
                        window_vibrancy::apply_tabbed(&win, dark),
                    ),
                    "none" => ("(未调用)".to_string(), Ok(())),
                    other => (format!("(未知材质 {other})"), Ok(())),
                };

                let mut spec = spec_for_thread;
                spec.api_called = api_called;

                let (w, h) = win
                    .inner_size()
                    .map(|s| (s.width, s.height))
                    .unwrap_or((0, 0));
                let pos = win.outer_position().ok();
                let hwnd = win
                    .hwnd()
                    .map(|x| format!("{}", x.0 as isize))
                    .unwrap_or_else(|_| "unavailable".into());

                let result = RunResult {
                    spec,
                    apply_ok: apply_result.is_ok(),
                    apply_error: apply_result.err().map(|e| e.to_string()),
                    window_width: w,
                    window_height: h,
                    window_x: pos.map(|p| p.x).unwrap_or(0),
                    window_y: pos.map(|p| p.y).unwrap_or(0),
                    hwnd,
                    run_id: run_id_thread,
                    html_path: html_path_display,
                };

                // ⚠️ **不许用 `let _ =` 吞掉写失败。**
                //
                // 这里踩过一次：外部脚本传进来的 `--result` 路径含空格，
                // 被 shell 拆成两个参数 → 路径在空格处截断 → `fs::write` 失败 →
                // 而当时的 `let _ =` 让这次失败**完全无声**。
                // 现象是"进程活着、窗口开着、就是不产出结果"，排查方向全错。
                //
                // **教训与项目既定纪律一致：错误码可以复用，错误说明不能丢。**
                let write_result = serde_json::to_string_pretty(&result)
                    .map_err(|e| format!("序列化失败: {e}"))
                    .and_then(|json| {
                        fs::write(&result_for_thread, json)
                            .map_err(|e| format!("写入 {} 失败: {e}", result_for_thread.display()))
                    });

                match &write_result {
                    Ok(()) => println!("result_json : {}", result_for_thread.display()),
                    Err(e) => {
                        println!("result_json : (写入失败)");
                        eprintln!("写结果 JSON 失败 —— {e}");
                    }
                }

                // 再等一会，让 DWM 把材质合成完，截图脚本会在这之后动手
                std::thread::sleep(Duration::from_millis(1200));

                println!("=== S1 尖刺 ===");
                println!("run_id      : {}", result.run_id);
                println!("material    : {}", result.spec.material);
                println!("api_called  : {}", result.spec.api_called);
                println!("dark_param  : {:?}", result.spec.dark_param);
                println!("decorations : {}", result.spec.decorations);
                println!("apply_ok    : {}", result.apply_ok);
                if let Some(e) = &result.apply_error {
                    println!("apply_error : {e}");
                }
                println!(
                    "window      : {}x{} @ ({},{})",
                    result.window_width, result.window_height, result.window_x, result.window_y
                );
                println!("hwnd        : {}", result.hwnd);
                println!("READY");
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("尖刺运行失败");
}

/// 构造测试页。
///
/// **页面本身就是测量工具**，所以它有几样东西：
/// 1. **大号深色粗体文字**标注配置 —— 截图里能直接读到"这是哪一格"，不靠文件名。
///    用深色是因为实测材质背景是近白的：浅色文字在近白底上几乎看不见
///    （第一版就是浅色字，截图里只剩一片白）。
/// 2. **不透明色块 + 描边块** —— 材质若未生效，这些区域会是死色，边缘出现硬边
/// 3. **窗口四周 14px 透明边** —— 它是判断材质的**关键观察区**：
///    材质生效时这里透出桌面（像素方差高），失效时是死色（方差低）。
/// 4. **亮度阶梯条** —— 让"材质到底有没有改变背景亮度"变成可测的量，
///    而不是靠肉眼说"看起来差不多"。
fn build_html(
    run_id: &str,
    material: &str,
    dark: Option<bool>,
    decorations: bool,
    transparent: bool,
    blur: bool,
) -> String {
    let mode = match dark {
        Some(true) => "dark=true",
        Some(false) => "dark=false",
        None => "dark=未指定",
    };

    // ── S2: the blur test page ────────────────────────────────────────────
    //
    // WHY A CHECKERBOARD: the question is not "does it look glassy" but
    // "did `backdrop-filter` actually blur anything". A high-frequency
    // checkerboard makes that answerable by pixels: blur provably smooths it,
    // and a non-blurred control region right next to it keeps its contrast.
    //
    // WHY A SOLID-RED PROBE: red survives every material blend nearly
    // untouched, so reading the checkerboard's red channel inside the card
    // still tells us whether it was smoothed. (S1 learned this the hard way:
    // checking saturation alone made every channel look washed out.)
    //
    // CARD GEOMETRY IS EMITTED IN THE RESULT JSON (see CARD_* below), so the
    // analysis script never has to guess where the card is.
    if blur {
        return format!(
            r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8">
<style>
  /* Transparent page background: this is what makes the window's own
     transparency (and therefore the S2 question) meaningful. */
  html,body {{ margin:0; height:100%; background:transparent; overflow:hidden;
    font: 13px/1.6 "Microsoft YaHei UI","Noto Sans SC",system-ui,sans-serif;
    color:#101014; }}

  /* High-frequency checkerboard. BLUR PROVABLY SMOOTHS THIS.
     Deliberately pure BLACK / WHITE, and the metric is LUMINANCE.

     Two earlier attempts and why they were abandoned:
       1. two overlapping 45-degree red gradients -> rendered white/near-black
       2. explicit red/black gradients            -> still rendered white/near-black
     Red simply does not survive to the screenshot in this WebView. Rather than
     keep guessing why, the pattern now asserts ONLY what is actually measured:
     sharp black/white edges, which blur provably softens. A test page must not
     claim a colour it does not render -- that is how a measurement ends up
     disagreeing with reality while the report still says "pass". */
  .checker {{
    position:fixed; inset:0; background-color:#000000;
    background-image: repeating-conic-gradient(#ffffff 0% 25%, #000000 0% 50%);
    background-size: 16px 16px;
  }}

  /* The card under test. Mostly empty on purpose: the interior must stay a
     clean checkerboard sample so the measurement is not polluted by text. */
  .card {{
    position: fixed;
    left: {card_x}px; top: {card_y}px;
    width: {card_w}px; height: {card_h}px;
    border-radius: 12px;
    border: 1px solid rgba(255,255,255,0.35);
    background: rgba(255,255,255,0.06);
    -webkit-backdrop-filter: blur(24px) saturate(160%);
    backdrop-filter: blur(24px) saturate(160%);
  }}

  /* A second card WITHOUT backdrop-filter, same size and colour.
     This is the control: if BOTH look smoothed, then something other than
     `backdrop-filter` is doing it, and the measurement is invalid. */
  .card-control {{
    position: fixed;
    left: {ctrl_x}px; top: {card_y}px;
    width: {card_w}px; height: {card_h}px;
    border-radius: 12px;
    border: 1px solid rgba(255,255,255,0.35);
    background: rgba(255,255,255,0.06);
  }}

  .label {{
    position: fixed; left: {card_x}px; top: {label_y}px;
    font-size: 12px; font-weight: 700; color:#ffffff;
    text-shadow: 0 1px 4px #000;
    background: rgba(0,0,0,0.55); padding: 3px 8px; border-radius: 6px;
  }}
  .hud {{ position:fixed; right:14px; top:14px; font-size:11px; color:#fff;
          background:rgba(0,0,0,0.6); padding:6px 10px; border-radius:6px;
          font-family:Consolas,monospace; }}
</style></head><body>
<div class="checker"></div>
<div class="card" id="card"></div>
<div class="card-control" id="card-control"></div>
<div class="label">backdrop-filter: blur(24px) &nbsp;|&nbsp; run {run_id}</div>
<div class="hud">material={material} &middot; {mode} &middot; transparent={transparent} &middot; blur={blur}</div>
</body></html>"#,
            card_x = CARD_X,
            card_y = CARD_Y,
            card_w = CARD_W,
            card_h = CARD_H,
            ctrl_x = CARD_X + CARD_W + 40.0,
            label_y = CARD_Y + CARD_H + 10.0,
        );
    }

    // ── S1's original page (unchanged when --blur is absent) ──────────────
    format!(
        r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8">
<style>
  /* 背景保持透明：让窗口背景（材质）能透出来。
     注意 WebView2 默认不透明，还需要窗口侧 transparent(true) 配合。 */
  html,body {{ margin:0; height:100%; background:transparent;
    font: 15px/1.7 "Microsoft YaHei UI","Noto Sans SC",system-ui,sans-serif;
    color:#101014; }}
  .wrap {{ padding:20px; }}
  .id {{ font-size:30px; font-weight:700; letter-spacing:.2px; }}
  .meta {{ margin-top:10px; font-size:17px; font-weight:600; }}
  .row {{ display:flex; gap:14px; margin-top:20px; }}
  .box {{ width:180px; height:84px; border-radius:8px; display:flex;
          align-items:center; justify-content:center; font-size:13px; font-weight:600; }}
  .opaque {{ background:#2b2b2f; color:#fff; border:1px solid #00000055; }}
  .white  {{ background:#ffffff; color:#111; border:1px solid #00000055; }}
  .stroke {{ background:transparent; border:2px solid #101014; }}
  .ladder {{ display:flex; margin-top:20px; }}
  .ladder div {{ width:56px; height:34px; border:1px solid #00000033;
                 display:flex; align-items:flex-end; justify-content:center;
                 font-size:9px; color:#000; }}
  .hint {{ margin-top:20px; padding:12px 14px; border:2px solid #10101499;
           border-radius:8px; background:#ffffff40; max-width:700px; font-weight:600; }}
  /* 四周透明边：材质生效时透出桌面，失效时是死色 —— 关键观察区 */
  .edge {{ position:fixed; inset:0; pointer-events:none;
           border:14px solid rgba(255,0,0,0.001); }}
</style></head><body>
<div class="edge"></div>
<div class="wrap">
  <div class="id">{run_id}</div>
  <div class="meta">material=<b>{material}</b> &middot; {mode} &middot; decorations=<b>{decorations}</b></div>
  <div class="row">
    <div class="box opaque">不透明块 #2b2b2f</div>
    <div class="box white">白色块（看对比）</div>
    <div class="box stroke">只有描边（看分层）</div>
  </div>
  <div class="ladder">
    <div style="background:#000000">0</div><div style="background:#333333"></div>
    <div style="background:#666666"></div><div style="background:#999999"></div>
    <div style="background:#cccccc"></div><div style="background:#ffffff">255</div>
  </div>
  <div class="hint">
    <b>观察要点</b>：① 窗口四周那 14px 边是否透出桌面（透出＝材质生效）；
    ② 三个色块边缘有没有黑边或白边（有＝材质未合成）；
    ③ 整窗底色是纯白（255）还是被材质染色（＝材质在工作）。
  </div>
</div>
</body></html>"#
    )
}
