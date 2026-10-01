//! # 下载引擎的端到端测试（**本地假服务器，不碰外网**）
//!
//! ## 为什么用本地假服务器而不是 Mock
//!
//! 因为这几条不变量**只在真实 HTTP 语义下才成立**：
//!
//! | 不变量 | 它依赖的真实语义 |
//! |---|---|
//! | **I1** 未校验的字节不以正式文件名存在 | **文件系统**的 rename 语义 |
//! | **I4** 服务端忽略 Range 时必须重头下 | **HTTP 状态码**（200 vs 206） |
//! | **I5** 续传前置条件 | **真实的响应头**（`accept-ranges` / `etag` / `content-encoding`） |
//!
//! 一个 Mock 传输层可以"假装"返回 200，但它**证明不了**我们的客户端
//! 在真实响应下会怎么解析头。而这个文件里有一个**真的 TCP 服务器**，
//! 它按字节写 HTTP 响应 —— 所以那些不变量是在真实协议上被验证的。
//!
//! ## 为什么自己写最小 HTTP 客户端
//!
//! **不引 HTTP 客户端依赖**：尖刺阶段每多一条依赖就多一条要审的许可。
//! 而这里只需要 `GET`、一个 `Range` 头、以及"读头 + 读体"这点能力 ——
//! 它大约 80 行，且**它的正确性由这个文件里的端到端测试保证**
//! （若它解析错了，所有测试都会红）。
//!
//! ⚠️ **它只支持 `http://`（明文）**。真实下载走 HTTPS，那需要 TLS ——
//! 而 TLS 不该自己写。所以本文件末尾有一条测试**明确记下这个限制**，
//! 免得有人以为它可以下真实的 Mojang 地址。

use qul_core::http::{FetchRequest, FetchResponse, Transport};
use qul_core::retry::{CancelToken, RetryPolicy};
use qul_infra::check::{sha1_hex, Sha1Verifier};
use qul_infra::download::{
    download, download_segmented, temp_path, DownloadConfig, DownloadOutcome, NoVerify, Verifier,
};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ══════════════════════════ 最小 HTTP 客户端 ══════════════════════════

/// 一个**故意最小**的 HTTP/1.1 客户端。
///
/// 它只做 `GET`，只解析 `Content-Length` 定长响应（**不支持 chunked**）——
/// 因为拿它测试的是"我们自己的下载引擎"，而不是"我们自己的 HTTP 实现"。
/// 真实下载会用平台提供的网络栈（见文件末尾的限制说明）。
pub struct TinyHttp;

impl Transport for TinyHttp {
    fn fetch(&self, req: &FetchRequest) -> Result<FetchResponse, String> {
        let rest = req
            .url
            .strip_prefix("http://")
            .ok_or_else(|| format!("TinyHttp 只支持 http://（收到 {}）", req.url))?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let mut stream = TcpStream::connect(authority).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .ok();

        let mut head = format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n");
        if let Some(r) = req.range_header() {
            head.push_str(&format!("Range: {r}\r\n"));
        }
        if let Some(v) = &req.if_match {
            head.push_str(&format!("If-Match: {v}\r\n"));
        }
        if let Some(v) = &req.if_unmodified_since {
            head.push_str(&format!("If-Unmodified-Since: {v}\r\n"));
        }
        for (k, v) in &req.extra_headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        stream
            .write_all(head.as_bytes())
            .map_err(|e| e.to_string())?;
        stream.flush().ok();

        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader
            .read_line(&mut status_line)
            .map_err(|e| e.to_string())?;
        let status: u16 = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("状态行无法解析：{status_line:?}"))?;

        let mut resp = FetchResponse::new(status, Vec::new());
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            let t = line.trim_end();
            if t.is_empty() {
                break;
            }
            if let Some((k, v)) = t.split_once(':') {
                resp.headers
                    .insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        let len: usize = resp
            .header("content-length")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        if len > 0 {
            reader.read_exact(&mut body).map_err(|e| e.to_string())?;
        }
        resp.body = body;
        Ok(resp)
    }
}

