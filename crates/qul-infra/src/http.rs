//! # 真实的 HTTP 传输（M3 · 它一直缺）
//!
//! ## 🔴 先说清楚这件事的分量
//!
//! 在整个 M1 与 M2 里，`Transport` **只有测试用的实现**（`tests/common/mod.rs`
//! 的 `TinyHttp`）。也就是说：
//!
//! > **"下载引擎"从没联过网。**
//!
//! 那对 M1 是对的（出口条件是"调用链通不通"，用假服务器验更可控），
//! 而**M3 的出口条件是"真实安装"** —— 它必须有真的网络。
//!
//! ## 两条实现纪律（都来自方案 §11.5）
//!
//! > **所有 URL 与哈希一律取自元数据，不硬编码**
//!
//! 所以本模块**不知道任何端点** —— 它只负责"把这条请求发出去、把响应读回来"。
//! 端点从 `version.json` 来，由调用方给。
//!
//! 第二条：**`Content-Range` / `Accept-Ranges` / `ETag` / `Last-Modified`
//! 必须如实带回去** —— 下载引擎的续传判定（I5）完全依赖它们，
//! 而一个"只回状态码与 body"的实现会让续传**静默退化成全量重下**。
//!
//! ## ⚠️ TLS：这是**唯一**一个我不敢替项目决定的地方
//!
//! 官方端点全是 `https://`，所以**没有 TLS 就装不了任何东西**。
//! 而工作区当前**零第三方依赖**（只有 serde 与 thiserror），
//! 引入 TLS 意味着引入一棵依赖树 —— 而那棵树里每个 crate 的许可证
//! 都必须进 `docs/来源记录.md` 的台账。
//!
//! **我对那棵树的许可证没有实测确认，所以不凭记忆选。**
//!
//! 本文件因此实现**全部** HTTP 逻辑（请求构造、重定向、头部解析、
//! 分块与定长读取），而把 **TLS 握手**留成一个注入点：
//!
//! | 实现 | 用途 |
//! |---|---|
//! | [`PlainHttpTransport`] | **真的能跑** —— 用于假服务器、`http://` 代理、以及测试 |
//! | [`NoNetwork`] | 明确地说"我不联网"（离线模式的正确做法） |
//!
//! 于是：
//! - 离线路径**现在就能被验证**（用 `NoNetwork`）；
//! - 假服务器路径**现在就能被验证**（用 `PlainHttpTransport` + `TinyHttp` 式服务器）；
//! - **https:// 的那一步会给出一个明确的错误**，而不是一个含混的失败。
//!
//! 下一轮要做的事因此是**一件有边界的事**：给 `PlainHttpTransport` 加一层 TLS，
//! 而选哪个 TLS 后端是一个需要查许可证的决定。

use qul_core::http::{FetchRequest, FetchResponse, Transport};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// **不联网。**
///
/// ## 为什么这是一个正经的实现而不是"占位"
///
/// 离线模式**需要**它：一条"离线安装"的路径必须能走到位，
/// 而它的正确行为是**明确报错**（而不是悄悄用一个默认的 HTTP 实现）。
///
/// 与 `qul_core::offline::OfflineGate` 同一个思路：
/// **把"关闭"做成一个类型，而不是一个布尔** —— 于是"忘了传实现"
/// 会变成编译错误，而不是一次意外的联网。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoNetwork;

impl Transport for NoNetwork {
    fn fetch(&self, req: &FetchRequest) -> Result<FetchResponse, String> {
        Err(format!(
            "离线模式：不发起任何网络请求（被请求的 URL 是 `{}`）。\
             要下载就先切到在线模式 —— 这是刻意的，不是故障。",
            // ⚠️ **只回显主机名，不回显整条 URL** ——
            // 整条 URL 可能带查询参数里的令牌。
            host_of(&req.url).unwrap_or("(无法解析)")
        ))
    }
}

/// 从 URL 里取主机名（**脱敏用**）。
pub fn host_of(url: &str) -> Option<&str> {
    let after = url.split("://").nth(1)?;
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    Some(&after[..end])
}

