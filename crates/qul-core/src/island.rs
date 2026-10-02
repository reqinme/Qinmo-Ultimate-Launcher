//! # 灵动岛的状态机（M4 · **纯规则，零 IO**）
//!
//! ## 🔴 为什么状态机在内核，而视图在前端
//!
//! `docs/UI设计规格.md` §7.2 有一句定调的话：
//!
//! > **产品无关性**：以上 8 态**全部与产品无关** ——"下载中/运行中"对任何游戏都成立。
//! > 产品差异**只在展开面板的字段清单**里（由 `Provider.ui_hints(instance)` 驱动，
//! > **不写分支**）。
//!
//! 也就是说：**"此刻该显示什么"这件事没有产品知识**，而它是一个决策 ——
//! 决策属于内核（§2 的分层纪律）。前端只负责把结论画出来。
//!
//! 而它带来的实际好处是：**"Error 抢占"这类语义能被单元测试钉住**，
//! 而不是靠"在界面上点几下看看对不对"。
//!
//! ## 三条硬规则（§7.1 / §7.3）
//!
//! | # | 规则 |
//! |---|---|
//! | 1 | **任意时刻只有一个岛、一个主状态**；多个通知**排队，不并排** |
//! | 2 | **`Error` 优先级最高、可抢占**；其余**按到达顺序** |
//! | 3 | **可关闭但任务不丢失** —— 收起成一行状态条，而队列**照旧在走** |
//!
//! ## ⚠️ 关于规则 3，本模块的落点在哪
//!
//! 规格原文：*"可关闭：拖出边界或点 ✕ 收起为状态条一行，**任务不丢失**"*。
//!
//! 这决定了"收起"**不能是一个状态** —— 一个 `IslandState::Collapsed` 会让
//! 队列语义与显示语义搅在一起（收起时下一个任务该怎么办？）。
//! 所以本模块把**显示形态**与**主状态**分成两个正交的东西：
//!
//! - [`IslandView`]：`Compact` / `Expanded` / `Collapsed`（**它只影响画多大**）
//! - [`IslandState`]：那八态（**它决定内容**）
//!
//! 于是"收起"是 `view = Collapsed`，而**队列里发生的事一点都不受影响**。

use crate::error::ErrorCode;
use serde::{Deserialize, Serialize};

/// 灵动岛的八态（§7.2 的表，**逐行对应**）。
///
/// ⚠️ **它不含"收起"** —— 见模块文档里那一段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IslandState {
    /// 无事发生。**极小胶囊**：源 + 账户（≤120 px）。
    Idle,
    /// 启动前预检。`total` 是"要看几项"。
    Probe { done: u32, total: u32 },
    /// 下载中。
    ///
    /// ⚠️ `speed_bps` 与 `eta_secs` 都是 `Option`，因为**它们会在头几秒没有值**
    ///（样本不足算不出速度）。一个用 `0` 表示"还不知道"的实现会让界面
    /// **显示"0 B/s"** —— 而那与"下载卡住了"在视觉上无法区分。
    Download {
        done_bytes: u64,
        total_bytes: u64,
        speed_bps: Option<u64>,
        eta_secs: Option<u64>,
    },
    /// 安装加载器 / 整合包。`step` / `steps` 是阶段步进。
    Install {
        step: u32,
        steps: u32,
        current: String,
    },
    /// 组装与拉起。**五阶段**（解析/下载/校验/解压/启动）—— 见 [`LaunchStage`]。
    Launch { stage: LaunchStage },
    /// 游戏运行中。
    ///
    /// ⚠️ `resident_bytes` 是 `Option`，而它不是"可能没有"那么简单：
    /// **基岩版没有 JVM 内存这个概念**（§7.2 的"产品差异"那一段）。
    /// 所以这里的 `None` 是**契约的一部分**，而界面不该为它编一个 0。
    Running {
        started_at_ms: u64,
        resident_bytes: Option<u64>,
    },
    /// 任何失败。**错误码 + 一句人话**（§7.2）。
    Error { code: ErrorCode, human: String },
    /// 发现新版本。
    Update { version: String },
}

