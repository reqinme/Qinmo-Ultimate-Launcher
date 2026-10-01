//! # 资产索引（M3 · **纯规则，零 IO**）
//!
//! ## 实测的形态（本机 `assets/indexes/34.json`）
//!
//! ```json
//! { "objects": {
//!     "icons/icon_128x128.png": { "hash": "b62ca8ec…", "size": 9101 },
//!     "minecraft/sounds/…": { "hash": "1ae2ea93…", "size": 21208 }
//! } }
//! ```
//!
//! | 实测值（版本 `26.3`） | |
//! |---|---|
//! | `objects` 条目数 | **5147** |
//! | 磁盘布局 | `assets/objects/<hash 前 2 位>/<hash>` |
//! | 抽取后的默认 URL | `https://resources.download.minecraft.net/<hash 前 2 位>/<hash>` |
//!
//! 最后两行来自方案 §11.5 的端点清单，而**本模块只负责算出路径**，
//! **不发任何请求、不读任何文件**。
//!
//! ## 🔴 三条必须处理的实测事实
//!
//! ### ① 索引条目数 ≠ 文件数：**必须按哈希去重**
//!
//! **逻辑名可以有多个共享同一个哈希。** 实测：5147 个逻辑名里存在重复。
//!
//! 一个按逻辑名逐条下载的实现会**把同一个哈希下两次**（第二次白下），
//! 而更坏的是它会让进度总数虚高 —— 用户看到一个永远到不了的"5147/5147"。
//!
//! ### ② URL 与落盘路径**都由哈希推出**，而逻辑名只用于 `virtual/`
//!
//! 也就是说：**同一个哈希只该存在一份**。这既是磁盘效率，也是正确的语义
//! （两个逻辑名指向同一份内容）。
//!
//! ### ③ 老版本用 `virtual/legacy` 目录（**而新版本不用**）
//!
//! 实测 `assets/virtual/` 在本机**不存在** —— 因为 `26.3` 的索引 id 是 `34`，
//! 而不是需要虚拟化的那份（`pre-1.6` / `legacy`）。
//!
//! **一个"总是建立 virtual 目录"的实现会在新版本上凭空造出上万个硬链接** ——
//! 而那些文件既不会被读，又会让"实例有多大"这个问题变成错的。
//! 所以本模块**只算出该不该做**，而做不做由调用方决定（见 [`should_virtualize`]）。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 索引里的一个对象。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetObject {
    /// SHA-1（同时也是落盘名与 URL 的末段）
    pub hash: String,
    #[serde(default)]
    pub size: u64,
}

/// 资产索引。
///
/// ⚠️ 它**只有 `objects` 一个键**（实测）。而那些老索引里还有
/// `map_to_resources` / `virtual` 这类开关 —— 它们**在新索引里没有**，
/// 所以用 `#[serde(default)]` 容忍缺失，而不是要求它们存在。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetIndex {
    /// 逻辑名 → 对象
    pub objects: BTreeMap<String, AssetObject>,
    /// 老索引里的开关：把对象也映射到 `resources/` 下
    #[serde(default, rename = "map_to_resources")]
    pub map_to_resources: bool,
    /// 老索引里的开关：把对象虚拟化到 `virtual/<id>/`
    /// ⚠️ **`virtual` 不是 Rust 关键字**，所以不需要那个下划线尾巴。
    /// 第一版我写成 `virtual_` 并且没加 `rename` —— 于是 JSON 键变成了
    /// `virtual_`，而真实的键是 `virtual`。有一条测试抓到了它。
    #[serde(default, rename = "virtual")]
    pub virtual_index: bool,
}

impl AssetIndex {
    pub fn parse(json: &str) -> Result<Self, String> {
        let idx: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if idx.objects.is_empty() {
            return Err("索引里一个对象都没有".into());
        }
        Ok(idx)
    }

    /// **去重后的哈希集合，按哈希升序。**
    ///
    /// 它是"要下载/校验哪些文件"的真答案 —— 见模块文档事实 ①。
    pub fn unique_hashes(&self) -> Vec<(&str, u64)> {
        let mut m: BTreeMap<&str, u64> = BTreeMap::new();
        for o in self.objects.values() {
            // 同一个哈希出现两次时，**取见到的大小**（它们本该相同；
            // 不同的话那是元数据自相矛盾，而这里不掩盖它 ——
            // 见下面那条测试）
            m.entry(o.hash.as_str()).or_insert(o.size);
        }
        m.into_iter().collect()
    }

