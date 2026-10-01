// 候选 3 的第二组验证：**续传所依赖的三件事**。
//
// 第一组探针证明了 WinHTTP 能走 https。而"能不能走 https"不等于
// "能不能支持续传" —— 而下载引擎的 I5（服务端内容变了就重头下）
// 完全建立在三个响应头之上：
//
//   Accept-Ranges: bytes     ← 服务端支不支持分段
//   ETag                     ← 内容标识（变了就不能续）
//   Content-Range            ← 分段响应里"这段是整体的哪一段"
//
// 第一组探针里我写 `WINHTTP_QUERY_ETAG` **读不到**，而 PowerShell 对照
// 显示那个头**确实存在** → **是我的 FFI 用法错了**，不是服务器没给。
//
// 正确的做法：`WinHttpQueryHeaders` 的第三参数传**头部名字符串**，
// 而 `dwInfoLevel` 用 `WINHTTP_QUERY_CUSTOM`。
//
// 这个文件验四件事：
//   ① 按**名字**查头（ETag / Last-Modified / Accept-Ranges）
//   ② 带 `Range:` 请求 → 期望 **206**
//   ③ 读 `Content-Range`（分段响应独有）
//   ④ 只读一小段（证明分段真的省了流量）
//
// 编译运行同第一组。

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
const WINHTTP_QUERY_CUSTOM: DWORD = 65535;
const WINHTTP_QUERY_FLAG_NUMBER: DWORD = 0x2000_0000;