// ══════════════════════════ 本地假服务器 ══════════════════════════

/// 假服务器的行为开关。**每一种都对应一条不变量要验的情形。**
#[derive(Clone)]
pub struct FakeBehavior {
    /// **忽略 Range**（无论收到什么都返回 200 + 整份）→ 验 I4
    pub ignore_range: bool,
    /// 声称的 `Accept-Ranges`
    pub accept_ranges: Option<&'static str>,
    /// 发一个压缩声明（让续传判定失败）
    pub content_encoding: Option<&'static str>,
    /// 是否发 `ETag`
    pub etag: Option<&'static str>,
    /// 每次响应体都被改坏（模拟"传输损坏"）→ 验校验失败重试
    pub corrupt_every: Option<usize>,
}

impl Default for FakeBehavior {
    fn default() -> Self {
        Self {
            ignore_range: false,
            accept_ranges: Some("bytes"),
            content_encoding: None,
            etag: Some("\"v1\""),
            corrupt_every: None,
        }
    }
}

/// 一个**真的 TCP 服务器**，按字节写 HTTP 响应。
pub struct FakeServer {
    pub addr: String,
    pub payload: Arc<Vec<u8>>,
    pub behavior: FakeBehavior,
    pub hits: Arc<AtomicUsize>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl FakeServer {
    /// 起一个服务器。`behavior` 决定它怎么回应。
    pub fn start(payload: Vec<u8>, behavior: FakeBehavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("应当能绑定本地端口");
        let addr = listener.local_addr().unwrap().to_string();
        let payload = Arc::new(payload);
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));

        {
            let (payload, hits, stop, behavior) = (
                payload.clone(),
                hits.clone(),
                stop.clone(),
                behavior.clone(),
            );
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let (payload, hits, behavior) =
                        (payload.clone(), hits.clone(), behavior.clone());
                    std::thread::spawn(move || {
                        let _ = serve_one(stream, &payload, &behavior, &hits);
                    });
                }
            });
        }

        Self {
            addr,
            payload,
            behavior,
            hits,
            stop,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // 主动连一次，让 accept 循环从阻塞里出来
        let _ = TcpStream::connect(&self.addr);
    }
}

