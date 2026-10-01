//! # HTTP 传输的形状（**纯形状，零 IO**）
//!
//! ## 为什么这一层要有 trait
//!
//! 三条理由，每条都具体：
//!
//! 1. **内核不许碰 IO**（架构测试强制）→ 所以形状在这里，
//!    而"真的发请求"在 `qul-infra`。
//! 2. **下载引擎的竞态必须在 CI 里能复现** —— 一个注入的传输层让
//!    "服务端忽略 Range"、"响应截断"、"校验不匹配"这些情形
//!    **都能被精确制造**，而不必去等真实网络出错。
//! 3. **将来的真实客户端要能换进去** —— 本项目现在用的是
//!    `qul-infra` 里一个基于 `std::net` 的最小 HTTP/1.1 客户端
//!    （零依赖）。等接 Tauri/WebView2 的网络栈时，**换实现不改引擎**。
//!
//! ## ⚠️ 一条来自规格的硬约束（I4）
//!
//! 规格不变量 I4：
//!
//! > **服务端忽略 Range（返回 200 而非 206）时，必须删掉临时文件重头下**，不许接着写
//!
//! 所以 [`FetchResponse::is_partial`] **不是**"看看有没有 content-range 头"这种宽容判断 ——
//! 它只看状态码是不是 **206**。一个返回 200 的响应哪怕带了 `Content-Range` 头，
//! 也**必须**当成"服务端忽略了 Range"处理。
//!
//! 理由：**"接着写"产出的文件长度是对的、内容是错的** ——
//! 而它随后会被校验拦住，表现为"反复校验失败"，
//! **而真正的原因在几百行外的 Range 处理里。**

use std::collections::BTreeMap;

/// 一次请求的形状。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    pub url: String,
    /// 请求的字节区间（`None` = 整份）。**闭区间**，与 HTTP 的 `Range` 语义一致。
    pub range: Option<(u64, u64)>,
    /// **续传用的校验头**（I5）：服务端内容变了就必须重头下。
    ///
    /// 值为 `None` 表示"这是首次下载，没有可用的校验头"。
    pub if_match: Option<String>,
    pub if_unmodified_since: Option<String>,
    /// 附加请求头（**测试需要它来构造"忽略 Range 的服务端"**）
    pub extra_headers: Vec<(String, String)>,
}

impl FetchRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            range: None,
            if_match: None,
            if_unmodified_since: None,
            extra_headers: Vec::new(),
        }
    }

    pub fn with_range(mut self, start: u64, end_inclusive: u64) -> Self {
        self.range = Some((start, end_inclusive));
        self
    }

    /// `Range: bytes=<start>-<end>` 的值。整份请求时为 `None`。
    pub fn range_header(&self) -> Option<String> {
        self.range.map(|(s, e)| format!("bytes={s}-{e}"))
    }
}

