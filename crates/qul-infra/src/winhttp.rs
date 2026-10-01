//! # WinHTTP 传输（M3 · **零新依赖的 https**）
//!
//! ## 它为什么存在
//!
//! 上一轮的实测确认：官方端点全是 `https://`，而工作区 `Cargo.lock`
//! **只有 18 个包、零网络依赖**。所以"真实安装"需要一个 TLS 通道，
//! 而那是一个**要决定的事**（见 `spikes/winhttp-probe/TLS候选对照.md`）。
//!
//! 本文件是那个决定的**候选 C 的可运行实现**：直接用 Windows 自带的
//! `winhttp.dll`，于是
//!
//! | | |
//! |---|---|
//! | 新增 crate | **0** |
//! | C 工具链 | **不需要** |
//! | 许可证变化 | **无**（系统 DLL） |
//! | TLS 由谁维护 | **Windows 更新** |
//!
//! ## 🔴 它的边界，写在最前面
//!
//! **只支持 Windows。** 而这不是"将就"：方案 §7 的预算与 §11.5 的端点清单
//! **本来就只针对 Windows**（Tauri 2 的 Windows 目标是唯一目标平台）。
//! 所以本模块在非 Windows 上**不存在** —— 用 `cfg` 挡住，
//! 而不是提供一个"能编译但会失败"的版本。
//!
//! ## 三条纪律（都来自下载引擎的契约）
//!
//! | 纪律 | 不这么做会怎样 |
//! |---|---|
//! | **`Content-Range` / `Accept-Ranges` / `ETag` / `Last-Modified` 必须如实带回去** | 续传（I5）**静默退化成全量重下** |
//! | **`Range` 与 `If-Match` / `If-Unmodified-Since` 必须真的发出去** | 同上，而且更坏：服务端给了 206 而我们当成 200 |
//! | **超时必须设**（而不是用系统默认） | 一个不响应的服务器会让安装**永远挂在那里**，而 UI 上没有"取消"以外的出路 |
//!
//! ## ⚠️ 而这里有一个**必须记下来的实测教训**
//!
//! 探针第一版读 `Content-Length` 得到 `27718`，而真值是 `277187` ——
//! 因为 `WinHttpQueryHeaders` 返回的 `lpdwBufferLength` **不含结尾 NUL**，
//! 而我多减了 1。那个错的长度让我按"27 万字节的文件"去要一个 100 万偏移的
//! `Range`，于是服务端回 **416**，看起来像"服务端不支持 Range"。
//!
//! **一个少一位的数字，把一个"能力缺失"的假结论摆在了我面前。**
//! 所以本模块的每一处长度处理都有注释说明"减不减 1"。

#![cfg(windows)]
// ⚠️ **这个 allow 是刻意的。**
//
// FFI 类型别名（`HINTERNET` / `BOOL` / `DWORD` / `LPCWSTR`）**刻意与 Windows API
// 的名字逐字一致** —— 因为本文件的每一处都要能对着 Microsoft 的文档读。
// 改成 `Hinternet` / `Dword` 会让"这个参数是什么"变成要来回翻译的事，
// 而在 FFI 里那种翻译成本直接变成 bug。
#![allow(clippy::upper_case_acronyms)]

use qul_core::http::{FetchRequest, FetchResponse, Transport};
use std::ffi::c_void;
use std::ptr;

type HINTERNET = *mut c_void;
type BOOL = i32;
type DWORD = u32;
type LPCWSTR = *const u16;

const WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: DWORD = 4;
const WINHTTP_FLAG_SECURE: DWORD = 0x0080_0000;
const WINHTTP_QUERY_STATUS_CODE: DWORD = 19;
const WINHTTP_QUERY_CUSTOM: DWORD = 65535;
// ⚠️ `FLAG_NUMBER` 只对**有固定数值语义**的查询有意义（状态码、长度）。
// 对按名字查头（`CUSTOM`）**不能加**，否则会把 "bytes" 这类字符串
// 当成数字解析。
const WINHTTP_QUERY_FLAG_NUMBER: DWORD = 0x2000_0000;

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(
        pszAgentW: LPCWSTR,
        dwAccessType: DWORD,
        pszProxyW: LPCWSTR,
        pszProxyBypassW: LPCWSTR,
        dwFlags: DWORD,
    ) -> HINTERNET;
    fn WinHttpConnect(
        hSession: HINTERNET,
        pswzServerName: LPCWSTR,
        nServerPort: u16,
        dwReserved: DWORD,
    ) -> HINTERNET;
    fn WinHttpOpenRequest(
        hConnect: HINTERNET,
        pwszVerb: LPCWSTR,
        pwszObjectName: LPCWSTR,
        pwszVersion: LPCWSTR,
        pwszReferrer: LPCWSTR,
        ppwszAcceptTypes: *const LPCWSTR,
        dwFlags: DWORD,
    ) -> HINTERNET;
    fn WinHttpSendRequest(
        hRequest: HINTERNET,
        lpszHeaders: LPCWSTR,
        dwHeadersLength: DWORD,
        lpOptional: *const c_void,
        dwOptionalLength: DWORD,
        dwTotalLength: DWORD,
        dwContext: usize,
    ) -> BOOL;
    fn WinHttpReceiveResponse(hRequest: HINTERNET, lpReserved: *mut c_void) -> BOOL;
    fn WinHttpQueryHeaders(
        hRequest: HINTERNET,
        dwInfoLevel: DWORD,
        pwszName: LPCWSTR,
        lpBuffer: *mut c_void,
        lpdwBufferLength: *mut DWORD,
        lpdwIndex: *mut DWORD,
    ) -> BOOL;
    fn WinHttpQueryDataAvailable(
        hRequest: HINTERNET,
        lpdwNumberOfBytesAvailable: *mut DWORD,
    ) -> BOOL;
    fn WinHttpReadData(
        hRequest: HINTERNET,
        lpBuffer: *mut c_void,
        dwNumberOfBytesToRead: DWORD,
        lpdwNumberOfBytesRead: *mut DWORD,
    ) -> BOOL;
    fn WinHttpSetTimeouts(
        hInternet: HINTERNET,
        dwResolveTimeout: i32,
        dwConnectTimeout: i32,
        dwSendTimeout: i32,
        dwReceiveTimeout: i32,
    ) -> BOOL;
    fn WinHttpCloseHandle(hInternet: HINTERNET) -> BOOL;
}

/// 把 Rust 字符串变成宽字符串（含结尾 NUL）。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 一个 RAII 的 WinHTTP 句柄。
///
/// ## 为什么需要它
///
/// 第一版探针里我手写了三次 `WinHttpCloseHandle`，而**任何一条提前 return
/// 都会漏掉它们**。在 FFI 里漏掉句柄不是"内存泄漏"那么轻 ——
/// 一个被泄漏的 WinHTTP 会话会**占着一个到服务器的连接**，
/// 而安装一个版本要开几十个请求。
struct Handle(HINTERNET);