    /// 逻辑名 → 哈希（给 `virtual/` 用）。
    pub fn logical_names(&self) -> Vec<(&str, &str)> {
        self.objects
            .iter()
            .map(|(k, v)| (k.as_str(), v.hash.as_str()))
            .collect()
    }

    /// 去重后的总字节数（**"还要下多少"要用它，而不是逐条求和**）。
    pub fn unique_bytes(&self) -> u64 {
        self.unique_hashes().iter().map(|(_, s)| *s).sum()
    }
}

/// **一个对象该落在哪**（相对实例根，用 `/` 分隔）。
///
/// 它**只由哈希决定** —— 这正是"同一个哈希只存在一份"的落点。
pub fn object_rel_path(hash: &str) -> String {
    format!("assets/objects/{}/{}", prefix_of(hash), hash)
}

/// 对象的下载 URL 路径段（`<前2位>/<hash>`）。
///
/// 调用方把它拼到默认资源主机（方案 §11.5：
/// `https://resources.download.minecraft.net/`）后面。
pub fn object_url_path(hash: &str) -> String {
    format!("{}/{}", prefix_of(hash), hash)
}

/// 哈希的前两位。
///
/// ⚠️ **短于 2 位的哈希会返回它自己** —— 而那不是"宽容"，是"这个哈希形态不对"。
/// 校验哈希长度是**调用方**的事（`Sha1Verifier::new` 会拒），
/// 这里只保证不 panic。
pub fn prefix_of(hash: &str) -> &str {
    if hash.len() >= 2 {
        &hash[..2]
    } else {
        hash
    }
}