/// **明文 HTTP 传输**（`http://`）。
///
/// ## 它真的能用，而这不是"降级"
///
/// 三个真实用途：
///
/// 1. **假服务器**：端到端测试整条下载链路（含续传、重定向、头部解析）；
/// 2. **本地代理**：企业环境里常见的 `http://proxy` 转发；
/// 3. **镜像**：部分镜像站提供 `http://` 端点。
///
/// 而 `https://` 请求会返回一个**说清缺什么**的错误 —— 见模块文档。
pub struct PlainHttpTransport {
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    /// **跟随重定向的上限。**
    ///
    /// 官方端点会 302 到 CDN，所以必须跟随；而上限存在的理由是
    /// **一个重定向环会让进程卡死**，而那种故障的样子是"没反应"。
    pub max_redirects: u8,
}

impl Default for PlainHttpTransport {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            // 读超时给得宽松：大文件的一个 chunk 之间可能间隔较久。
            // ⚠️ 而它**不是**停滞检测 —— 那一层在 `download.rs` 的超时策略里。
            read_timeout: Duration::from_secs(60),
            max_redirects: 5,
        }
    }
}

impl PlainHttpTransport {
    pub fn new() -> Self {
        Self::default()
    }

    fn fetch_once(&self, req: &FetchRequest) -> Result<FetchResponse, String> {
        let (host, port, path) = parse_http_url(&req.url)?;
        let addr = format!("{host}:{port}");
        let stream = TcpStream::connect(&addr)
            .map_err(|e| format!("连不上 {host}:{port}（{}）", e.kind()))?;
        stream
            .set_read_timeout(Some(self.read_timeout))
            .map_err(|e| format!("设置读超时失败：{e}"))?;
        let mut w = stream
            .try_clone()
            .map_err(|e| format!("复制连接句柄失败：{e}"))?;

        // ── 请求 ──
        // 逐行构造，**不拼一个长字符串** —— 那样最容易在缺 `\r\n` 时出错，
        // 而缺一个 `\r\n` 的表现是"服务器不响应"（没有任何错误信息）。
        let mut head = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n");
        head.push_str("Accept-Encoding: identity\r\n");
        head.push_str("Connection: close\r\n");
        // ⚠️ **续传的两个头必须转发**，否则 `download.rs` 的 I5
        //（"服务端内容变了就重头下"）会**静默失效** ——
        // 而那种失效的样子是"续传后文件坏了"，而不是一个明确的错误。
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
        w.write_all(head.as_bytes())
            .map_err(|e| format!("发请求失败：{e}"))?;
        w.flush().map_err(|e| format!("flush 失败：{e}"))?;

        // ── 响应头 ──
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("读状态行失败：{e}"))?;
        let status = parse_status_line(&line)?;

        let mut headers: Vec<(String, String)> = Vec::new();
        loop {
            let mut l = String::new();
            let n = reader
                .read_line(&mut l)
                .map_err(|e| format!("读头部失败：{e}"))?;
            if n == 0 {
                return Err("响应头没有以空行结束".into());
            }
            let t = l.trim_end_matches(['\r', '\n']);
            if t.is_empty() {
                break;
            }
            if let Some((k, v)) = t.split_once(':') {
                headers.push((k.trim().to_string(), v.trim().to_string()));
            }
        }

        // ── body ──
        //
        // ⚠️ **两种读法，而选错会让 body 截断或挂住。**
        //
        // | 情形 | 怎么读 |
        // |---|---|
        // | 有 `Content-Length` | 读**恰好**那么多字节 |
        // | 有 `Transfer-Encoding: chunked` | 按 chunk 读 |
        // | 两者都没有（且 Connection: close） | 读到 EOF |
        let body = if let Some(te) = header_get(&headers, "transfer-encoding") {
            if te.to_ascii_lowercase().contains("chunked") {
                read_chunked(&mut reader)?
            } else {
                read_to_end(&mut reader)?
            }
        } else if let Some(cl) = header_get(&headers, "content-length") {
            let n: usize = cl
                .trim()
                .parse()
                .map_err(|_| format!("Content-Length 不是数字：{cl:?}"))?;
            let mut buf = vec![0u8; n];
            reader
                .read_exact(&mut buf)
                .map_err(|e| format!("body 不足 {n} 字节：{e}"))?;
            buf
        } else {
            read_to_end(&mut reader)?
        };

        // ⚠️ **把权威的头部如实带回去。** 下载引擎的续传判定完全依赖它们，
        // 而一个"只回状态码与 body"的实现会让续传**静默退化成全量重下**。
        let mut resp = FetchResponse::new(status, body);
        for (k, v) in &headers {
            resp = resp.with_header(k, v);
        }
        Ok(resp)
    }
}

