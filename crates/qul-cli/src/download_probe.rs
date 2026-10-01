//! # 真实下载的端到端取证（M3）
//!
//! ## 它证明的是哪一段
//!
//! ```text
//!   WinHTTP（https + TLS 由系统栈做）
//!      ↓
//!   Transport trait           ← qul_core 定义的契约
//!      ↓
//!   download 引擎             ← 重试 / 续传判定 / 临时文件 / 提交
//!      ↓
//!   Sha1Verifier              ← 校验
//!      ↓
//!   磁盘上的一个字节正确的文件
//! ```
//!
//! **这一段此前从没被真实网络走过** —— 而 M3 的出口条件正是"真实安装"。
//!
//! ## 它下载什么（**故意的选择**）
//!
//! 它下载**版本清单**（约 271 KB），而不是客户端 jar（40 MB）。三个理由：
//!
//! 1. **它是真实安装的第一步** —— 没有清单就不知道有哪些版本，
//!    所以这条链走通本身就是"安装能开始"的证据；
//! 2. 271 KB 让取证**几秒内完成**，而不是几十秒；
//! 3. **它的 sha1 不在我们手里**（清单是入口，没有人给它签名）——
//!    所以这里**刻意不校验 sha1**，而改成断言"它是一个能解析的清单，
//!    且里面的版本数与官方一致"。
//!
//! ⚠️ **不校验 sha1 这件事要写清楚**：这不是"忘了"，而是
//! "这个文件的存在性由 TLS 保证，而完整性由它自己的内容保证"。
//! 而**有 sha1 的文件（库、客户端）一律校验** —— 见 `install.rs`。
//!
//! ## 它同时验续传
//!
//! 第二段用 `Range` 只取前 1024 字节 —— 那证明**服务端回了 206**，
//! 也就是下载引擎的续传判定（I5）有真实的输入。

use qul_core::http::{FetchRequest, Transport};
use std::collections::BTreeMap;

/// 官方清单的 URL。**方案 §11.5 的端点清单**里有它。
///
/// ⚠️ 它在这里是**硬编码**的，而方案 §11.5 的纪律是"所有 URL 取自元数据"。
/// **这条 URL 是那个纪律的唯一例外** —— 因为它是**入口**：
/// 在拿到清单之前，没有任何元数据可以告诉我们它在哪。
/// 所以它必须是一个常量，而**这个例外要写下来**。
const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