impl Handle {
    fn new(h: HINTERNET, what: &str) -> Result<Self, String> {
        if h.is_null() {
            Err(format!(
                "{what} 失败（系统错误 {}）",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(Self(h))
        }
    }
    fn raw(&self) -> HINTERNET {
        self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

/// **WinHTTP 传输。**
pub struct WinHttpTransport {
    /// 连接超时（毫秒）
    pub connect_ms: i32,
    /// 接收超时（毫秒）
    pub receive_ms: i32,
    /// 跟随重定向（**WinHTTP 只支持有限次自动重定向**，所以自己判）
    pub max_redirects: u8,
    /// 用户代理（**会被服务器与 CDN 看到**）
    pub user_agent: String,
}

impl Default for WinHttpTransport {
    fn default() -> Self {
        Self {
            connect_ms: 15_000,
            // 读超时给宽松：大文件的两个 chunk 之间可能间隔较久。
            // ⚠️ 它**不是**停滞检测 —— 那一层在 `download.rs` 的超时策略里。
            receive_ms: 60_000,
            max_redirects: 5,
            user_agent: "qinmo-launcher/0.1".to_string(),
        }
    }
}

impl WinHttpTransport {
    pub fn new() -> Self {
        Self::default()
    }

    fn fetch_once(&self, req: &FetchRequest) -> Result<FetchResponse, String> {
        let (host, port, path, secure) = split_url(&req.url)?;

        unsafe {
            let agent = wide(&self.user_agent);
            let session = Handle::new(
                WinHttpOpen(
                    agent.as_ptr(),
                    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                    ptr::null(),
                    ptr::null(),
                    0,
                ),
                "WinHttpOpen",
            )?;
            let _ = WinHttpSetTimeouts(
                session.raw(),
                5_000,
                self.connect_ms,
                self.connect_ms,
                self.receive_ms,
            );

            let h = wide(&host);
            let conn = Handle::new(
                WinHttpConnect(session.raw(), h.as_ptr(), port, 0),
                &format!("WinHttpConnect({host}:{port})"),
            )?;

            let verb = wide("GET");
            let obj = wide(&path);
            let flags = if secure { WINHTTP_FLAG_SECURE } else { 0 };
            let request = Handle::new(
                WinHttpOpenRequest(
                    conn.raw(),
                    verb.as_ptr(),
                    obj.as_ptr(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                    flags,
                ),
                "WinHttpOpenRequest",
            )?;

            // ── 请求头 ──
            //
            // ⚠️ **续传的三个头必须真的发出去。** 少发 `Range` 会让服务端
            // 回 200（整份），而我们的调用方以为拿到了 206 ——
            // 那会让"续传"变成"从 0 开始写而以为在追加"。
            let mut headers = String::new();
            if let Some(r) = req.range_header() {
                headers.push_str(&format!("Range: {r}\r\n"));
            }
            if let Some(v) = &req.if_match {
                headers.push_str(&format!("If-Match: {v}\r\n"));
            }
            if let Some(v) = &req.if_unmodified_since {
                headers.push_str(&format!("If-Unmodified-Since: {v}\r\n"));
            }
            for (k, v) in &req.extra_headers {
                headers.push_str(&format!("{k}: {v}\r\n"));
            }
            let hdr = if headers.is_empty() {
                None
            } else {
                Some(wide(&headers))
            };
            let (hp, hl) = match &hdr {
                Some(w) => (w.as_ptr(), (w.len() - 1) as DWORD),
                None => (ptr::null(), 0),
            };

            if WinHttpSendRequest(request.raw(), hp, hl, ptr::null(), 0, 0, 0) == 0 {
                return Err(format!(
                    "发送请求失败（系统错误 {}）",
                    std::io::Error::last_os_error()
                ));
            }
            if WinHttpReceiveResponse(request.raw(), ptr::null_mut()) == 0 {
                return Err(format!(
                    "接收响应失败（系统错误 {}）—— https 握手的失败通常在这里",
                    std::io::Error::last_os_error()
                ));
            }

            let status = query_number(
                request.raw(),
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            )
            .ok_or("读不到状态码")? as u16;

            // ── 头部：**如实带回去**（下载引擎的 I5 依赖它们）──
            //
            // ⚠️ 第一版我写了 `resp = resp.with_headers_from(&resp)` ——
            // **那个方法不存在**（`FetchResponse` 没有 `Clone`，也没有那个方法）。
            // 改成先收集、最后一次性构造，于是不需要克隆。
            let mut collected: Vec<(String, String)> = Vec::new();
            for name in [
                "Content-Length",
                "Content-Range",
                "Accept-Ranges",
                "ETag",
                "Last-Modified",
                "Content-Type",
                "Location",
            ] {
                if let Some(v) = query_named(request.raw(), name) {
                    collected.push((name.to_string(), v));
                }
            }

            // ── body ──
            //
            // ⚠️ **用 `WinHttpQueryDataAvailable` + `WinHttpReadData` 循环，
            // 而不是"读一次"**：一个 chunk 不代表整份。
            // 而"读一次就当完事"的实现会让**大文件静默截断** ——
            // 那种截断随后会被 SHA-1 拦住，表现为"下载的文件坏了"，
            // 而真因是这里少了一个循环。
            let mut body: Vec<u8> = Vec::new();
            // 预分配：有 Content-Length 就先要那么多（**但不超过上限**，
            // 免得一个恶意的巨大数字让我们先分配几 GB）。
            if let Some(n) = query_number(
                request.raw(),
                WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER,
            ) {
                body.reserve((n as usize).min(64 * 1024 * 1024));
            }
            loop {
                let mut avail: DWORD = 0;
                if WinHttpQueryDataAvailable(request.raw(), &mut avail) == 0 {
                    return Err(format!(
                        "查询可读字节失败（系统错误 {}）",
                        std::io::Error::last_os_error()
                    ));
                }
                if avail == 0 {
                    break;
                }
                let want = avail.min(128 * 1024) as usize;
                let mut buf = vec![0u8; want];
                let mut got: DWORD = 0;
                if WinHttpReadData(
                    request.raw(),
                    buf.as_mut_ptr() as *mut c_void,
                    want as DWORD,
                    &mut got,
                ) == 0
                {
                    return Err(format!(
                        "读 body 失败（系统错误 {}）",
                        std::io::Error::last_os_error()
                    ));
                }
                if got == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..got as usize]);
            }

            let mut resp = FetchResponse::new(status, body);
            for (k, v) in collected {
                resp = resp.with_header(k, v);
            }
            Ok(resp)
        }
    }
}

/// `WINHTTP_QUERY_CONTENT_LENGTH`。
///
/// ⚠️ 我第一版给它起名叫 `WINHTTP_QUERY_CUSTOM_NUMBER_CONTENT_LENGTH` ——
/// **那个常量在 WinHTTP 里不存在**，是我把两个概念拼出来的名字。
/// 真实的名字就是 `WINHTTP_QUERY_CONTENT_LENGTH`。
const WINHTTP_QUERY_CONTENT_LENGTH: DWORD = 5;

/// **读一个数值型头部。**
unsafe fn query_number(req: HINTERNET, level: DWORD) -> Option<u32> {
    let mut v: DWORD = 0;
    let mut len = std::mem::size_of::<DWORD>() as DWORD;
    let ok = WinHttpQueryHeaders(
        req,
        level,
        ptr::null(),
        &mut v as *mut DWORD as *mut c_void,
        &mut len,
        ptr::null_mut(),
    );
    if ok == 0 {
        None
    } else {
        Some(v)
    }
}

/// **按名字读一个字符串头。**
///
/// ## ⚠️ 这里有两处必须写下来的细节
///
/// 1. **第三参数给名字时，第二参数必须是 `WINHTTP_QUERY_CUSTOM`。**
///    第一版探针用了 `WINHTTP_QUERY_ETAG`（一个专用常量），
///    结果**读不到 ETag** —— 而 PowerShell 对照显示那个头**确实存在**。
///    是我的用法错了，不是服务器没给。
/// 2. **返回的 `lpdwBufferLength` 不含结尾 NUL** ——所以**不要减 1**。
///    第一版减了 1，于是 `Content-Length` 从 `277187` 变成 `27718`，
///    而那个错的长度让我去要一个超界的 `Range`，服务端回 416，
///    看起来像"服务端不支持 Range"。
unsafe fn query_named(req: HINTERNET, name: &str) -> Option<String> {
    let w = wide(name);
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as DWORD;
    let ok = WinHttpQueryHeaders(
        req,
        WINHTTP_QUERY_CUSTOM,
        w.as_ptr(),
        buf.as_mut_ptr() as *mut c_void,
        &mut len,
        ptr::null_mut(),
    );
    if ok == 0 {
        return None;
    }
    // **不减 1** —— 见上面第 2 点。
    let n = (len / 2) as usize;
    if n == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..n]))
}