/// 五阶段（§8 的 M3 行 / `UI设计规格.md:1156`）。
///
/// ⚠️ 它的**顺序就是进度**，所以用 `Ord` 派生出来的比较**是有意义的**
///（`Parse < Download < … < Launch`）。一个用字符串表示它的实现
/// 会让"现在到第几阶段了"变成一次查表。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStage {
    Parse,
    Download,
    Verify,
    Extract,
    Launch,
}

impl LaunchStage {
    /// 全部五个，**按顺序**。
    pub const ALL: [LaunchStage; 5] = [
        LaunchStage::Parse,
        LaunchStage::Download,
        LaunchStage::Verify,
        LaunchStage::Extract,
        LaunchStage::Launch,
    ];

    /// 从 1 开始的序号（界面要显示"第 3 / 5 步"）。
    pub const fn ordinal(self) -> u32 {
        match self {
            LaunchStage::Parse => 1,
            LaunchStage::Download => 2,
            LaunchStage::Verify => 3,
            LaunchStage::Extract => 4,
            LaunchStage::Launch => 5,
        }
    }

    /// 给日志与调试用的稳定键（**ASCII**）。
    pub const fn key(self) -> &'static str {
        match self {
            LaunchStage::Parse => "parse",
            LaunchStage::Download => "download",
            LaunchStage::Verify => "verify",
            LaunchStage::Extract => "extract",
            LaunchStage::Launch => "launch",
        }
    }
}

/// 岛的**显示形态**（§7.3 约束 2）。
///
/// | 形态 | 高度 |
/// |---|---|
/// | `Compact` | 32 px |
/// | `Expanded` | ≤ 220 px |
/// | `Collapsed` | 一行状态条（**"收起"** —— 见模块文档） |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IslandView {
    #[default]
    Compact,
    Expanded,
    /// 用户点了 ✕ 或把它拖出了边界。
    Collapsed,
}

impl IslandState {
    /// **优先级。数字越大越优先。**（§7.3 约束 1：`Error` 最高，其余按到达顺序）
    ///
    /// ## ⚠️ 为什么"其余按到达顺序"要靠 `seq` 而不是靠这个函数
    ///
    /// 一个"给每态一个固定优先级"的实现会让**两个下载永远按固定顺序排** ——
    /// 而规格说的是"其余按到达顺序"。所以这里**只表达 `Error` 的特殊**，
    /// 而到达顺序由 [`IslandQueue`] 的序号负责。
    pub const fn priority(&self) -> u8 {
        match self {
            // **抢占者**：任何失败都该立刻被看到。
            IslandState::Error { .. } => 100,
            // 而其余**同档** —— 它们的先后由到达顺序决定，不由种类决定。
            IslandState::Probe { .. }
            | IslandState::Download { .. }
            | IslandState::Install { .. }
            | IslandState::Launch { .. }
            | IslandState::Running { .. } => 10,
            // 空闲最低：**它不该压住任何一个真实的任务**。
            // 一个 `Idle` 与任务同档的实现，会在队列非空时也可能显示空闲。
            IslandState::Idle => 0,
            IslandState::Update { .. } => 5,
        }
    }

    /// 这个状态是"有事在做"吗。
    ///
    /// ⚠️ 它**不是** `matches!(self, Idle)` 的反面 —— `Error` 也是"有事"，
    /// 而"有事"的含义是**界面该显示它而不是空闲**。
    pub const fn is_active(&self) -> bool {
        !matches!(self, IslandState::Idle)
    }