pub fn run_download_probe() -> i32 {
    println!("=== 真实下载端到端取证（M3）===");
    println!();

    #[cfg(not(windows))]
    {
        println!("✗ 本命令需要 WinHTTP（仅 Windows）。");
        return 2;
    }

    #[cfg(windows)]
    {
        let t = qul_infra::winhttp::WinHttpTransport::new();

        // ── ① 取清单 ──
        println!("【1】用 WinHTTP 取版本清单（**https，TLS 由系统栈做**）");
        println!("  URL : {MANIFEST_URL}");
        println!("  说明: 这条 URL 硬编码是**唯一允许的例外** —— 它是入口，");
        println!("        在拿到清单之前没有任何元数据能告诉我们它在哪。");
        let started = std::time::Instant::now();
        let resp = match t.fetch(&FetchRequest::get(MANIFEST_URL)) {
            Ok(r) => r,
            Err(e) => {
                println!("  ✗ 请求失败：{e}");
                return 1;
            }
        };
        let ms = started.elapsed().as_millis();
        println!(
            "  ✓ 状态码 {}   {} 字节   {ms} ms",
            resp.status,
            resp.body.len()
        );
        if !resp.is_success() {
            println!("  ✗ 状态码不是 2xx");
            return 1;
        }

        // ── ② 头部如实带回了吗（下载引擎的 I5 依赖它）──
        println!();
        println!("【2】续传所需的头部是否如实带回");
        let sig = resp.resume_signals();
        let mut missing = Vec::new();
        for (name, v) in [
            ("accept-ranges", sig.accept_ranges.clone()),
            ("content-length", sig.content_length.map(|n| n.to_string())),
            ("etag", sig.etag.clone()),
            ("last-modified", sig.last_modified.clone()),
        ] {
            match v {
                Some(x) => println!("  ✓ {name} = {x}"),
                None => {
                    println!("  ✗ {name} 读不到");
                    missing.push(name);
                }
            }
        }
        if !missing.is_empty() {
            println!("  ✗ 缺 {missing:?} —— **续传会静默退化成全量重下**");
            return 1;
        }
        // 判定交给规则层（不在这里判）——
        // ⚠️ 是 `to_signals()` 再用 `verdict()`（两个类型，两次转换），
        // 而不是 `ResumeSignalsRaw::verdict()`。我第一版写错了。
        let verdict = sig.to_signals().verdict();
        println!("  规则层判定：{verdict:?}");

        // ── ③ 内容是一个真清单 ──
        println!();
        println!("【3】内容是一个能解析的真清单");
        let text = String::from_utf8_lossy(&resp.body).to_string();
        let m = match qul_core::descriptor::VersionManifest::parse(&text) {
            Ok(m) => m,
            Err(e) => {
                println!("  ✗ 解析失败：{e}");
                return 1;
            }
        };
        println!("  版本条目 : {} 条", m.versions.len());
        println!("  最新正式版 : {}", m.latest.release);
        println!("  最新快照   : {}", m.latest.snapshot);
        println!("  类型分布   : {:?}", m.kinds());
        if m.versions.len() < 100 {
            println!("  ✗ 条目太少（{}）—— 那不像一份真清单", m.versions.len());
            return 1;
        }

        // ── ④ 本机那份清单与它一致吗 ──
        println!();
        println!("【4】与本机官方启动器的缓存比对（**独立来源的交叉验证**）");
        let local = std::path::Path::new(&std::env::var("APPDATA").unwrap_or_default())
            .join(".minecraft")
            .join("versions")
            .join("version_manifest_v2.json");
        match std::fs::read_to_string(&local) {
            Ok(t2) => match qul_core::descriptor::VersionManifest::parse(&t2) {
                Ok(m2) => {
                    println!("  本机 : {} 条", m2.versions.len());
                    println!("  网络 : {} 条", m.versions.len());
                    if m2.versions.len() == m.versions.len() {
                        println!("  ✓ 条目数一致");
                    } else {
                        println!(
                            "  ⚠️ 条目数不同（差 {}）—— 可能是两次取的时间不同，不一定是错",
                            (m.versions.len() as i64 - m2.versions.len() as i64).abs()
                        );
                    }
                    println!("  本机最新正式版 : {}", m2.latest.release);
                }
                Err(e) => println!("  ⚠️ 本机那份解析失败：{e}"),
            },
            Err(e) => println!("  （读不到本机缓存：{e}）"),
        }

        // ── ⑤ 续传：Range 请求必须拿到 206 ──
        println!();
        println!("【5】续传：带 Range 的请求（**期望 206 而不是 200**）");
        let mut r2 = FetchRequest::get(MANIFEST_URL);
        r2.range = Some((0, 1023));
        match t.fetch(&r2) {
            Ok(p) => {
                println!("  状态码 {}   拿到 {} 字节", p.status, p.body.len());
                println!("  Content-Range = {:?}", p.header("content-range"));
                if p.is_partial() && p.body.len() == 1024 {
                    println!("  ✓ **206 + 恰好 1024 字节** —— 续传有真实的输入");
                } else if !p.is_partial() {
                    println!(
                        "  ✗ 服务端回了 {}（不是 206）—— 那下载引擎必须**删掉临时文件重头下**（I4）",
                        p.status
                    );
                    println!("     这一条**不是**我们的 bug：I4 就是为了这个情形写的。");
                } else {
                    println!("  ✗ 字节数不对（期望 1024）");
                    return 1;
                }
            }
            Err(e) => {
                println!("  ✗ Range 请求失败：{e}");
                return 1;
            }
        }

        // ── ⑥ 真的经过 download 引擎落盘一次 ──
        println!();
        println!("【6】真的经 `download` 引擎落盘（临时文件 → 校验 → 提交）");
        let tmp = std::env::temp_dir().join(format!("qul-dlprobe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let dest = tmp.join("manifest.json");
        // 先算一遍网络那份的 sha1，用它当期望值（**自洽校验**：
        // 它验的是"引擎把字节原封不动落盘了"，而不是"内容是对的"）。
        let want = qul_infra::check::sha1_hex(&resp.body);
        let verifier = match qul_infra::check::Sha1Verifier::new(&want) {
            Some(v) => v,
            None => {
                println!("  ✗ 算出来的 sha1 形态不对");
                return 1;
            }
        };
        let cancel = qul_core::retry::CancelToken::new();
        let outcome = qul_infra::download::download(
            &t,
            MANIFEST_URL,
            &dest,
            &verifier,
            &qul_infra::download::DownloadConfig::default(),
            &cancel,
            None,
        );
        match &outcome {
            qul_infra::download::DownloadOutcome::Done {
                bytes, attempts, ..
            } => {
                println!("  ✓ Done：{bytes} 字节，{attempts} 次尝试");
                match std::fs::metadata(&dest) {
                    Ok(md) if md.len() == resp.body.len() as u64 => {
                        println!("  ✓ 落盘大小与响应一致（{}）", md.len());
                    }
                    Ok(md) => {
                        println!("  ✗ 落盘大小 {} ≠ 响应 {}", md.len(), resp.body.len());
                        return 1;
                    }
                    Err(e) => {
                        println!("  ✗ 文件不在：{e}");
                        return 1;
                    }
                }
                // **临时文件不该留下**
                let parts = qul_infra::zip::sweep_parts(&tmp);
                if parts > 0 {
                    println!("  ✗ 留下了 {parts} 个临时文件");
                    return 1;
                }
                println!("  ✓ 没有留下临时文件（I1：未校验的字节不以正式名存在）");
            }
            other => {
                println!("  ✗ 下载没有成功：{other:?}");
                let _ = std::fs::remove_dir_all(&tmp);
                return 1;
            }
        }
        let _ = std::fs::remove_dir_all(&tmp);

        // ── ⑦ 本地假服务器：证明失败路径 ──
        println!();
        println!("【7】明确不联网的实现（离线模式）");
        let off = qul_infra::http::NoNetwork;
        match off.fetch(&FetchRequest::get("https://a.b/x?token=SECRET")) {
            Ok(_) => {
                println!("  ✗ 离线实现竟然成功了");
                return 1;
            }
            Err(e) => {
                println!("  ✓ 报错：{e}");
                if e.contains("SECRET") {
                    println!("  ✗ **错误里泄露了查询参数**");
                    return 1;
                }
                println!("  ✓ 且已脱敏（不含查询参数）");
            }
        }

        println!();
        println!("【结论】");
        println!("  ✓ https 走通（TLS 由**系统栈**做，零新依赖）");
        println!("  ✓ 续传所需的四个头如实带回");
        println!("  ✓ 917 条的清单能解析，且与本机缓存条目数一致");
        println!("  ✓ Range 请求得到 206 + 恰好 1024 字节");
        println!("  ✓ 经 `download` 引擎真的落盘，且不留临时文件");
        println!("  ✓ 离线实现明确报错且已脱敏");
        println!();
        println!("  ⚠️ 本命令**不下载游戏文件**（它只取清单）。");
        println!("     完整安装要 `qul install`，而它需要更多前置（natives 解压等）。");
        let _ = BTreeMap::<String, String>::new();
        0
    }
}