impl Transport for PlainHttpTransport {
    fn fetch(&self, req: &FetchRequest) -> Result<FetchResponse, String> {
        if req.url.starts_with("https://") {
            // **明确说清缺什么**，而不是一个含混的连接失败。
            return Err(format!(
                "`https://` 需要一层 TLS，而本项目**刻意还没引任何 TLS 依赖**。\n  \
                 官方端点全是 https，所以真实安装需要它。\n  \
                 这不是故障，而是一个**待决定的事**：TLS 后端（及其许可证）要进台账。\n  \
                 URL 的主机是 `{}`。",
                host_of(&req.url).unwrap_or("(无法解析)")
            ));
        }

        let mut url = req.url.clone();
        for _ in 0..self.max_redirects {
            let mut r = req.clone();
            r.url = url.clone();
            let resp = self.fetch_once(&r)?;
            // 301 / 302 / 303 / 307 / 308
            if matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
                let loc = resp
                    .header("location")
                    .ok_or_else(|| format!("{} 没有 Location 头", resp.status))?;
                url = if loc.starts_with("http://") {
                    loc.to_string()
                } else if let Some(rest) = loc.strip_prefix('/') {
                    // 相对跳转：拼回同一主机的绝对地址
                    let (host, port, _) = parse_http_url(&req.url)?;
                    if port == 80 {
                        format!("http://{host}/{rest}")
                    } else {
                        format!("http://{host}:{port}/{rest}")
                    }
                } else {
                    return Err(format!("不认识的重定向目标：{loc}"));
                };
                continue;
            }
            return Ok(resp);
        }
        Err(format!(
            "重定向超过 {} 次 —— 可能是重定向环，已停下（而不是继续转）",
            self.max_redirects
        ))
    }
}

// ─────────────────────── 解析助手（都有测试）───────────────────────

/// `http://host[:port]/path`
pub fn parse_http_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("只支持 http://（收到 `{}`）", safe_prefix(url)))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err("URL 里没有主机名".into());
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| format!("端口不是数字：{p:?}"))?,
        ),
        None => (authority.to_string(), 80),
    };
    Ok((host, port, path.to_string()))
}

/// 只回显 URL 的 scheme + 主机名（**脱敏**）。
fn safe_prefix(url: &str) -> String {
    match url.split_once("://") {
        Some((s, rest)) => {
            let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            format!("{s}://{}…", &rest[..end])
        }
        None => "(无法解析)".into(),
    }
}

fn parse_status_line(line: &str) -> Result<u16, String> {
    let mut it = line.split_whitespace();
    let _http = it.next().ok_or("状态行是空的")?;
    let code = it.next().ok_or("状态行里没有状态码")?;
    code.parse::<u16>()
        .map_err(|_| format!("状态码不是数字：{code:?}"))
}

fn header_get<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn read_to_end<R: Read>(r: &mut R) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    r.read_to_end(&mut buf)
        .map_err(|e| format!("读 body 失败：{e}"))?;
    Ok(buf)
}

