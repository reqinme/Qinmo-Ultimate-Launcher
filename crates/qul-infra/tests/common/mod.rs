//! # 共享的测试脚手架：最小 HTTP 客户端 + 可注入故障的假服务器
//!
//! ## 为什么抽成一个 `common` 模块
//!
//! 因为 `download_e2e.rs` 与 `fault_injection.rs` 都要用它，而
//! **测试脚手架的复制粘贴比生产代码的复制粘贴更危险**：
//! 两份会**各自漂移**，于是"一个文件里的那条测试过了、另一个没测到"——
//! 而那种偏差**没有任何症状**。
//!
//! ## 它为什么不是 Mock
//!
//! 因为要验的不变量**只在真实 HTTP 语义下才成立**：
//! **I1** 依赖文件系统的 rename 语义，**I4** 依赖 HTTP 状态码（200 vs 206），
//! **I5** 依赖真实的响应头。一个 Mock 可以"假装"返回 200，
//! 但它**证明不了**我们的客户端在真实响应下怎么解析头。
//!
//! 所以这里有一个**真的 `TcpListener`**，按字节写 HTTP 响应。
//!
//! ## ⚠️ `TinyHttp` 只支持明文 `http://`
//!
//! 真实下载走 HTTPS，那需要 TLS —— 而 **TLS 不该自己写**。
//! 这个限制有一条**显式断言它的测试**（见 `download_e2e.rs` 末尾），
//! 免得有人以为它能下真实的 Mojang 地址。
#![allow(dead_code)] // 两个测试文件各自用到不同的部分

use qul_core::http::{FetchRequest, FetchResponse, Transport};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

// ══════════════════════════ 最小 HTTP 客户端 ══════════════════════════

/// 一个**故意最小**的 HTTP/1.1 客户端。
///
/// 只做 `GET`，只解析 `Content-Length` 定长响应（**不支持 chunked**）——
/// 因为拿它测试的是"我们自己的下载引擎"，而不是"我们自己的 HTTP 实现"。
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
        if status_line.trim().is_empty() {
            // **服务端直接关连接**是一种真实故障（拒绝服务、崩溃）。
            // 把它报成一个明确的错误，而不是一个"状态行无法解析"的谜题。
            return Err("连接被对端关闭（没有响应）".to_string());
        }
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
            // **对端中途断开**要报成"响应被截断"，而不是一个裸的 IO 错误 ——
            // 前者能被上层归类为可重试，后者只能被当成未知故障。
            reader
                .read_exact(&mut body)
                .map_err(|e| format!("响应体被截断（期望 {len} 字节）：{e}"))?;
        }
        resp.body = body;
        Ok(resp)
    }
}

// ══════════════════════════ 假服务器与故障注入 ══════════════════════════

/// 假服务器的行为开关。**每一种都对应一条不变量或一类真实故障。**
#[derive(Clone)]
pub struct FakeBehavior {
    /// **忽略 Range**（无论收到什么都返回 200 + 整份）→ 验 I4
    pub ignore_range: bool,
    /// 声称的 `Accept-Ranges`
    pub accept_ranges: Option<&'static str>,
    /// 发一个压缩声明（让续传判定失败）→ 验 I5
    pub content_encoding: Option<&'static str>,
    /// 是否发 `ETag`
    pub etag: Option<&'static str>,
    /// 每第 N 次响应体坏一个字节（模拟传输损坏）→ 验校验失败重试
    pub corrupt_every: Option<usize>,
    /// 每第 N 次**只发一半响应体就关连接** → 验截断
    pub truncate_every: Option<usize>,
    /// 每第 N 次返回这个 HTTP 状态码（如 500 / 503）→ 验 5xx 重试
    pub fail_with_every: Option<(usize, u16)>,
    /// 每第 N 次**直接关连接不发任何响应** → 验连接级故障
    pub drop_every: Option<usize>,
    /// 发一个**错误的 `Content-Length`**（比实际体短）→ 验我们的长度校验
    pub wrong_content_length: bool,
    /// 每次响应前 sleep 这么多毫秒 → 验超时（**测试里要设得小**）
    pub delay_ms: u64,
}

impl Default for FakeBehavior {
    fn default() -> Self {
        Self {
            ignore_range: false,
            accept_ranges: Some("bytes"),
            content_encoding: None,
            etag: Some("\"v1\""),
            corrupt_every: None,
            truncate_every: None,
            fail_with_every: None,
            drop_every: None,
            wrong_content_length: false,
            delay_ms: 0,
        }
    }
}

/// 一个**真的 TCP 服务器**，按字节写 HTTP 响应。
pub struct FakeServer {
    pub addr: String,
    pub payload: Arc<Vec<u8>>,
    pub behavior: FakeBehavior,
    pub hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
}

impl FakeServer {
    pub fn start(payload: Vec<u8>, behavior: FakeBehavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("应当能绑定本地端口");
        let addr = listener.local_addr().unwrap().to_string();
        let payload = Arc::new(payload);
        let hits = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));

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

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
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
            if let Some(spec) = v.trim().strip_prefix("bytes=") {
                if let Some((a, bb)) = spec.split_once('-') {
                    let start = a.trim().parse::<u64>().unwrap_or(0);
                    let end = bb.trim().parse::<u64>().unwrap_or(u64::MAX);
                    range = Some((start, end));
                }
            }
        }
    }

    // **连接级故障**：不发任何响应就关连接
    if let Some(every) = b.drop_every {
        if every > 0 && n.is_multiple_of(every) {
            return Ok(()); // 直接 drop
        }
    }
    // **HTTP 级故障**
    if let Some((every, code)) = b.fail_with_every {
        if every > 0 && n.is_multiple_of(every) {
            let head = format!(
                "HTTP/1.1 {code} Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(head.as_bytes())?;
            return Ok(());
        }
    }
    if b.delay_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(b.delay_ms));
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

    let mut body = body;
    // 传输损坏：坏一个字节
    if let Some(every) = b.corrupt_every {
        if every > 0 && n.is_multiple_of(every) && !body.is_empty() {
            body[0] = body[0].wrapping_add(1);
        }
    }
    // **截断**：只发一半（而 Content-Length 说的是全长）
    let declared_len = body.len();
    let truncate = b
        .truncate_every
        .map(|every| every > 0 && n.is_multiple_of(every))
        .unwrap_or(false);

    let mut head = format!(
        "HTTP/1.1 {status} {}\r\n",
        if status == 206 {
            "Partial Content"
        } else {
            "OK"
        }
    );
    // 错误的 Content-Length：说得多、发得少（**这也是截断的一种**，
    // 区别是它由长度字段本身撒谎造成）
    let len_field = if b.wrong_content_length {
        declared_len + 1024
    } else {
        declared_len
    };
    head.push_str(&format!("Content-Length: {len_field}\r\n"));
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

    if truncate {
        let half = body.len() / 2;
        stream.write_all(&body[..half])?;
    } else {
        stream.write_all(&body)?;
    }
    stream.flush()?;
    Ok(())
}

// ══════════════════════════ 工具 ══════════════════════════

pub fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "qul-fi-{tag}-{}-{}",
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

pub fn payload(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i % 251) as u8).collect()
}