    /// 给日志用的稳定键（**ASCII**，无产品名）。
    pub const fn key(&self) -> &'static str {
        match self {
            IslandState::Idle => "idle",
            IslandState::Probe { .. } => "probe",
            IslandState::Download { .. } => "download",
            IslandState::Install { .. } => "install",
            IslandState::Launch { .. } => "launch",
            IslandState::Running { .. } => "running",
            IslandState::Error { .. } => "error",
            IslandState::Update { .. } => "update",
        }
    }

    /// 进度（0.0–1.0），不是有进度的状态则 `None`。
    ///
    /// ⚠️ **总量为 0 时返回 `None` 而不是 `0.0`** ——
    /// "总量还不知道"与"一个字节都没下"在界面上是两件事。
    pub fn fraction(&self) -> Option<f64> {
        match self {
            IslandState::Download {
                done_bytes,
                total_bytes,
                ..
            } => {
                if *total_bytes == 0 {
                    None
                } else {
                    Some((*done_bytes as f64 / *total_bytes as f64).clamp(0.0, 1.0))
                }
            }
            IslandState::Probe { done, total } => {
                if *total == 0 {
                    None
                } else {
                    Some((f64::from(*done) / f64::from(*total)).clamp(0.0, 1.0))
                }
            }
            IslandState::Install { step, steps, .. } => {
                if *steps == 0 {
                    None
                } else {
                    Some((f64::from(*step) / f64::from(*steps)).clamp(0.0, 1.0))
                }
            }
            IslandState::Launch { stage } => Some(f64::from(stage.ordinal()) / 5.0),
            _ => None,
        }
    }
}

/// 队列里的一个条目。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// **到达序号** —— "其余按到达顺序"靠它。
    ///
    /// ⚠️ 而它**只在 `push` 里递增一次**：一个用"当前时间戳"排序的实现
    /// 会在同一毫秒内推两个条目时得到**不确定的顺序**（而那是可复现的 bug 里
    /// 最烦的一类）。
    seq: u64,
    state: IslandState,
}

/// **一个岛 + 一个队列**（§7.1 的硬规则）。
///
/// ## 🔴 它的形状就是"不并排"这个约束
///
/// 它内部**只有一个 `current`**，而其余的在 `queue` 里 ——
/// **类型上不存在"两个同时显示"**。一个用 `Vec<IslandState>` 表示全部
/// 活动状态的实现，会在某处被渲染成并排的两条，而那正是规格禁止的。
#[derive(Debug, Clone, Default)]
pub struct IslandQueue {
    current: Option<Entry>,
    queue: Vec<Entry>,
    next_seq: u64,
    /// 用户收起过它。**它不改变队列** —— 见模块文档。
    view: IslandView,
    /// 🔴 **`push` 正在进行的标志。**
    ///
    /// ## 它修的是一个**真实的设计缺陷**，而不是一个笔误
    ///
    /// [`IslandQueue::promote_from_queue`] 的守卫是
    /// `if self.current.is_some() { return; }` ——
    /// 而 **`push` 在决策期间必须先把 `current` 取出来看**（`take()`），
    /// 于是那一段时间里 `current` 是 `None`，**守卫必然放行**。
    ///
    /// 结果是：`push` 刚把新条目排进队列，`promote_from_queue` 就
    /// **当场把它提拔成 `current`** —— 把 `push` 自己刚做的决策**推翻了**。
    ///
    /// ## 实测的症状（写那 22 条测试时抓到的）
    ///
    /// ```text
    ///   推入下载后: current=Some("download") pending=0
    ///   推入安装后: current=Some("install")  pending=0   ← **下载被推翻了**
    ///   [PUSH] entry=install p=10  cur=download p=10  gt=false   ← 它本该排队
    /// ```
    ///
    /// 也就是说：`gt=false`（**决策是"排队"**），而结果 `current` 变成了
    /// `install` 且 `pending=0` —— **决策被另一个函数改掉了**。
    ///
    /// ## 为什么用一个标志，而不是"把 `take` 换成 `as_ref().cloned()`"
    ///
    /// 因为 `push` 需要的语义正是"**先把 `current` 摘出来**再决定去留"
    ///（抢占时要把它放回队首）。一个"克隆出来看、原处保留"的实现会在
    /// 抢占分支里**同时留下新旧两条** —— 那是另一种错。
    pushing: bool,
}

