//! 实例数据布局 —— **"东西放在哪"的通用模型**。
//!
//! 这一层回答的问题是：**一个实例的目录长什么样**。
//! 它**不认识任何具体游戏**——不知道某个文件叫 `mods` 还是 `behaviour_packs`，
//! 也不知道谁依赖谁。
//!
//! ## 为什么这一层值得单独存在
//!
//! 方案 §5.5 定了三层目录（`import/` `persist/` `build/`）。这三层的划分依据是
//! **"数据在重建实例时该怎么办"**，与游戏无关：
//!
//! | 层 | 含义 | 重建时 |
//! |---|---|---|
//! | `import` | 外部导入、只读原始素材 | 保留 |
//! | `persist` | 用户自己产生的、丢了就没了 | **必须保留** |
//! | `build` | 可由声明式配置重建的产物 | 可丢弃重建 |
//!
//! **把这条"重建语义"放进内核，把"哪个游戏的文件放哪"放进 Provider**——
//! 这就是内核与 Provider 的分界线在数据目录上的具体落点。

use serde::{Deserialize, Serialize};

/// 实例目录的三个层。
///
/// 见方案 §5.5 `[INSTANCE-DATA]`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    /// 外部导入的原始素材（整合包解出来的、用户拖进来的）。
    ///
    /// **只读语义**：启动器不改它，只在读取后写进 `build` 或 `persist`。
    Import,
    /// 用户自己的数据：存档、截图、配置改动。
    ///
    /// **丢了就没了**——所以任何"清理"操作都必须先问，且默认不含这一层。
    Persist,
    /// 可由声明式配置重建的产物：由解析结果生成的运行目录。
    ///
    /// **可丢弃**：删掉之后按配置重新生成，内容应当一致。
    Build,
}

impl Layer {
    /// 全部层，顺序即目录的层级顺序（确定性，供遍历与测试）。
    pub const ALL: &'static [Layer] = &[Self::Import, Self::Persist, Self::Build];

    /// 目录名（**契约**：改名等于破坏已有实例）。
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Persist => "persist",
            Self::Build => "build",
        }
    }

    /// **重建实例时，这一层能不能丢。**
    ///
    /// 这是三层划分的**全部理由**，所以它必须是可查询的，
    /// 而不是散落在各处的注释里——否则"清理"逻辑迟早会删错层。
    pub const fn survives_rebuild(self) -> bool {
        !matches!(self, Self::Build)
    }
}

/// 一份相对路径，**只允许描述"在哪一层"**，不携带游戏知识。
///
/// 之所以要类型而不是裸 `String`：`String` 什么都能塞，
/// 于是"某个 Provider 顺手写了个绝对路径或 `..`"这种错会一路漂到磁盘操作里。
/// 这里在构造时就挡住。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelPath(String);

/// 构造 [`RelPath`] 时的错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelPathError {
    #[error("相对路径不能为空")]
    Empty,
    #[error("相对路径不能以 `/` 或 `\\` 开头（那是绝对路径）")]
    Absolute,
    #[error("相对路径不能包含 `..`（会逃出实例目录）")]
    ParentTraversal,
    #[error("相对路径不能包含空字节")]
    Nul,
}

impl RelPath {
    /// 构造一个实例内相对路径。
    ///
    /// **这是安全边界，不是格式校验**：`..` 与绝对路径是解压/安装类操作的
    /// 经典越界途径（方案 §5.8 的安全基线把 zip-slip 列在其中）。
    /// 在这里挡住，比在写文件的那一刻挡住可靠得多。
    pub fn new(raw: impl Into<String>) -> Result<Self, RelPathError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(RelPathError::Empty);
        }
        if raw.contains('\0') {
            return Err(RelPathError::Nul);
        }
        if raw.starts_with('/') || raw.starts_with('\\') {
            return Err(RelPathError::Absolute);
        }
        // Windows 盘符形式（`C:`）也算绝对
        if raw.len() >= 2 && raw.as_bytes()[1] == b':' {
            return Err(RelPathError::Absolute);
        }
        // 按两种分隔符切分后逐段检查，避免 `a/../../b` 这类绕过
        if raw.split(['/', '\\']).any(|seg| seg == "..") {
            return Err(RelPathError::ParentTraversal);
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 只有_build_层可丢() {
        assert!(!Layer::Build.survives_rebuild());
        assert!(Layer::Import.survives_rebuild());
        assert!(Layer::Persist.survives_rebuild(), "用户数据丢了就没了");
    }

    #[test]
    fn 三层目录名是契约() {
        let names: Vec<&str> = Layer::ALL.iter().map(|l| l.dir_name()).collect();
        assert_eq!(names, vec!["import", "persist", "build"]);
    }

    #[test]
    fn 拒绝逃出实例目录的路径() {
        for bad in [
            "../evil",
            "a/../../b",
            "a\\..\\..\\b",
            "/abs",
            "\\abs",
            "C:/abs",
            "C:\\abs",
            "",
            "a\0b",
        ] {
            assert!(RelPath::new(bad).is_err(), "应拒绝: {:?}", bad);
        }
    }

    #[test]
    fn 接受正常的实例内路径() {
        for ok in [
            "a",
            "a/b",
            "a/b/c.txt",
            "a\\b",
            "..a",
            "a/..b",
            "配置/x.json",
        ] {
            assert!(RelPath::new(ok).is_ok(), "应接受: {:?}", ok);
        }
    }
}