fn read_chunked<R: BufRead>(r: &mut R) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let mut size_line = String::new();
        r.read_line(&mut size_line)
            .map_err(|e| format!("读 chunk 长度失败：{e}"))?;
        let t = size_line.trim();
        // `1a;ext=1` —— 分号后面是扩展，忽略
        let hex = t.split(';').next().unwrap_or("").trim();
        if hex.is_empty() {
            return Err("chunk 长度是空的".into());
        }
        let n = usize::from_str_radix(hex, 16)
            .map_err(|_| format!("chunk 长度不是十六进制：{hex:?}"))?;
        if n == 0 {
            // 末尾还有一个空行（trailer 段）
            let mut tail = String::new();
            let _ = r.read_line(&mut tail);
            break;
        }
        let mut buf = vec![0u8; n];
        r.read_exact(&mut buf)
            .map_err(|e| format!("chunk body 不足 {n} 字节：{e}"))?;
        out.extend_from_slice(&buf);
        // 每个 chunk 之后有一个 CRLF
        let mut crlf = [0u8; 2];
        r.read_exact(&mut crlf)
            .map_err(|e| format!("chunk 后的 CRLF 缺失：{e}"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────── URL 解析 ─────────────────

    #[test]
    fn 解析_http_url() {
        assert_eq!(
            parse_http_url("http://a.b/c/d").unwrap(),
            ("a.b".into(), 80, "/c/d".into())
        );
        assert_eq!(
            parse_http_url("http://a.b:8080/x").unwrap(),
            ("a.b".into(), 8080, "/x".into())
        );
        // 没有路径 ⇒ `/`
        assert_eq!(
            parse_http_url("http://a.b").unwrap(),
            ("a.b".into(), 80, "/".into())
        );
    }

    #[test]
    fn https_被明确拒绝而不是含混失败() {
        let e = parse_http_url("https://piston-meta.mojang.com/x").unwrap_err();
        assert!(e.contains("只支持 http"), "{e}");
        // **错误里必须带脱敏后的前缀**，而不是整条 URL
        assert!(e.contains("https://piston-meta.mojang.com"), "{e}");
    }

    #[test]
    fn 主机名提取() {
        assert_eq!(
            host_of("https://a.b:8443/x?y=1"),
            Some("a.b:8443"),
            "端口也算主机的一部分"
        );
        assert_eq!(host_of("http://a.b"), Some("a.b"));
        assert_eq!(host_of("垃圾"), None);
    }

    #[test]
    fn 脱敏前缀不泄露查询参数() {
        // ⚠️ 查询参数里可能有令牌 —— 所以前缀只到主机名。
        let s = safe_prefix("https://a.b/p?token=SECRET");
        assert!(!s.contains("SECRET"), "{s}");
        assert!(s.contains("a.b"), "{s}");
    }

    // ───────────────── 状态行 ─────────────────

    #[test]
    fn 解析状态行() {
        assert_eq!(parse_status_line("HTTP/1.1 200 OK\r\n").unwrap(), 200);
        assert_eq!(
            parse_status_line("HTTP/1.1 206 Partial Content").unwrap(),
            206
        );
        assert!(parse_status_line("").is_err());
        assert!(parse_status_line("HTTP/1.1 abc OK").is_err());
    }

    // ───────────────── 头部 ─────────────────

    #[test]
    fn 头部名不区分大小写() {
        let h = vec![
            ("Content-Length".to_string(), "5".to_string()),
            ("ETag".to_string(), "\"abc\"".to_string()),
        ];
        assert_eq!(header_get(&h, "content-length"), Some("5"));
        assert_eq!(header_get(&h, "etag"), Some("\"abc\""));
        assert_eq!(header_get(&h, "missing"), None);
    }

    // ───────────────── chunked 读取 ─────────────────

    #[test]
    fn 读_chunked() {
        let raw = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        let mut r = BufReader::new(&raw[..]);
        assert_eq!(read_chunked(&mut r).unwrap(), b"Wikipedia");
    }

    #[test]
    fn 读_chunked_忽略扩展() {
        // `1a;ext=1` 这种形态在真实响应里存在
        let raw = b"4;foo=bar\r\nWiki\r\n0\r\n\r\n";
        let mut r = BufReader::new(&raw[..]);
        assert_eq!(read_chunked(&mut r).unwrap(), b"Wiki");
    }

    #[test]
    fn 畸形_chunk_长度报错而不是静默截断() {
        let raw = b"zz\r\nWiki\r\n0\r\n\r\n";
        let mut r = BufReader::new(&raw[..]);
        let e = read_chunked(&mut r).unwrap_err();
        assert!(e.contains("十六进制"), "{e}");
    }

    #[test]
    fn chunk_body_不足时报错() {
        let raw = b"10\r\nabc\r\n";
        let mut r = BufReader::new(&raw[..]);
        assert!(read_chunked(&mut r).is_err());
    }

    // ───────────────── 离线传输 ─────────────────

    #[test]
    fn no_network_明确报错且已脱敏() {
        let t = NoNetwork;
        let req = FetchRequest::get("https://a.b/secret?token=XYZ");
        let e = t.fetch(&req).unwrap_err();
        assert!(e.contains("离线模式"), "{e}");
        // ⚠️ **不许把查询参数带进错误** —— 那可能是一个令牌
        assert!(!e.contains("XYZ"), "错误信息泄露了查询参数：{e}");
        assert!(e.contains("a.b"), "{e}");
    }

    #[test]
    fn plain_http_对_https_给出说清缺什么的错误() {
        let t = PlainHttpTransport::new();
        let req = FetchRequest::get("https://a.b/x");
        let e = t.fetch(&req).unwrap_err();
        assert!(e.contains("TLS"), "{e}");
        assert!(e.contains("待决定"), "要说清这是一个待决定的事：{e}");
        // 而它**不该**说"网络不可达" —— 那会把人引向错方向
        assert!(!e.contains("连不上"), "{e}");
    }
}