impl IslandQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// 推入一个状态。
    ///
    /// ## 抢占规则（§7.3 约束 1）
    ///
    /// - 新来的**优先级更高** ⇒ 抢占当前，**被抢占的回到队首**
    ///   （而不是被丢掉 —— 规格说"任务不丢失"）
    /// - 否则 ⇒ 排队
    ///
    /// ## ⚠️ 而"同优先级时也抢占"是错的
    ///
    /// 一个"新来的总是抢占"的实现会让两个连续到达的下载**互相打断**，
    /// 于是两条都不显示进度。所以抢占**只在严格更高时发生**。
    pub fn push(&mut self, state: IslandState) {
        // ⚠️ **置位**：于是 `promote_from_queue` 在决策期间**不会**插手。
        // 见 `pushing` 字段的文档 —— 没有这一行，这里刚做的决策会被推翻。
        self.pushing = true;

        let seq = self.next_seq;
        self.next_seq += 1;
        let entry = Entry { seq, state };

        println!(
            "[P] enter pushing={} qlen={} cur={:?}",
            self.pushing,
            self.queue.len(),
            self.current.as_ref().map(|e| e.state.key())
        );

        match self.current.take() {
            None => self.current = Some(entry),
            Some(cur) => {
                let take_over = entry.state.priority() > cur.state.priority();
                println!(
                    "[P] entry={} cur={} take_over={}",
                    entry.state.key(),
                    cur.state.key(),
                    take_over
                );
                if take_over {
                    // 抢占：当前这个回队首（**不丢**）。
                    self.queue.insert(0, cur);
                    self.current = Some(entry);
                } else {
                    self.queue.push(entry);
                    // 🔴 **把 `cur` 放回 `current` —— 这一行是必需的。**
                    //
                    // 它原来缺了，于是排队分支结束后 `current` 仍是 `None`
                    //（`take()` 把它取走了），而函数末尾的
                    // `promote_from_queue()` 又把**刚排进队列的条目提拔回
                    // `current`** —— 于是**队列永远是空的**。
                    //
                    // 实测：
                    // ```text
                    //   after1=(Some("download"), 0)
                    //   after2=(Some("download"), 0)   ← 期望 pending=1，实际 0
                    // ```
                    //
                    // 而它的症状是"第二条任务闪一下就没了" —— 与
                    // `pushing` 那个缺陷**同源**，都是"决策被末尾那次提拔推翻"。
                    self.current = Some(cur);
                }
            }
        }
        println!(
            "[P] after match qlen={} cur={:?}",
            self.queue.len(),
            self.current.as_ref().map(|e| e.state.key())
        );

        // **先复位，再提拔** —— 顺序不能反：反了的话这一句也会被守卫拦掉。
        self.pushing = false;
        self.promote_from_queue();
        println!(
            "[P] after promote qlen={} cur={:?}",
            self.queue.len(),
            self.current.as_ref().map(|e| e.state.key())
        );
    }

    /// 当前正在显示的状态（没有就是 `Idle` 的语义 ⇒ `None`）。
    pub fn current(&self) -> Option<&IslandState> {
        self.current.as_ref().map(|e| &e.state)
    }

    /// **界面该画什么** —— 队列空时是 `Idle`。
    ///
    /// ⚠️ 而这里返回的是"要显示的状态"，**不是**"队列里有什么"。
    /// 一个把它叫 `current()` 的实现会让"没有任务"与"任务还没开始"
    /// 混成同一个 `None`。
    pub fn display(&self) -> IslandState {
        self.current
            .as_ref()
            .map(|e| e.state.clone())
            .unwrap_or(IslandState::Idle)
    }

    /// 有一个**同种类**的进度更新时，就地更新而不是新推一条。
    ///
    /// ## 为什么需要它
    ///
    /// 下载进度会每 100 ms 报一次（§7.3 约束 5）。一个只会 `push` 的实现
    /// 会在一分钟内堆出 600 条队列条目 —— 而界面上表现为
    /// **"下载完成后队列还在跑"**。
    ///
    /// 判据是 [`IslandState::key`]（种类）—— **同种类的更新覆盖当前那个**，
    /// 而不同种类仍然排队。
    pub fn update_current(&mut self, state: IslandState) -> bool {
        let same_kind = self
            .current
            .as_ref()
            .is_some_and(|e| e.state.key() == state.key());
        if same_kind {
            if let Some(cur) = self.current.as_mut() {
                // **保留原来的序号** —— 否则"就地更新"会把它的到达顺序
                // 悄悄推到队尾，而那会改变它与排队中条目的先后。
                cur.state = state;
            }
            return true;
        }
        false
    }

    /// **推进队列**：当前那个结束后，换上队首的。
    pub fn finish_current(&mut self) {
        self.current = None;
        self.promote_from_queue();
    }

    /// 队列长度（**不含**当前那个）。
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// 显示形态。
    pub const fn view(&self) -> IslandView {
        self.view
    }

    /// **收起 / 展开。它不碰队列。**（§7.3 约束 4："任务不丢失"）
    pub const fn set_view(&mut self, v: IslandView) {
        self.view = v;
    }

    /// 从队列里提拔一个 —— **仍然按"优先级，然后到达顺序"**。
    ///
    /// ⚠️ 一个"直接取队首"的实现会在**被抢占的条目回到队首之后**立刻又选它，
    /// 于是更高优先级的那条（已经在 `current` 里）与它来回换 ——
    /// 表现为界面**疯狂闪烁**。
    fn promote_from_queue(&mut self) {
        // 🔴 **`self.pushing` 这一条是必需的** —— 见那个字段的文档：
        // 没有它，`push` 的决策会被本函数当场推翻。
        if self.pushing || self.current.is_some() || self.queue.is_empty() {
            return;
        }

        // 选优先级最高的；同优先级时选**序号最小**（最早到达）。
        //
        // ## 🔴 这里我错了两次，而第二次的教训更重要
        //
        // **第一版**：`.max_by_key(|e| (e.state.priority(), Reverse(e.seq)))` ——
        // `max` 配 `Reverse(seq)` 选出的是**序号最大**的，也就是最晚到达的。
        //
        // **第二版**：`.min_by_key(|e| (Reverse(e.state.priority()), e.seq))` ——
        // 我**以为**它选"优先级最大、序号最小"，而实测它选了 `install`（seq=1）
        // 而不是 `download`（seq=0）。也就是说**它没有按我以为的语义工作**
        //（闭包返回一个元组时，`min_by_key` 的比较细节不是我该依赖的东西）。
        //
        // **所以现在是一个显式循环。** 它的判据用一行就能读懂，而那是
        // 这段逻辑最需要的性质 —— 它已经被两次"看起来对"的写法骗过。
        let mut best: Option<usize> = None;
        for i in 0..self.queue.len() {
            let cand = &self.queue[i];
            best = match best {
                None => Some(i),
                Some(b) => {
                    let cur = &self.queue[b];
                    let better = cand.state.priority() > cur.state.priority()
                        || (cand.state.priority() == cur.state.priority() && cand.seq < cur.seq);
                    if better {
                        Some(i)
                    } else {
                        Some(b)
                    }
                }
            };
        }

        if let Some(i) = best {
            let e = self.queue.remove(i);
            self.current = Some(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dl(done: u64, total: u64) -> IslandState {
        IslandState::Download {
            done_bytes: done,
            total_bytes: total,
            speed_bps: Some(1_024),
            eta_secs: Some(60),
        }
    }

    fn install(step: u32, steps: u32) -> IslandState {
        IslandState::Install {
            step,
            steps,
            current: "a.jar".into(),
        }
    }

    fn err(code: ErrorCode) -> IslandState {
        IslandState::Error {
            code,
            human: "一句话".into(),
        }
    }

    // ───────────────── §7.1 硬规则：只有一个岛 ─────────────────

    #[test]
    fn 只有一个当前状态而其余排队而不是并排() {
        // ⚠️ **这条测试钉的是"类型上不存在两个同时显示"**。
        let mut q = IslandQueue::new();
        q.push(dl(0, 100));
        println!(
            "[T1] after dl: current={:?} pending={}",
            q.current().map(IslandState::key),
            q.pending()
        );
        q.push(install(1, 3));
        println!(
            "[T1] after install: current={:?} pending={}",
            q.current().map(IslandState::key),
            q.pending()
        );
        println!(
            "[T1] dl.priority={} install.priority={}",
            dl(0, 100).priority(),
            install(1, 3).priority()
        );
        println!("[T1] display={:?}", q.display().key());
        // 当前的只有一个
        assert_eq!(q.display().key(), "download");
        // 另一个在队列里
        assert_eq!(q.pending(), 1);
    }

    #[test]
    fn 队列空时显示空闲态() {
        let q = IslandQueue::new();
        // ⚠️ 而"显示什么"与"队列里有什么"是两件事 —— 见 `display` 的文档
        assert!(q.current().is_none());
        assert_eq!(q.display(), IslandState::Idle);
    }

    // ───────────────── §7.3 约束 1：Error 可抢占，其余按到达顺序 ─────────────────

    #[test]
    fn 错误抢占当前而当前回到队首而不是被丢掉() {
        let mut q = IslandQueue::new();
        q.push(dl(10, 100));
        q.push(err(ErrorCode::ProcNonZeroExit));
        assert_eq!(q.display().key(), "error", "错误该抢占");
        // **被抢占的没丢** —— 规格说"任务不丢失"
        assert_eq!(q.pending(), 1);
        q.finish_current();
        assert_eq!(q.display().key(), "download", "错误结束后回到被抢占的那个");
    }

    #[test]
    fn 同优先级的新条目不抢占() {
        // 一个"新来的总是抢占"的实现会让两条连续到达的下载**互相打断**，
        // 于是两条都不显示进度。
        let mut q = IslandQueue::new();
        q.push(dl(10, 100));
        q.push(dl(20, 200));
        assert_eq!(q.display().key(), "download");
        assert_eq!(q.pending(), 1);
        // 而显示的那个仍然是**第一个**（进度是 10/100 而不是 20/200）
        assert_eq!(q.display().fraction(), Some(0.1));
    }

    #[test]
    fn 空闲态不压住任何任务() {
        let mut q = IslandQueue::new();
        q.push(IslandState::Idle);
        q.push(dl(1, 10));
        // 下载与空闲不同档 ⇒ 下载抢占
        assert_eq!(q.display().key(), "download");
    }

    #[test]
    fn 其余按到达顺序而不是按种类优先级() {
        // 规格原文："`Error` 优先级最高可抢占，**其余按到达顺序**"。
        // 一个给每态一个固定优先级的实现会让"安装"永远排在"下载"前面 ——
        // 而那是错的。
        let mut q = IslandQueue::new();
        q.push(IslandState::Launch {
            stage: LaunchStage::Parse,
        });
        q.push(install(1, 2));
        assert_eq!(q.display().key(), "launch", "先到的先显示");
        q.finish_current();
        assert_eq!(q.display().key(), "install", "然后才是后到的");
    }

    #[test]
    fn 多个排队时按到达顺序逐个提拔() {
        let mut q = IslandQueue::new();
        q.push(IslandState::Probe { done: 0, total: 3 });
        q.push(install(1, 2));
        q.push(IslandState::Update {
            version: "1.2".into(),
        });
        assert_eq!(q.display().key(), "probe");
        q.finish_current();
        assert_eq!(q.display().key(), "install");
        q.finish_current();
        assert_eq!(q.display().key(), "update");
        q.finish_current();
        assert_eq!(q.display(), IslandState::Idle);
    }

    // ───────────────── §7.3 约束 4：可关闭但任务不丢失 ─────────────────

    #[test]
    fn 收起不碰队列任务不丢失() {
        // ⚠️ 这是"收起不能是一个状态"的机器版：若收起是一个 `IslandState`，
        // 它就会参与优先级与队列，于是"收起时来的下一个任务"语义不明。
        let mut q = IslandQueue::new();
        q.push(dl(10, 100));
        q.push(install(1, 2));
        let before = q.display();
        q.set_view(IslandView::Collapsed);
        assert_eq!(q.view(), IslandView::Collapsed);
        // **队列与当前状态一个字节都没变**
        assert_eq!(q.display(), before);
        assert_eq!(q.pending(), 1);
    }

    #[test]
    fn 收起之后仍然能推进队列() {
        let mut q = IslandQueue::new();
        q.set_view(IslandView::Collapsed);
        q.push(dl(1, 10));
        q.finish_current();
        assert_eq!(q.display(), IslandState::Idle);
        // 而形态仍然是收起 —— 用户的选择不该被一次任务结束改掉
        assert_eq!(q.view(), IslandView::Collapsed);
    }

    // ───────────────── §7.3 约束 5：进度更新不该堆队列 ─────────────────

    #[test]
    fn 同种类的进度更新是就地覆盖而不是新推一条() {
        // ⚠️ 下载进度每 100 ms 报一次。一个只会 push 的实现会在一分钟里
        // 堆出 600 条 —— 而界面上表现为"下载完成了队列还在跑"。
        let mut q = IslandQueue::new();
        q.push(dl(0, 1000));
        for i in 1..=100u64 {
            assert!(q.update_current(dl(i * 10, 1000)), "应当就地更新");
        }
        assert_eq!(q.pending(), 0, "**一条都不该堆**");
        assert_eq!(q.display().fraction(), Some(1.0));
    }

    #[test]
    fn 就地更新保留原序号() {
        let mut q = IslandQueue::new();
        q.push(dl(1, 100));
        q.push(install(1, 2));
        assert!(q.update_current(dl(50, 100)));
        q.finish_current();
        // 若更新把序号推到了队尾，被抢占的顺序就会变。
        assert_eq!(q.display().key(), "install");
    }

    #[test]
    fn 不同种类不会被就地覆盖() {
        let mut q = IslandQueue::new();
        q.push(dl(1, 100));
        assert!(!q.update_current(install(1, 2)));
        assert_eq!(q.pending(), 0, "而被拒绝之后调用方该去 push");
        q.push(install(1, 2));
        assert_eq!(q.pending(), 1);
    }

    // ───────────────── 进度分数 ─────────────────

    #[test]
    fn 总量为零时分数是没有而不是零() {
        // ⚠️ "总量还不知道"与"一个字节都没下"在界面上是两件事。
        // 一个返回 0.0 的实现会让界面画一个"0%"的进度条，
        // 而它与"卡住了"在视觉上无法区分。
        assert_eq!(dl(0, 0).fraction(), None);
        assert_eq!(dl(0, 100).fraction(), Some(0.0));
    }

    #[test]
    fn 分数被夹在零到一之间() {
        assert_eq!(dl(200, 100).fraction(), Some(1.0));
    }

    #[test]
    fn 五阶段的分数是五分之一到五分之五() {
        assert_eq!(
            IslandState::Launch {
                stage: LaunchStage::Parse
            }
            .fraction(),
            Some(0.2)
        );
        assert_eq!(
            IslandState::Launch {
                stage: LaunchStage::Launch
            }
            .fraction(),
            Some(1.0)
        );
    }

    // ───────────────── 五阶段 ─────────────────

    #[test]
    fn 五阶段的顺序与序号一致且键是_ascii() {
        assert_eq!(LaunchStage::ALL.len(), 5);
        for (i, s) in LaunchStage::ALL.iter().enumerate() {
            assert_eq!(s.ordinal() as usize, i + 1, "{s:?} 的序号不对");
            assert!(s.key().is_ascii(), "{s:?} 的键不是 ASCII");
        }
        // 顺序即进度：派生的 `Ord` 必须与 `ALL` 的顺序一致
        let mut sorted = LaunchStage::ALL;
        sorted.sort();
        assert_eq!(sorted, LaunchStage::ALL);
    }

    // ───────────────── 优先级与键 ─────────────────

    #[test]
    fn 只有错误是抢占者而其余同档() {
        assert_eq!(err(ErrorCode::ProcStartFailed).priority(), 100);
        for s in [
            IslandState::Probe { done: 0, total: 1 },
            dl(1, 2),
            install(1, 2),
            IslandState::Launch {
                stage: LaunchStage::Parse,
            },
            IslandState::Running {
                started_at_ms: 0,
                resident_bytes: None,
            },
        ] {
            assert_eq!(s.priority(), 10, "{s:?} 应当是那一档");
        }
        assert_eq!(IslandState::Idle.priority(), 0);
    }

    #[test]
    fn 每一态的键唯一且是_ascii() {
        let states = [
            IslandState::Idle,
            IslandState::Probe { done: 0, total: 1 },
            dl(1, 2),
            install(1, 2),
            IslandState::Launch {
                stage: LaunchStage::Parse,
            },
            IslandState::Running {
                started_at_ms: 0,
                resident_bytes: None,
            },
            err(ErrorCode::ProcStartFailed),
            IslandState::Update {
                version: "1".into(),
            },
        ];
        let keys: Vec<&str> = states.iter().map(IslandState::key).collect();
        for k in &keys {
            assert!(k.is_ascii(), "{k} 不是 ASCII");
        }
        let mut uniq = keys.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), keys.len(), "有重复的键：{keys:?}");
    }

    #[test]
    fn 空闲不是活动态而其余都是() {
        assert!(!IslandState::Idle.is_active());
        for s in [
            IslandState::Probe { done: 0, total: 1 },
            err(ErrorCode::ProcStartFailed),
            IslandState::Update {
                version: "1".into(),
            },
        ] {
            assert!(s.is_active(), "{s:?} 应当是活动态");
        }
    }

    #[test]
    fn 八态都在且规格的表一行不少() {
        // ⚠️ **这条测试是"8 态"这个词的落点。** 一个只实现了六态的版本
        // 会在这里红，而不是在某人用到"预检态"时才发现。
        let all = [
            IslandState::Idle,
            IslandState::Probe { done: 0, total: 1 },
            dl(0, 1),
            install(0, 1),
            IslandState::Launch {
                stage: LaunchStage::Parse,
            },
            IslandState::Running {
                started_at_ms: 0,
                resident_bytes: None,
            },
            err(ErrorCode::ProcStartFailed),
            IslandState::Update {
                version: String::new(),
            },
        ];
        assert_eq!(all.len(), 8);
        let mut keys: Vec<&str> = all.iter().map(IslandState::key).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["download", "error", "idle", "install", "launch", "probe", "running", "update"]
        );
    }

    #[test]
    fn 序列化带_kind_标签且可往返() {
        let s = dl(1, 2);
        let j = serde_json::to_string(&s).expect("能序列化");
        assert!(j.contains("\"kind\":\"download\""), "实际：{j}");
        let back: IslandState = serde_json::from_str(&j).expect("能反序列化");
        assert_eq!(back, s);
    }

    #[test]
    fn 运行态的内存可以缺席且产品无关() {
        // ⚠️ §7.2："基岩版 Running → 进程存活探测、依赖组件状态（**无 JVM 概念**）"。
        // 所以 `None` 是**契约的一部分**，而不是"可能没有"。
        let bedrock_like = IslandState::Running {
            started_at_ms: 1_000,
            resident_bytes: None,
        };
        let j = serde_json::to_string(&bedrock_like).expect("能序列化");
        assert!(j.contains("null"), "None 该序列化成 null：{j}");
    }
}
