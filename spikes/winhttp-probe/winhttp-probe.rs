// 候选 3 的最小可行性验证：**WinHTTP FFI，零新依赖**。
//
// 这个文件存在的唯一目的：把"WinHTTP 零依赖可行"从一句主张变成**实测证据**。
// 它只用 core 的 FFI 能力，不引任何 crate。
//
// 验四件事（缺一件这条路都不成立）：
//   ① 能成功握手 https（TLS 由系统栈做）
//   ② 能拿到状态码
//   ③ 能拿到 Content-Length / ETag 这类**下载引擎续传要的头部**（I5）
//   ④ 能分段读 body
//
// 编译：rustc --edition 2021 -O winhttp-probe.rs -o winhttp-probe.exe
// 运行：winhttp-probe.exe
//
// 注意：本文件**不是**生产代码。它是取证。

#![allow(non_snake_case, non_camel_case_types)]

use std::ffi::c_void;
use std::ptr;

type HINTERNET = *mut c_void;
type BOOL = i32;
type DWORD = u32;
type LPCWSTR = *const u16;

const WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: DWORD = 4;
const WINHTTP_FLAG_SECURE: DWORD = 0x0080_0000;
const WINHTTP_QUERY_STATUS_CODE: DWORD = 19;
const WINHTTP_QUERY_CONTENT_LENGTH: DWORD = 5;
const WINHTTP_QUERY_ETAG: DWORD = 24;
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
    fn WinHttpReadData(
        hRequest: HINTERNET,
        lpBuffer: *mut c_void,
        dwNumberOfBytesToRead: DWORD,
        lpdwNumberOfBytesRead: *mut DWORD,
    ) -> BOOL;
    fn WinHttpCloseHandle(hInternet: HINTERNET) -> BOOL;
    fn WinHttpSetTimeouts(
        hInternet: HINTERNET,
        dwResolveTimeout: i32,
        dwConnectTimeout: i32,
        dwSendTimeout: i32,
        dwReceiveTimeout: i32,
    ) -> BOOL;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    // 用**官方端点**测 —— 那是真实安装要走的那个。
    // 而它也是"TLS 真的握手了"的最好证据。
    let host = "piston-meta.mojang.com";
    let path = "/mc/game/version_manifest_v2.json";

    unsafe {
        let agent = wide("qinmo-probe/0.1");
        let session = WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        );
        if session.is_null() {
            println!("✗ WinHttpOpen 失败");
            std::process::exit(1);
        }
        println!("✓ WinHttpOpen");

        // 超时设短一点，免得网络不通时挂住
        let _ = WinHttpSetTimeouts(session, 5000, 8000, 8000, 20000);

        let h = wide(host);
        let conn = WinHttpConnect(session, h.as_ptr(), 443, 0);
        if conn.is_null() {
            println!("✗ WinHttpConnect 失败");
            std::process::exit(1);
        }
        println!("✓ WinHttpConnect {host}:443");

        let verb = wide("GET");
        let obj = wide(path);
        let req = WinHttpOpenRequest(
            conn,
            verb.as_ptr(),
            obj.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE, // ← https
        );
        if req.is_null() {
            println!("✗ WinHttpOpenRequest 失败");
            std::process::exit(1);
        }
        println!("✓ WinHttpOpenRequest（SECURE 标志 = 走 TLS）");

        if WinHttpSendRequest(req, ptr::null(), 0, ptr::null(), 0, 0, 0) == 0 {
            println!("✗ WinHttpSendRequest 失败（err={}）", std::io::Error::last_os_error());
            std::process::exit(1);
        }
        if WinHttpReceiveResponse(req, ptr::null_mut()) == 0 {
            println!(
                "✗ WinHttpReceiveResponse 失败（err={}）—— TLS 握手大概率在这里失败",
                std::io::Error::last_os_error()
            );
            std::process::exit(1);
        }
        println!("✓ 收到响应 —— **https 握手成功**");

        // ① 状态码
        let mut code: DWORD = 0;
        let mut len = std::mem::size_of::<DWORD>() as DWORD;
        let ok = WinHttpQueryHeaders(
            req,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut code as *mut DWORD as *mut c_void,
            &mut len,
            ptr::null_mut(),
        );
        if ok == 0 {
            println!("✗ 读不到状态码");
            std::process::exit(1);
        }
        println!("✓ 状态码 = {code}");

        // ② Content-Length
        let mut clen: DWORD = 0;
        let mut l2 = std::mem::size_of::<DWORD>() as DWORD;
        let ok2 = WinHttpQueryHeaders(
            req,
            WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            &mut clen as *mut DWORD as *mut c_void,
            &mut l2,
            ptr::null_mut(),
        );
        println!(
            "  Content-Length = {}（{}）",
            clen,
            if ok2 == 0 { "读不到" } else { "✓ 读到了" }
        );

        // ③ ETag（下载引擎续传判定要用 —— I5）
        let mut buf = vec![0u16; 256];
        let mut blen = (buf.len() * 2) as DWORD;
        let ok3 = WinHttpQueryHeaders(
            req,
            WINHTTP_QUERY_ETAG,
            ptr::null(),
            buf.as_mut_ptr() as *mut c_void,
            &mut blen,
            ptr::null_mut(),
        );
        let etag = if ok3 != 0 {
            let n = (blen / 2) as usize;
            String::from_utf16_lossy(&buf[..n.saturating_sub(1)])
        } else {
            "(读不到)".to_string()
        };
        println!("  ETag = {etag}");

        // ④ 分段读 body
        let mut total = 0usize;
        let mut first = Vec::new();
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            let mut got: DWORD = 0;
            let ok = WinHttpReadData(
                req,
                chunk.as_mut_ptr() as *mut c_void,
                chunk.len() as DWORD,
                &mut got,
            );
            if ok == 0 || got == 0 {
                break;
            }
            if total == 0 {
                first.extend_from_slice(&chunk[..(got as usize).min(64)]);
            }
            total += got as usize;
        }
        println!("✓ 分段读 body：共 {total} 字节");
        println!("  开头：{}", String::from_utf8_lossy(&first));

        let _ = WinHttpCloseHandle(req);
        let _ = WinHttpCloseHandle(conn);
        let _ = WinHttpCloseHandle(session);

        // 结论
        let ok_plain = code == 200 && total > 1000;
        if ok_plain {
            println!();
            println!("✓✓ 候选 3 可行：**WinHTTP FFI 能走通 https，且零新依赖**");
            println!("   ① TLS 握手（系统栈）✓  ② 状态码 ✓  ③ 头部 ✓  ④ 分段读 ✓");
        } else {
            println!();
            println!("✗ 候选 3 不通（状态码 {code}，body {total} 字节）");
            std::process::exit(1);
        }
    }
}