/// 拆 URL：`(host, port, path, secure)`。
///
/// 它支持 `http` 与 `https` —— 而 https 正是本模块存在的理由。
pub fn split_url(url: &str) -> Result<(String, u16, String, bool), String> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(format!(
            "只支持 http/https（收到 `{}`）",
            // 脱敏：只回 scheme 与主机名
            match url.split_once("://") {
                Some((s, r)) => {
                    let end = r.find(['/', '?', '#']).unwrap_or(r.len());
                    format!("{s}://{}…", &r[..end])
                }
                None => "(无法解析)".into(),
            }
        ));
    };
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
        None => (authority.to_string(), if secure { 443 } else { 80 }),
    };
    Ok((host, port, path.to_string(), secure))
}

impl Transport for WinHttpTransport {
    fn fetch(&self, req: &FetchRequest) -> Result<FetchResponse, String> {
        // **自己判重定向**，而不是依赖 WinHTTP 的自动策略 ——
        // 因为我们要把 `Location` 拿在手里，且要对环数设上限。
        let mut url = req.url.clone();
        for _ in 0..=self.max_redirects {
            let mut r = req.clone();
            r.url = url.clone();
            let resp = self.fetch_once(&r)?;
            if matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
                let loc = resp
                    .header("location")
                    .ok_or_else(|| format!("{} 没有 Location 头", resp.status))?;
                url = if loc.starts_with("http://") || loc.starts_with("https://") {
                    loc.to_string()
                } else if let Some(rest) = loc.strip_prefix('/') {
                    let (host, port, _, secure) = split_url(&req.url)?;
                    let scheme = if secure { "https" } else { "http" };
                    if (secure && port == 443) || (!secure && port == 80) {
                        format!("{scheme}://{host}{rest}")
                    } else {
                        format!("{scheme}://{host}:{port}{rest}")
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

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────── URL 拆分（**不需要网络**）─────────────────

    #[test]
    fn 拆_https_url() {
        let (h, p, path, s) = split_url("https://a.b/c/d").unwrap();
        assert_eq!(
            (h.as_str(), p, path.as_str(), s),
            ("a.b", 443, "/c/d", true)
        );
    }

    #[test]
    fn 拆_http_url() {
        let (h, p, path, s) = split_url("http://a.b:8080/x").unwrap();
        assert_eq!(
            (h.as_str(), p, path.as_str(), s),
            ("a.b", 8080, "/x", false)
        );
    }

    #[test]
    fn 显式端口覆盖默认() {
        let (_, p, _, _) = split_url("https://a.b:8443/x").unwrap();
        assert_eq!(p, 8443);
    }

    #[test]
    fn 没有路径时是根() {
        let (_, _, path, _) = split_url("https://a.b").unwrap();
        assert_eq!(path, "/");
    }

    #[test]
    fn 不支持的_scheme_被拒绝且脱敏() {
        let e = split_url("ftp://a.b/x?token=SECRET").unwrap_err();
        assert!(e.contains("只支持 http/https"), "{e}");
        assert!(!e.contains("SECRET"), "错误泄露了查询参数：{e}");
    }

    #[test]
    fn 端口不是数字时报错() {
        let e = split_url("http://a.b:abc/x").unwrap_err();
        assert!(e.contains("端口不是数字"), "{e}");
    }

    #[test]
    fn 没有主机名时报错() {
        assert!(split_url("http:///x").is_err());
    }

    // ───────────────── 宽字符串 ─────────────────

    #[test]
    fn 宽字符串以_nul_结尾() {
        let w = wide("ab");
        assert_eq!(w, vec![b'a' as u16, b'b' as u16, 0]);
        assert_eq!(w.last(), Some(&0));
    }

    #[test]
    fn 宽字符串处理非_ascii() {
        let w = wide("中");
        // UTF-16 里"中"是一个码元
        assert_eq!(w.len(), 2);
        assert_eq!(w[1], 0);
    }
}