/// 一次响应的形状。**只留引擎真正要用的部分** ——
/// 一个完整的 HTTP 响应模型（分块、重定向链、cookie……）在这里是过度设计。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    pub status: u16,
    /// 头名统一成**小写**（HTTP 头名大小写不敏感，而混着大小写会让查找出错）
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl FetchResponse {
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body,
        }
    }

    pub fn with_header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.headers.insert(k.into().to_ascii_lowercase(), v.into());
        self
    }

    pub fn header(&self, k: &str) -> Option<&str> {
        self.headers
            .get(&k.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// **是不是部分内容（206）。**
    ///
    /// ⚠️ **只看状态码**，不看有没有 `Content-Range` 头 ——
    /// 见模块文档（I4）：一个返回 200 的响应哪怕带了那个头，
    /// 也必须当成"服务端忽略了 Range"。
    pub fn is_partial(&self) -> bool {
        self.status == 206
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// 从响应头提炼"可续传性"信号（I5 的输入）。
    ///
    /// **它只做提炼，不做判定** —— 判定在 `crate::download::ServerResumeSignals::verdict`，
    /// 而那个判定是**纯规则**（可穷举测试）。把判定也写在这里会让规则散进 IO 层。
    pub fn resume_signals(&self) -> crate::http::ResumeSignalsRaw {
        crate::http::ResumeSignalsRaw {
            accept_ranges: self.header("accept-ranges").map(|s| s.to_string()),
            content_encoding: self.header("content-encoding").map(|s| s.to_string()),
            content_length: self
                .header("content-length")
                .and_then(|s| s.trim().parse::<u64>().ok()),
            etag: self.header("etag").map(|s| s.to_string()),
            last_modified: self.header("last-modified").map(|s| s.to_string()),
        }
    }

    /// 从响应头读镜像哈希提示（`x-bmclapi-hash`）。
    ///
    /// **它只用于缓存命中判断，不用于完整性校验** ——
    /// 见 `crate::source::MirrorHint` 的类型文档。
    pub fn mirror_hint(&self) -> Option<crate::source::MirrorHint> {
        self.header("x-bmclapi-hash")
            .and_then(crate::source::MirrorHint::parse)
    }
}

/// **可续传性信号的原始形态**（还在头里的样子）。
///
/// 它是"头 → 规则"之间的那一层，而**这一层存在本身就是一条纪律**：
/// 判定必须发生在纯规则那一侧，否则"什么算可续传"会被散在 HTTP 解析代码里，
/// 而那正是最难测的地方。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResumeSignalsRaw {
    pub accept_ranges: Option<String>,
    pub content_encoding: Option<String>,
    pub content_length: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl ResumeSignalsRaw {
    /// 转成纯规则层的判据输入。
    ///
    /// 三处**刻意的严格**：
    ///
    /// | 头 | 严格之处 | 理由 |
    /// |---|---|---|
    /// | `Accept-Ranges` | 必须**恰好是 `bytes`**（不区分大小写） | `none` 与缺失都表示不能续传，而 `bytes, foo` 这种多值形式我们**不猜** |
    /// | `Content-Encoding` | **缺失视为 `identity`** | HTTP 规定未压缩时可以不发这个头；把它当"不是 identity"会让所有正常响应都无法续传 |
    /// | `ETag` | **弱 ETag（`W/"..."`）不算** | 弱 ETag 只保证"语义等价"，**不保证字节一致** —— 而续传需要的正是字节一致 |
    pub fn to_signals(&self) -> crate::download::ServerResumeSignals {
        use crate::download::ServerResumeSignals;
        ServerResumeSignals {
            accept_ranges_bytes: self
                .accept_ranges
                .as_deref()
                .map(|v| v.trim().eq_ignore_ascii_case("bytes"))
                .unwrap_or(false),
            content_encoding_identity: self
                .content_encoding
                .as_deref()
                .map(|v| v.trim().eq_ignore_ascii_case("identity"))
                // **缺失视为 identity**：HTTP 规定未压缩时可以不发这个头
                .unwrap_or(true),
            has_content_length: self.content_length.is_some(),
            strong_etag: self
                .etag
                .as_deref()
                .map(|v| v.trim())
                .filter(|v| !v.starts_with("W/") && !v.starts_with("w/"))
                .map(|v| v.to_string()),
            last_modified: self.last_modified.clone(),
        }
    }
}

/// **传输层接口。** 唯一的实现约束是：**它必须能表达"服务端忽略了 Range"**。
///
/// 也就是说，实现**不许**在"请求了 Range 但拿到 200"时自己帮引擎重试 ——
/// 那个决策属于引擎（I4：**必须删掉临时文件重头下**），
/// 而实现方替它做决定会让那条不变量**无处落地**。
pub trait Transport {
    fn fetch(&self, req: &FetchRequest) -> Result<FetchResponse, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_头格式() {
        assert_eq!(
            FetchRequest::get("http://x")
                .with_range(0, 99)
                .range_header(),
            Some("bytes=0-99".to_string())
        );
        assert_eq!(
            FetchRequest::get("http://x").range_header(),
            None,
            "整份请求无 Range"
        );
    }

    #[test]
    fn 部分内容只看状态码() {
        // I4：一个返回 200 的响应**哪怕带了 Content-Range 头**，
        // 也必须当成"服务端忽略了 Range"。
        let r = FetchResponse::new(200, vec![]).with_header("content-range", "bytes 0-99/1000");
        assert!(!r.is_partial(), "200 永远不是部分内容");

        let p = FetchResponse::new(206, vec![]);
        assert!(p.is_partial());
    }

    #[test]
    fn 头名大小写不敏感() {
        let r = FetchResponse::new(200, vec![]).with_header("Content-Length", "42");
        assert_eq!(r.header("content-length"), Some("42"));
        assert_eq!(r.header("CONTENT-LENGTH"), Some("42"));
        assert_eq!(r.header("nope"), None);
    }

    // ───────────────── I5 的提炼规则 ─────────────────

    #[test]
    fn accept_ranges_必须恰好是_bytes() {
        let ok = ResumeSignalsRaw {
            accept_ranges: Some("bytes".into()),
            content_encoding: None,
            content_length: Some(1),
            etag: Some("\"x\"".into()),
            last_modified: None,
        };
        assert!(ok.to_signals().accept_ranges_bytes);

        for bad in ["none", "Bytes, foo", "", "byte"] {
            let s = ResumeSignalsRaw {
                accept_ranges: Some(bad.into()),
                ..ok.clone()
            };
            assert!(
                !s.to_signals().accept_ranges_bytes,
                "`{bad}` 不该被判为支持 Range（我们**不猜**多值形式）"
            );
        }
        // 缺失也是不支持
        let missing = ResumeSignalsRaw {
            accept_ranges: None,
            ..ok.clone()
        };
        assert!(!missing.to_signals().accept_ranges_bytes);
    }

    #[test]
    fn content_encoding_缺失视为_identity() {
        // HTTP 规定未压缩时可以不发这个头。
        // 把它当"不是 identity"会让**所有正常响应都无法续传** ——
        // 而那会让续传功能事实上不存在。
        let s = ResumeSignalsRaw {
            accept_ranges: Some("bytes".into()),
            content_encoding: None,
            content_length: Some(1),
            etag: Some("\"x\"".into()),
            last_modified: None,
        };
        assert!(s.to_signals().content_encoding_identity);

        for bad in ["gzip", "br", "deflate"] {
            let s2 = ResumeSignalsRaw {
                content_encoding: Some(bad.into()),
                ..s.clone()
            };
            assert!(
                !s2.to_signals().content_encoding_identity,
                "{bad} 压缩过的响应不能续传"
            );
        }
        // 显式 identity 也可以
        let s3 = ResumeSignalsRaw {
            content_encoding: Some("identity".into()),
            ..s
        };
        assert!(s3.to_signals().content_encoding_identity);
    }

    #[test]
    fn 弱_etag_不算强_etag() {
        // 弱 ETag 只保证"语义等价"，**不保证字节一致** ——
        // 而续传需要的正是字节一致。
        let base = ResumeSignalsRaw {
            accept_ranges: Some("bytes".into()),
            content_encoding: None,
            content_length: Some(1),
            etag: Some("W/\"weak\"".into()),
            last_modified: None,
        };
        let sig = base.to_signals();
        assert!(sig.strong_etag.is_none(), "弱 ETag 不该被当成强 ETag");

        // 而强 ETag 可以
        let strong = ResumeSignalsRaw {
            etag: Some("\"strong\"".into()),
            ..base.clone()
        };
        assert_eq!(
            strong.to_signals().strong_etag.as_deref(),
            Some("\"strong\"")
        );
    }

    #[test]
    fn 弱_etag_配上_last_modified_仍然可以续传() {
        // I5 的判据是"强 ETag **或** Last-Modified" ——
        // 所以弱 ETag 只是"那一半不算"，不是"整体不能续传"。
        let s = ResumeSignalsRaw {
            accept_ranges: Some("bytes".into()),
            content_encoding: None,
            content_length: Some(100),
            etag: Some("W/\"weak\"".into()),
            last_modified: Some("Wed, 21 Oct 2026 07:28:00 GMT".into()),
        };
        use crate::download::ResumeVerdict;
        assert_eq!(s.to_signals().verdict(), ResumeVerdict::Ok);
    }

    #[test]
    fn 一个都不能续传的响应会被判为必须重头下() {
        use crate::download::ResumeVerdict;
        let s = ResumeSignalsRaw {
            accept_ranges: None,
            content_encoding: Some("gzip".into()),
            content_length: None,
            etag: None,
            last_modified: None,
        };
        match s.to_signals().verdict() {
            ResumeVerdict::Restart { missing } => {
                assert_eq!(missing.len(), 4, "四项都缺：{missing:?}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn 镜像哈希提示从头里读出来且不可用于校验() {
        let r = FetchResponse::new(200, vec![])
            .with_header("x-bmclapi-hash", "0123456789abcdef0123456789abcdef01234567");
        let h = r.mirror_hint().expect("应当能解析");
        assert!(h.is_usable_for_cache_hit());
        assert!(
            !h.is_usable_for_integrity(),
            "镜像哈希**绝不能**用于完整性校验"
        );
        // 非法形态返回 None 而不是一个坏值
        let bad = FetchResponse::new(200, vec![]).with_header("x-bmclapi-hash", "nope");
        assert!(bad.mirror_hint().is_none());
    }
}