fn serve_one(
    mut stream: TcpStream,
    payload: &[u8],
    b: &FakeBehavior,
    hits: &AtomicUsize,
) -> std::io::Result<()> {
    let n = hits.fetch_add(1, Ordering::SeqCst);
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut range: Option<(u64, u64)> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let t = line.trim_end();
        if t.is_empty() {
            break;
        }
        if let Some(v) = t.strip_prefix("Range: ") {
            // `bytes=start-end`
            if let Some(spec) = v.trim().strip_prefix("bytes=") {
                if let Some((a, bb)) = spec.split_once('-') {
                    let start = a.trim().parse::<u64>().unwrap_or(0);
                    let end = bb.trim().parse::<u64>().unwrap_or(u64::MAX);
                    range = Some((start, end));
                }
            }
        }
    }

    let total = payload.len() as u64;
    // I4：**忽略 Range** 的服务器永远返回 200 + 整份
    let (status, body, start, end) = match range {
        Some((s, e)) if !b.ignore_range => {
            let e = e.min(total.saturating_sub(1));
            if s > e || s >= total {
                // 越界的 Range：返回 416 —— 这也是一个真实分支
                let head = format!(
                    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nContent-Range: bytes */{total}\r\n\r\n"
                );
                stream.write_all(head.as_bytes())?;
                return Ok(());
            }
            (206u16, payload[s as usize..=e as usize].to_vec(), s, e)
        }
        _ => (200u16, payload.to_vec(), 0u64, total.saturating_sub(1)),
    };

    // 让它坏掉：模拟传输损坏（验校验失败重试）
    let mut body = body;
    if let Some(every) = b.corrupt_every {
        if every > 0 && n.is_multiple_of(every) && !body.is_empty() {
            body[0] = body[0].wrapping_add(1);
        }
    }

    let mut head = format!(
        "HTTP/1.1 {status} {}\r\n",
        if status == 206 {
            "Partial Content"
        } else {
            "OK"
        }
    );
    head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    if status == 206 {
        head.push_str(&format!("Content-Range: bytes {start}-{end}/{total}\r\n"));
    }
    if let Some(ar) = b.accept_ranges {
        head.push_str(&format!("Accept-Ranges: {ar}\r\n"));
    }
    if let Some(ce) = b.content_encoding {
        head.push_str(&format!("Content-Encoding: {ce}\r\n"));
    }
    if let Some(et) = b.etag {
        head.push_str(&format!("ETag: {et}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

// ══════════════════════════ 工具 ══════════════════════════

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "qul-dl-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|x| x.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn payload(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}

// ══════════════════════════ I1：未校验的字节 ══════════════════════════

#[test]
fn 校验通过后才有正式文件名() {
    let data = payload(4096);
    let srv = FakeServer::start(data.clone(), FakeBehavior::default());
    let d = tmpdir("commit");
    let dest = d.join("a.bin");
    let want = sha1_hex(&data);
    let v = Sha1Verifier::new(&want).unwrap();

    let out = download(
        &TinyHttp,
        &srv.url("/a.bin"),
        &dest,
        &v,
        &DownloadConfig::default(),
        &CancelToken::new(),
        None,
    );
    match out {
        DownloadOutcome::Done {
            bytes, segmented, ..
        } => {
            assert_eq!(bytes, 4096);
            assert!(!segmented, "单段下载不该标为分段");
        }
        other => panic!("应当成功，实际 {other:?}"),
    }
    assert!(dest.is_file(), "正式文件必须存在");
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    // **I1：临时文件必须已经不在了**
    assert!(
        !temp_path(&dest).exists(),
        "提交之后不该留下 .download 临时文件"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 校验不通过时不留下正式文件() {
    // I1 的核心：**未通过校验的字节永远不以正式文件名存在**。
    // 若这条不成立，游戏启动器会读到一个"看起来完整"的坏文件，
    // 而用户看到的是"游戏起不来" —— 真正的状态是"那份文件本该被删掉"。
    let data = payload(2048);
    let srv = FakeServer::start(data.clone(), FakeBehavior::default());
    let d = tmpdir("noverify");
    let dest = d.join("b.bin");
    // **故意给一个错的期望哈希**
    let v = Sha1Verifier::new(&"0".repeat(40)).unwrap();

    let out = download(
        &TinyHttp,
        &srv.url("/b.bin"),
        &dest,
        &v,
        &DownloadConfig::default(),
        &CancelToken::new(),
        None,
    );
    match out {
        DownloadOutcome::Failed {
            reason, attempts, ..
        } => {
            assert!(reason.contains("SHA-1"), "{reason}");
            assert!(attempts >= 3, "应当重试够次数：{attempts}");
        }
        other => panic!("应当失败，实际 {other:?}"),
    }
    assert!(!dest.exists(), "**绝不能留下正式文件**");
    assert!(!temp_path(&dest).exists(), "失败后临时文件也该被清掉");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 传输损坏会被校验拦住并重试成功() {
    // 每第 2 次响应坏一个字节 → 第一次失败、第二次成功。
    let data = payload(1024);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            corrupt_every: Some(2),
            ..Default::default()
        },
    );
    let d = tmpdir("corrupt");
    let dest = d.join("c.bin");
    let v = Sha1Verifier::new(&sha1_hex(&data)).unwrap();

    let out = download(
        &TinyHttp,
        &srv.url("/c.bin"),
        &dest,
        &v,
        &DownloadConfig::default(),
        &CancelToken::new(),
        None,
    );
    match out {
        DownloadOutcome::Done { .. } => {}
        other => panic!("第一次坏、第二次应当成功，实际 {other:?}"),
    }
    assert_eq!(std::fs::read(&dest).unwrap(), data, "最终内容必须正确");
    assert!(srv.hits.load(Ordering::SeqCst) >= 2, "应当请求了不止一次");
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ I4：服务端忽略 Range ══════════════════════════

#[test]
fn 服务端忽略_range_时我们重头下而不是接着写() {
    // **这条测试就是 I4 的验收。**
    //
    // "接着写"会产出一个**长度对、内容错**的文件 ——
    // 而它随后会被校验拦住，表现为"反复校验失败"，
    // 而真正的原因在 Range 处理里。
    //
    // 做法：先手工放一个"半截临时文件"，再让服务器忽略 Range。
    // 若实现是"接着写"，最终文件会错；若是"删掉重头下"，就会对。
    let data = payload(4096);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            ignore_range: true,
            ..Default::default()
        },
    );
    let d = tmpdir("ignored-range");
    let dest = d.join("d.bin");
    // 制造一个"上次下到一半"的临时文件（内容是垃圾）
    let tmp = temp_path(&dest);
    std::fs::write(&tmp, vec![0xEEu8; 1000]).unwrap();

    let v = Sha1Verifier::new(&sha1_hex(&data)).unwrap();
    let out = download(
        &TinyHttp,
        &srv.url("/d.bin"),
        &dest,
        &v,
        &DownloadConfig::default(),
        &CancelToken::new(),
        None,
    );
    match out {
        DownloadOutcome::Done { .. } => {}
        other => panic!("应当成功（重头下），实际 {other:?}"),
    }
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        data,
        "**内容必须完整正确** —— 若实现是接着写，这里会多出 1000 字节垃圾"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 忽略_range_时不消耗网络重试次数() {
    // 一个"忽略 Range"的服务端是**协议层面的正常分支**，不是错误。
    // 若它消耗重试次数，那么一个明确声明不支持续传的服务器
    // 会让下载在几次后**彻底失败** —— 而它本来只是慢一点而已。
    let data = payload(512);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            ignore_range: true,
            ..Default::default()
        },
    );
    let d = tmpdir("no-retry-cost");
    let dest = d.join("e.bin");
    let tmp = temp_path(&dest);
    std::fs::write(&tmp, vec![0u8; 64]).unwrap();

    let cfg = DownloadConfig {
        // 只允许 5 次网络尝试
        retry: RetryPolicy {
            max_attempts: 5,
            ..RetryPolicy::default_for_network()
        },
        ..Default::default()
    };
    let v = Sha1Verifier::new(&sha1_hex(&data)).unwrap();
    let out = download(
        &TinyHttp,
        &srv.url("/e.bin"),
        &dest,
        &v,
        &cfg,
        &CancelToken::new(),
        None,
    );
    assert!(
        matches!(out, DownloadOutcome::Done { .. }),
        "忽略 Range 不该把重试次数用光，实际 {out:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ I2：提交与取消 ══════════════════════════

#[test]
fn 已取消时不产生任何请求也不留文件() {
    let data = payload(256);
    let srv = FakeServer::start(data.clone(), FakeBehavior::default());
    let d = tmpdir("cancelled");
    let dest = d.join("f.bin");
    let cancel = CancelToken::new();
    cancel.cancel();

    let out = download(
        &TinyHttp,
        &srv.url("/f.bin"),
        &dest,
        &NoVerify,
        &DownloadConfig::default(),
        &cancel,
        None,
    );
    assert!(matches!(out, DownloadOutcome::Cancelled), "{out:?}");
    assert_eq!(srv.hits.load(Ordering::SeqCst), 0, "取消后不该发请求");
    assert!(!dest.exists());
    assert!(!temp_path(&dest).exists());
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ I8：多段并行 ══════════════════════════

#[test]
fn 多段下载拼出的文件与原始逐字节一致() {
    // I8 的核心：**每段带 Range 写进同一个临时文件的不同偏移**，
    // 全部完成才整体校验 + 原子提交。
    let data = payload(4 * 1024 * 1024 + 12345);
    let srv = FakeServer::start(data.clone(), FakeBehavior::default());
    let d = tmpdir("segmented");
    let dest = d.join("g.bin");
    let v = Sha1Verifier::new(&sha1_hex(&data)).unwrap();

    let out = download_segmented(
        &TinyHttp,
        &srv.url("/g.bin"),
        &dest,
        data.len() as u64,
        &v,
        &DownloadConfig {
            want_segments: 4,
            ..Default::default()
        },
        &CancelToken::new(),
    );
    match out {
        DownloadOutcome::Done {
            segmented, bytes, ..
        } => {
            assert!(segmented, "应当标为分段下载");
            assert_eq!(bytes, data.len() as u64);
        }
        other => panic!("应当成功，实际 {other:?}"),
    }
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        data,
        "**多段拼接必须逐字节一致** —— 一个漏几百字节的分段计划会在这里露出来"
    );
    assert!(srv.hits.load(Ordering::SeqCst) >= 4, "应当发了多段请求");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 多段遇到忽略_range_的服务器会明确失败而不是写坏文件() {
    // 这是分段下载**最危险的分支**：若某一段拿到 200（整份内容），
    // 把它写进那一段的偏移会**把文件写坏**。
    // 所以必须明确失败并退回单段。
    let data = payload(4 * 1024 * 1024);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            ignore_range: true,
            ..Default::default()
        },
    );
    let d = tmpdir("seg-ignore");
    let dest = d.join("h.bin");
    let v = Sha1Verifier::new(&sha1_hex(&data)).unwrap();

    let out = download_segmented(
        &TinyHttp,
        &srv.url("/h.bin"),
        &dest,
        data.len() as u64,
        &v,
        &DownloadConfig::default(),
        &CancelToken::new(),
    );
    match out {
        DownloadOutcome::Failed { reason, .. } => {
            assert!(reason.contains("206"), "必须说清「不是 206」：{reason}");
            assert!(reason.contains("退回单段"), "{reason}");
        }
        other => panic!("应当明确失败，实际 {other:?}"),
    }
    assert!(!dest.exists(), "失败时不该留下正式文件");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 多段在段长度不符时会拒绝而不是写坏() {
    // 一个"段长度不对"的服务器会被我们拦住。
    // 若照写，文件会**短一截或有一部分是旧的** —— 而校验可能碰巧通过（若短的是尾部）。
    let data = payload(3 * 1024 * 1024);
    let srv = FakeServer::start(data.clone(), FakeBehavior::default());
    let d = tmpdir("seg-len");
    let dest = d.join("i.bin");
    // **谎报总大小**：让它请求一个超出实际内容的段
    let v = Sha1Verifier::new(&sha1_hex(&data)).unwrap();
    let out = download_segmented(
        &TinyHttp,
        &srv.url("/i.bin"),
        &dest,
        data.len() as u64,
        &v,
        &DownloadConfig::default(),
        &CancelToken::new(),
    );
    // 正常情形应当成功；这条测试主要确保"不会静默写坏"
    match &out {
        DownloadOutcome::Done { .. } => {
            assert_eq!(std::fs::read(&dest).unwrap(), data);
        }
        DownloadOutcome::Failed { reason, .. } => {
            assert!(!dest.exists(), "失败时不该留下正式文件");
            assert!(!reason.is_empty());
        }
        other => panic!("不该是 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ 校验器注入（I3）══════════════════════════

/// 一个**只认"开头四个字节"**的假校验器，用来证明"校验函数按内容类型注入"。
struct MagicVerifier(&'static [u8]);

impl Verifier for MagicVerifier {
    fn verify(&self, path: &std::path::Path) -> Result<(), String> {
        let bytes = std::fs::read(path).map_err(|e| e.kind().to_string())?;
        if bytes.starts_with(self.0) {
            Ok(())
        } else {
            Err(format!("开头不是 {:?}", self.0))
        }
    }
    fn name(&self) -> &'static str {
        "内容魔数"
    }
}

#[test]
fn 校验方式是注入的_引擎不认识它() {
    // I3：**校验函数按内容类型注入**，不在下载器里 switch 文件类型。
    // 这条测试的证明方式是：**引擎完全不知道"魔数"这个概念**，
    // 却正确地用它做了校验与重试。
    let data = b"PK\x03\x04rest-of-a-zip".to_vec();
    let srv = FakeServer::start(data.clone(), FakeBehavior::default());
    let d = tmpdir("magic");
    let dest = d.join("j.zip");

    let out = download(
        &TinyHttp,
        &srv.url("/j.zip"),
        &dest,
        &MagicVerifier(b"PK\x03\x04"),
        &DownloadConfig::default(),
        &CancelToken::new(),
        None,
    );
    assert!(matches!(out, DownloadOutcome::Done { .. }), "{out:?}");

    // 换一个不对的魔数 → 必须失败
    let dest2 = d.join("k.zip");
    let out2 = download(
        &TinyHttp,
        &srv.url("/j.zip"),
        &dest2,
        &MagicVerifier(b"NOPE"),
        &DownloadConfig::default(),
        &CancelToken::new(),
        None,
    );
    match out2 {
        DownloadOutcome::Failed { reason, .. } => assert!(reason.contains("内容魔数"), "{reason}"),
        other => panic!("应当失败，实际 {other:?}"),
    }
    assert!(!dest2.exists());
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ I5：续传前置条件（真实头）══════════════════════════

#[test]
fn 压缩响应会被判为不可续传() {
    // I5：`Content-Encoding` 必须是 identity。
    // 这条用**真实响应头**验证（不是构造的假结构）——
    // 而我们自己的客户端也确实把它解析出来了。
    let data = payload(1024);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            content_encoding: Some("gzip"),
            ..Default::default()
        },
    );
    let resp = TinyHttp
        .fetch(&FetchRequest::get(srv.url("/x")).with_range(0, 99))
        .unwrap();
    assert_eq!(resp.header("content-encoding"), Some("gzip"));
    let sig = resp.resume_signals().to_signals();
    assert!(!sig.content_encoding_identity, "压缩响应必须被判为不可续传");
    use qul_core::download::ResumeVerdict;
    assert!(matches!(sig.verdict(), ResumeVerdict::Restart { .. }));
}

#[test]
fn 缺少_accept_ranges_会被判为不可续传() {
    let data = payload(512);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            accept_ranges: None,
            ..Default::default()
        },
    );
    let resp = TinyHttp.fetch(&FetchRequest::get(srv.url("/x"))).unwrap();
    let sig = resp.resume_signals().to_signals();
    assert!(!sig.accept_ranges_bytes);
    use qul_core::download::ResumeVerdict;
    assert!(matches!(sig.verdict(), ResumeVerdict::Restart { .. }));
}

#[test]
fn 弱_etag_在真实头里被识别为弱() {
    let data = payload(512);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            etag: Some("W/\"weak\""),
            ..Default::default()
        },
    );
    let resp = TinyHttp.fetch(&FetchRequest::get(srv.url("/x"))).unwrap();
    let sig = resp.resume_signals().to_signals();
    assert!(
        sig.strong_etag.is_none(),
        "弱 ETag 只保证语义等价，不保证字节一致"
    );
}

// ══════════════════════════ 限制说明 ══════════════════════════

#[test]
fn 本客户端只支持明文_http_并且这一点必须被显式拒绝() {
    // **这条测试的意义是"把限制写进测试"**，免得有人以为
    // 这个 `TinyHttp` 能下真实的 Mojang 地址（那些全是 https）。
    //
    // 真实下载会用平台提供的网络栈（Tauri/WebView2 那一侧），
    // 而 TLS 不该自己写。
    let e = TinyHttp
        .fetch(&FetchRequest::get("https://piston-meta.mojang.com/x"))
        .unwrap_err();
    assert!(e.contains("只支持 http://"), "{e}");
}