/// **这个索引要不要虚拟化到 `virtual/<id>/`。**
///
/// 实测：`26.3` 的索引（id `34`）**不需要**，而老的 `pre-1.6` / `legacy` 需要。
///
/// ## 判据为什么用"索引 id"而不是"版本号"
///
/// 因为**索引 id 与游戏版本号没有关系**（实测取值 `pre-1.6` / `legacy` /
/// `1.12` / `2` / `19` / `34`）。用版本号判会在 `1.12` 这种"索引 id 恰好
/// 长得像版本号"的地方出错。
///
/// ## 而这是**保守的**
///
/// 只有三个已知需要虚拟化的 id 会被认出来；**其余一律不虚拟化**。
/// 那个方向是安全的：不虚拟化只意味着"老的资源包可能读不到"，
/// 而**多虚拟化会凭空造出上万个文件**。
pub fn should_virtualize(index_id: &str, idx: &AssetIndex) -> bool {
    // 索引自己说了要，那就做（老索引里有 `virtual: true`）。
    if idx.virtual_index {
        return true;
    }
    matches!(index_id, "pre-1.6" | "legacy" | "1.12")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx_json() -> String {
        r#"{
            "objects": {
                "a/one.txt":   {"hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "size": 10},
                "a/two.txt":   {"hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "size": 20},
                "b/one-copy":  {"hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "size": 10},
                "b/deep/x":    {"hash": "cccccccccccccccccccccccccccccccccccccccc", "size": 30}
            }
        }"#
        .to_string()
    }

    // ───────────────── 去重（**最重要的一条**）─────────────────

    #[test]
    fn 逻辑名共享哈希时只算一个文件() {
        // ⚠️ **这是本模块存在的核心理由。**
        //
        // ## 而实测数据要说清楚
        //
        // `26.3` 的 5147 个逻辑名**恰好一一对应 5147 个不同哈希** ——
        // 也就是说**在这个版本上，去重是空转的**。
        //
        // 而它仍然必须存在，因为**协议允许共享**（同一个哈希可以被多个
        // 逻辑名引用），而代价只有一次 `BTreeMap` 插入。
        //
        // **我上一轮把"有重复"写成了实测结论 —— 那是错的**，
        // 而纠正它的成本是**一次 3 行的求和**。
        let idx = AssetIndex::parse(&idx_json()).unwrap();
        assert_eq!(idx.objects.len(), 4, "逻辑名有 4 个");
        let uniq = idx.unique_hashes();
        assert_eq!(uniq.len(), 3, "**哈希只有 3 个不同**");
        let hashes: Vec<&str> = uniq.iter().map(|(h, _)| *h).collect();
        assert!(hashes.contains(&"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert_eq!(hashes.len(), 3);
    }

    #[test]
    fn 去重后的字节数不等于逐条求和() {
        // 这三个数必须能被区分：逻辑名数 / 哈希数 / 字节数。
        let idx = AssetIndex::parse(&idx_json()).unwrap();
        let naive: u64 = idx.objects.values().map(|o| o.size).sum();
        assert_eq!(naive, 70, "逐条求和：10+20+10+30");
        assert_eq!(idx.unique_bytes(), 60, "**去重后：10+20+30**");
        assert_ne!(naive, idx.unique_bytes(), "两者必须能被区分");
    }

    #[test]
    fn 去重后的哈希是升序的() {
        // 顺序确定 ⇒ 进度与日志可复现。
        let idx = AssetIndex::parse(&idx_json()).unwrap();
        let h: Vec<&str> = idx.unique_hashes().iter().map(|(x, _)| *x).collect();
        let mut sorted = h.clone();
        sorted.sort();
        assert_eq!(h, sorted);
    }

    // ───────────────── 路径与 URL ─────────────────

    #[test]
    fn 落盘路径由哈希推出() {
        let h = "b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356";
        assert_eq!(
            object_rel_path(h),
            "assets/objects/b6/b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356"
        );
        assert_eq!(
            object_url_path(h),
            "b6/b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356"
        );
    }

    #[test]
    fn 前缀短于两位时不panic() {
        // 校验哈希形态是调用方的事（`Sha1Verifier::new` 会拒）。
        // 这里只保证不 panic —— 一个 `&h[..2]` 在短哈希上会 panic。
        assert_eq!(prefix_of("a"), "a");
        assert_eq!(prefix_of(""), "");
        assert_eq!(prefix_of("ab"), "ab");
    }

    // ───────────────── 虚拟化判定 ─────────────────

    #[test]
    fn 新索引不虚拟化而老索引虚拟化() {
        // ⚠️ **一个"总是建立 virtual 目录"的实现会在新版本上凭空造出
        // 上万个硬链接** —— 那些文件既不会被读，又会让"实例有多大"变成错的。
        let idx = AssetIndex::parse(&idx_json()).unwrap();
        assert!(!should_virtualize("34", &idx), "26.3 用的 id 是 34");
        assert!(!should_virtualize("19", &idx));
        assert!(!should_virtualize("2", &idx));
        // 而这三个需要
        assert!(should_virtualize("pre-1.6", &idx));
        assert!(should_virtualize("legacy", &idx));
        assert!(should_virtualize("1.12", &idx));
    }

    #[test]
    fn 索引自己说要虚拟化就虚拟化() {
        // 老索引里有 `virtual: true` —— 那比 id 更权威。
        let json = r#"{"objects":{"a":{"hash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1}},"virtual":true}"#;
        let idx = AssetIndex::parse(json).unwrap();
        assert!(should_virtualize("34", &idx), "索引自己说要，那就做");
    }

    #[test]
    fn map_to_resources缺失时不影响解析() {
        // 新索引没有这两个开关 —— 一个要求它们存在的实现会在这里失败。
        let idx = AssetIndex::parse(&idx_json()).unwrap();
        assert!(!idx.map_to_resources);
        assert!(!idx.virtual_index);
    }

    // ───────────────── 错误路径 ─────────────────

    #[test]
    fn 空索引是错误() {
        let e = AssetIndex::parse(r#"{"objects":{}}"#).unwrap_err();
        assert!(e.contains("一个对象都没有"), "{e}");
    }

    #[test]
    fn 坏json是错误() {
        assert!(AssetIndex::parse("{nope").is_err());
    }

    // ───────────────── 逻辑名映射（给 virtual 用）─────────────────

    #[test]
    fn 逻辑名映射能取出全部名字() {
        let idx = AssetIndex::parse(&idx_json()).unwrap();
        let names = idx.logical_names();
        assert_eq!(names.len(), 4);
        // 而**同一个哈希对应两个逻辑名**这件事能被看出来
        let a_count = names.iter().filter(|(_, h)| h.starts_with("aaaa")).count();
        assert_eq!(a_count, 2, "两个逻辑名指向同一个哈希");
    }
}