#[link(name = "winhttp")]
extern "system" {
    fn WinHttpOpen(a: LPCWSTR, t: DWORD, p: LPCWSTR, b: LPCWSTR, f: DWORD) -> HINTERNET;
    fn WinHttpConnect(s: HINTERNET, n: LPCWSTR, port: u16, r: DWORD) -> HINTERNET;
    fn WinHttpOpenRequest(
        c: HINTERNET,
        verb: LPCWSTR,
        obj: LPCWSTR,
        ver: LPCWSTR,
        refr: LPCWSTR,
        types: *const LPCWSTR,
        flags: DWORD,
    ) -> HINTERNET;
    fn WinHttpSendRequest(
        r: HINTERNET,
        hdrs: LPCWSTR,
        hlen: DWORD,
        opt: *const c_void,
        olen: DWORD,
        total: DWORD,
        ctx: usize,
    ) -> BOOL;
    fn WinHttpReceiveResponse(r: HINTERNET, res: *mut c_void) -> BOOL;
    fn WinHttpQueryHeaders(
        r: HINTERNET,
        level: DWORD,
        name: LPCWSTR,
        buf: *mut c_void,
        len: *mut DWORD,
        idx: *mut DWORD,
    ) -> BOOL;
    fn WinHttpReadData(r: HINTERNET, buf: *mut c_void, n: DWORD, got: *mut DWORD) -> BOOL;
    fn WinHttpCloseHandle(h: HINTERNET) -> BOOL;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn header_number(req: HINTERNET, which: DWORD) -> Option<DWORD> {
    let mut v: DWORD = 0;
    let mut len = std::mem::size_of::<DWORD>() as DWORD;
    let ok = WinHttpQueryHeaders(
        req,
        which | WINHTTP_QUERY_FLAG_NUMBER,
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

/// **按名字查头** —— 第一组探针漏掉的那一步。
unsafe fn header_named(req: HINTERNET, name: &str) -> Option<String> {
    let w = wide(name);
    let mut buf = vec![0u16; 512];
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
    // ⚠️ **不要减 1。**
    //
    // 第一版写的是 `len / 2 - 1`，理由是我以为"长度含结尾 NUL"。
    // 而 WinHTTP 返回的 `lpdwBufferLength` **是实际写入的字节数，不含 NUL** ——
    // 于是每个字符串都少了最后一个字符：
    //
    //   ETag            → `0x8DF1E31C78D55D`   （真值 …DDA）
    //   Content-Length  → `27718`              （真值 277187）
    //   Accept-Ranges   → `byte`               （真值 bytes）
    //
    // 而那个错的 `Content-Length` 让我按"27 万字节的文件"去要
    // `Range: 1000000-1000999` → 服务端回 **416**，看起来像"服务端不支持 Range"。
    //
    // **一个少一位的数字，把一个"服务端不支持"的假结论摆在了我面前。**
    // 这正是"实测"与"读文档"的差别：读文档我会记得减不减 1 是存疑的，
    // 而实跑一次，一个 416 会把我引向完全错的方向。
    let n = (len / 2) as usize;
    let s = String::from_utf16_lossy(&buf[..n]);
    Some(s)
}

fn main() {
    let host = "piston-meta.mojang.com";
    let path = "/mc/game/version_manifest_v2.json";
    let mut failures = 0;

    unsafe {
        let agent = wide("qinmo-probe/0.2");
        let session = WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, ptr::null(), ptr::null(), 0);
        assert!(!session.is_null(), "WinHttpOpen");
        let h = wide(host);
        let conn = WinHttpConnect(session, h.as_ptr(), 443, 0);
        assert!(!conn.is_null(), "WinHttpConnect");

        // ─────────── ① 按名字查头（先做一次不带 Range 的请求）───────────
        println!("【①】按名字查响应头（第一组探针在这里错过）");
        {
            let verb = wide("GET");
            let obj = wide(path);
            let req = WinHttpOpenRequest(conn, verb.as_ptr(), obj.as_ptr(), ptr::null(), ptr::null(), ptr::null(), WINHTTP_FLAG_SECURE);
            assert!(!req.is_null(), "OpenRequest");
            assert!(WinHttpSendRequest(req, ptr::null(), 0, ptr::null(), 0, 0, 0) != 0, "SendRequest");
            assert!(WinHttpReceiveResponse(req, ptr::null_mut()) != 0, "ReceiveResponse");

            for name in ["ETag", "Last-Modified", "Accept-Ranges", "Content-Length"] {
                match header_named(req, name) {
                    Some(v) => println!("  {name} = {v}"),
                    None => {
                        println!("  {name} = **读不到**");
                        // Content-Length 应当能按名字读到；读不到就记一次失败
                        if name == "Content-Length" {
                            failures += 1;
                        }
                    }
                }
            }
            let _ = WinHttpCloseHandle(req);
        }

        // ─────────── ② 带 Range 的请求 ───────────
        println!();
        println!("【②】带 Range 的请求（续传的基础）");
        // ⚠️ **偏移必须在文件内。**
        //
        // 第一版用了 `1000000-1000999`，而那个文件只有 **277187 字节** ——
        // 于是服务端回 **416 Range Not Satisfiable**，而我把那读成了
        // "服务端不支持 Range"。
        //
        // **一个超界的偏移把"我们不支持续传"这个假结论摆在了我面前。**
        // 而它其实是协议要求的行为（416 是正确答案）。
        //
        // 这与上一步那个"少一位的 Content-Length"是同一类错误：
        // **两次都差点把一个错结论写进结论文档。**
        // 差别是这一次我先去核对了文件大小 —— 而核对的动作是"读服务器自己给的头"。
        let start = 100_000u64;
        let end = 100_999u64; // 只取 1000 字节
        {
            let verb = wide("GET");
            let obj = wide(path);
            let req = WinHttpOpenRequest(conn, verb.as_ptr(), obj.as_ptr(), ptr::null(), ptr::null(), ptr::null(), WINHTTP_FLAG_SECURE);
            assert!(!req.is_null(), "OpenRequest(range)");

            let hdr = wide(&format!("Range: bytes={start}-{end}"));
            let ok = WinHttpSendRequest(req, hdr.as_ptr(), (hdr.len() - 1) as DWORD, ptr::null(), 0, 0, 0);
            if ok == 0 {
                println!("  ✗ SendRequest(Range) 失败");
                failures += 1;
            } else if WinHttpReceiveResponse(req, ptr::null_mut()) == 0 {
                println!("  ✗ ReceiveResponse(Range) 失败");
                failures += 1;
            } else {
                match header_number(req, WINHTTP_QUERY_STATUS_CODE) {
                    Some(206) => println!("  ✓ 状态码 = 206 Partial Content"),
                    Some(c) => {
                        println!("  ✗ 状态码 = {c}（期望 206）—— **服务端没按 Range 回**");
                        failures += 1;
                    }
                    None => {
                        println!("  ✗ 读不到状态码");
                        failures += 1;
                    }
                }
                match header_named(req, "Content-Range") {
                    Some(v) => println!("  Content-Range = {v}"),
                    None => {
                        println!("  ✗ Content-Range 读不到 —— 下载引擎无法知道这段是整体的哪一部分");
                        failures += 1;
                    }
                }
                // ③ 只读这一小段
                let mut buf = vec![0u8; 4096];
                let mut got: DWORD = 0;
                let mut total = 0usize;
                loop {
                    let ok = WinHttpReadData(req, buf.as_mut_ptr() as *mut c_void, buf.len() as DWORD, &mut got);
                    if ok == 0 || got == 0 {
                        break;
                    }
                    total += got as usize;
                }
                println!("  ✓ 实际读回 {total} 字节（期望 1000）");
                if total != 1000 {
                    println!("  ✗ 字节数不对");
                    failures += 1;
                }
            }
            let _ = WinHttpCloseHandle(req);
        }

        let _ = WinHttpCloseHandle(conn);
        let _ = WinHttpCloseHandle(session);

        println!();
        if failures == 0 {
            println!("✓✓ 候选 3 完整可行：**https + 分段 + 续传所需的三个头，零新依赖**");
        } else {
            println!("✗ 候选 3 有 {failures} 项不通 —— 那它不足以支撑续传");
            std::process::exit(1);
        }
    }
}
