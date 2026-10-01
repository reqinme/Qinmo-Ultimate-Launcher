//! # 下载引擎的规则（**纯规则，零 IO**）
//!
//! 完整设计见 `docs/下载引擎设计规格.md`（三份独立实现交叉验证：
//! LeviLauncher 的竞态处理 + NexBox 的自适应爬坡 + Axolotl 的速度地板）。
//! 本模块只实现**能被穷举测试的那部分**：状态机、并发爬坡、分段计划、字节口径。
//!
//! ## 八条不变量里，哪些是"规则"
//!
//! | 不变量 | 在哪一层 |
//! |---|---|
//! | **I1** 未校验的字节不以正式文件名存在于磁盘 | **IO 层**（`.download` 后缀 + rename） |
//! | **I2** 提交与取消必须串行化 | **状态机**（本模块）+ IO 层的锁 |
//! | **I3** 校验函数按内容类型注入 | 形状（本模块给出签名） |
//! | **I4** 服务端忽略 Range 时必须重头下 | **状态机**（本模块） |
//! | **I5** 续传前校验服务端真的支持 | **规则**（本模块判定）+ IO 层读响应头 |
//! | **I6** 进度事件双层节流 | **规则**（本模块给节流器） |
//! | **I7** `file_bytes` 与 `session_bytes` 分开 | **数据形状**（本模块） |
//! | **I8** 每段独立续传与校验，末段完成才整体校验 | **规则**（分段计划）+ IO 层 |
//!
//! ## ⚠️ 两条最容易做错的，值得在代码里重复一次
//!
//! **① 校验期间暂停不能回退成重新下载**（规格 §2 的原文注释）：
//!
//! > *"A pause during verification retains the complete transfer. Resuming runs all integrity checks
//! > again without an invalid HTTP Range request beyond EOF."*
//!
//! 也就是说：**在 `Verifying` 阶段按暂停，暂停请求要等这次校验与提交走完才生效**——
//! 而**不是**把状态退回 `Paused` 然后重新下一遍。
//!
//! **② 服务端忽略 Range 时必须删掉临时文件重头下**（I4）：
//!
//! 一个"接着写"的实现会在临时文件后面**追加一份从头开始的数据**，
//! 产出一个**长度对、内容错**的文件 —— 而它随后会被校验拦住，
//! 于是表现为"反复校验失败"，**而真正的原因在几百行外的 Range 处理里**。

use serde::{Deserialize, Serialize};

/// 任务状态（规格 §2 的状态机）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TaskState {
    /// 排队（受并发闸门限制）
    Queued,
    Downloading,
    Paused,
    /// 校验中。**暂停请求在此阶段要等校验与提交走完才生效。**
    Verifying,
    /// 提交中（rename）。**与 Pause/Cancel 互斥**（I2）。
    Committing,
    Done,
    Cancelled,
    Failed,
}

impl TaskState {
    pub const fn key(self) -> &'static str {
        match self {
            TaskState::Queued => "queued",
            TaskState::Downloading => "downloading",
            TaskState::Paused => "paused",
            TaskState::Verifying => "verifying",
            TaskState::Committing => "committing",
            TaskState::Done => "done",
            TaskState::Cancelled => "cancelled",
            TaskState::Failed => "failed",
        }
    }

    /// 是否已到终态（不会再变）。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskState::Done | TaskState::Cancelled | TaskState::Failed
        )
    }

    /// 这个阶段能不能被"立刻暂停"。
    ///
    /// **`Verifying` 与 `Committing` 都不能** —— 它们是 I2 说的"与取消互斥"的区间。
    /// 在 `Committing` 里强行暂停会产出一个**已 rename 但事件未发出**的任务，
    /// 而那就是 LeviLauncher 注释里专门警告的竞态。
    pub const fn can_pause_now(self) -> bool {
        matches!(self, TaskState::Queued | TaskState::Downloading)
    }
}

/// 一个任务收到的请求。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Pause,
    Resume,
    Cancel,
    /// 传输完成（进入校验）
    TransferDone,
    /// 校验通过（进入提交）
    Verified,
    /// 校验不通过
    VerifyFailed,
    /// 提交成功（rename 成功）
    Committed,
    /// 出错
    Failed,
}

/// 状态机的一次转移结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// 状态变了
    To(TaskState),
    /// **请求被记住了，但要等当前这个不可中断的阶段走完**
    ///
    /// 这是 I2 的落点：校验/提交期间按暂停不是"拒绝"，而是"排队"。
    /// 拒绝的话用户会觉得"按钮坏了"；立刻切状态则会产出一个
    /// **已 rename 但事件未发出**的任务。
    Deferred(TaskState),
    /// 请求在当前状态下无意义（例如对已 `Done` 的任务按暂停）
    Ignored { reason: &'static str },
}

impl Transition {
    pub fn state(&self) -> Option<TaskState> {
        match self {
            Transition::To(s) => Some(*s),
            Transition::Deferred(s) => Some(*s),
            Transition::Ignored { .. } => None,
        }
    }
    pub fn is_deferred(&self) -> bool {
        matches!(self, Transition::Deferred(_))
    }
}

/// **状态机**：纯函数，输入 `(当前状态, 请求, 是否已排队一个暂停)`，输出一次转移。
///
/// ## 为什么把它做成纯函数而不是让任务自己管状态
///
/// 因为竞态是**下载器里最难复现的一类缺陷**：它只在"某个请求恰好在某个瞬间到达"时出现。
/// 做成纯函数之后，**全部竞态都可以被穷举**（8 状态 × 8 请求 = 64 个组合，
/// 而其中每一对都能被断言）。
pub fn step(state: TaskState, req: Request, pause_pending: bool) -> Transition {
    use Request as R;
    use TaskState as S;

    // 终态：什么都不接受（除了查询性质的重复请求）
    if state.is_terminal() {
        return Transition::Ignored {
            reason: "任务已到终态",
        };
    }

    match (state, req) {
        // ── 排队 ──
        (S::Queued, R::Pause) => Transition::To(S::Paused),
        (S::Queued, R::Resume) => Transition::To(S::Queued),
        (S::Queued, R::Cancel) => Transition::To(S::Cancelled),
        (S::Queued, R::Failed) => Transition::To(S::Failed),
        // 排队中收到"传输完成"不该发生（还没开始传）
        (S::Queued, R::TransferDone) => Transition::Ignored {
            reason: "还没开始传输",
        },
        (S::Queued, R::Verified) | (S::Queued, R::VerifyFailed) | (S::Queued, R::Committed) => {
            Transition::Ignored {
                reason: "排队中还没到校验与提交",
            }
        }

        // ── 下载中 ──
        (S::Downloading, R::Pause) => Transition::To(S::Paused),
        (S::Downloading, R::Resume) => Transition::Ignored {
            reason: "已经在下载",
        },
        (S::Downloading, R::Cancel) => Transition::To(S::Cancelled),
        (S::Downloading, R::TransferDone) => Transition::To(S::Verifying),
        (S::Downloading, R::Failed) => Transition::To(S::Failed),
        (S::Downloading, R::Verified) | (S::Downloading, R::VerifyFailed) => Transition::Ignored {
            reason: "还没到校验阶段",
        },
        (S::Downloading, R::Committed) => Transition::Ignored {
            reason: "还没到提交阶段",
        },

        // ── 暂停 ──
        (S::Paused, R::Resume) => Transition::To(S::Downloading),
        (S::Paused, R::Pause) => Transition::Ignored {
            reason: "已经暂停"
        },
        (S::Paused, R::Cancel) => Transition::To(S::Cancelled),
        (S::Paused, R::Failed) => Transition::To(S::Failed),
        (S::Paused, R::TransferDone) => Transition::Ignored {
            reason: "暂停中不该收到传输完成",
        },
        (S::Paused, R::Verified) | (S::Paused, R::VerifyFailed) | (S::Paused, R::Committed) => {
            Transition::Ignored {
                reason: "暂停中还没到校验与提交",
            }
        }

        // ── 校验中：**I2 的核心** ──
        //
        // 暂停/取消在这里**不能立刻生效**，但也**不能丢**——
        // 所以它们变成一个"待办"。校验与提交走完之后才真正停下来。
        (S::Verifying, R::Pause) => Transition::Deferred(S::Verifying),
        (S::Verifying, R::Cancel) => Transition::Deferred(S::Verifying),
        (S::Verifying, R::Resume) => Transition::Ignored {
            reason: "校验不需要恢复",
        },
        // **校验通过 → 提交**。但若之前排了一个暂停，**先落到 `Paused`**：
        // 校验已经过了，重跑一次校验是免费的（规格原文要求"恢复时重跑校验"），
        // 而**提交是不可逆的**（rename 之后临时文件就没了）。
        (S::Verifying, R::Verified) => {
            if pause_pending {
                Transition::To(S::Paused)
            } else {
                Transition::To(S::Committing)
            }
        }
        (S::Verifying, R::VerifyFailed) => Transition::To(S::Failed),
        (S::Verifying, R::TransferDone) => Transition::Ignored {
            reason: "已经在校验",
        },
        (S::Verifying, R::Committed) => Transition::Ignored {
            reason: "还没到提交阶段",
        },
        (S::Verifying, R::Failed) => Transition::To(S::Failed),

        // ── 提交中：**与暂停/取消互斥** ──
        //
        // 这里**连"待办"都不排队**，因为 rename 一旦成功就不可逆 ——
        // 排一个取消只是徒劳（任务已经完成了）。
        // 所以取消在这里被**忽略**（`Ignored`），而不是延后。
        (S::Committing, R::Committed) => Transition::To(S::Done),
        (S::Committing, R::Pause) | (S::Committing, R::Cancel) => Transition::Ignored {
            reason: "正在提交，无法中断（rename 已不可逆）",
        },
        (S::Committing, R::Verified) => Transition::Ignored {
            reason: "已经在提交",
        },
        (S::Committing, R::TransferDone) => Transition::Ignored {
            reason: "已经在提交",
        },
        (S::Committing, R::VerifyFailed) => Transition::Ignored {
            reason: "已在提交，校验失败不该出现",
        },
        (S::Committing, R::Resume) => Transition::Ignored {
            reason: "提交无法恢复",
        },
        (S::Committing, R::Failed) => Transition::To(S::Failed),

        // 终态在上面被提前拦住了，但**编译器推不出这一点** ——
        // 而它的推不出正好迫使我把这一条写出来。
        // 这是一个好信号：穷举的状态机不该有任何"默认分支"，
        // 因为默认分支会让**新增一个状态时静默地走进它**。
        (S::Done, _) | (S::Cancelled, _) | (S::Failed, _) => Transition::Ignored {
            reason: "任务已到终态",
        },
    }
}

// ───────────────────────────── 自适应爬坡 ─────────────────────────────

/// **自适应并发爬坡**（规格 §3，依据 NexBox `segment_coordinator.rs:456-535`）。
///
/// 取代"固定并发数"：初 4 worker、每 2s 评估、改善 ×1.25、劣化 ×0.65、
/// 连续 2 次劣化收缩、上限 64 段。
///
/// ## 为什么"连续 2 次劣化"而不是"一次就缩"
///
/// 因为**单次测量在网络上噪声很大**（我们在 S5 实测里见过同一格两轮差 1.7 倍）。
/// 一次劣化就缩会让并发数在网络抖动的每一秒都来回跳，
/// 而**每一次调整都要重建连接** —— 抖动本身会因此变得更糟。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ramp {
    /// 当前 worker 数
    pub current: u32,
    /// 上一次评估时的吞吐（字节/秒）
    pub last_throughput: u64,
    /// 连续劣化次数
    pub consecutive_worse: u32,
}

/// 爬坡的配置常量。**它们都在规格里定了值**，所以不留给调用方自由发挥 ——
/// 一个可以被随手调的并发上限，会在某台机器上把对方站点打挂。
pub const RAMP_INITIAL: u32 = 4;
pub const RAMP_MAX: u32 = 64;
pub const RAMP_GROW_NUM: u32 = 125; // ×1.25
pub const RAMP_GROW_DEN: u32 = 100;
pub const RAMP_SHRINK_NUM: u32 = 65; // ×0.65
pub const RAMP_SHRINK_DEN: u32 = 100;
/// 连续这么多次劣化才收缩
pub const RAMP_WORSE_LIMIT: u32 = 2;
/// 改善要超过这个比例才算"真的改善"
pub const RAMP_IMPROVE_NUM: u64 = 105; // ×1.05
pub const RAMP_IMPROVE_DEN: u64 = 100;

impl Ramp {
    pub const fn new() -> Self {
        Self {
            current: RAMP_INITIAL,
            last_throughput: 0,
            consecutive_worse: 0,
        }
    }

    /// 喂一次实测吞吐，返回新的爬坡状态。
    ///
    /// 判据（规格 §3）：
    /// - 比上次**好 5% 以上** → 扩（×1.25），并把劣化计数清零
    /// - 比上次**差** → 劣化计数 +1；**连续 2 次**才缩（×0.65）
    /// - 在 5% 以内 → **不动**（噪声不该被当成信号）
    pub fn observe(&self, throughput: u64) -> Self {
        // 第一次观测只记基线，不调整 —— 没有"上一次"可比
        if self.last_throughput == 0 {
            return Self {
                current: self.current,
                last_throughput: throughput,
                consecutive_worse: 0,
            };
        }
        let last = self.last_throughput;
        let improved =
            throughput.saturating_mul(RAMP_IMPROVE_DEN) > last.saturating_mul(RAMP_IMPROVE_NUM);
        let worse = throughput < last;

        if improved {
            let next = (self.current * RAMP_GROW_NUM / RAMP_GROW_DEN).min(RAMP_MAX);
            Self {
                current: next.max(self.current),
                last_throughput: throughput,
                consecutive_worse: 0,
            }
        } else if worse {
            let n = self.consecutive_worse + 1;
            if n >= RAMP_WORSE_LIMIT {
                let next = (self.current * RAMP_SHRINK_NUM / RAMP_SHRINK_DEN).max(1);
                Self {
                    current: next,
                    last_throughput: throughput,
                    consecutive_worse: 0,
                }
            } else {
                Self {
                    current: self.current,
                    last_throughput: throughput,
                    consecutive_worse: n,
                }
            }
        } else {
            // 噪声区间：记下吞吐但不改并发
            Self {
                current: self.current,
                last_throughput: throughput,
                consecutive_worse: 0,
            }
        }
    }
}

impl Default for Ramp {
    fn default() -> Self {
        Self::new()
    }
}

// ───────────────────────────── 分段计划 ─────────────────────────────

/// 单文件多段并行的规则（规格 §5.1，**PCL 真正的那一招**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub index: u32,
    pub start: u64,
    pub end_inclusive: u64,
}

impl Segment {
    pub const fn len(&self) -> u64 {
        self.end_inclusive - self.start + 1
    }
    pub const fn is_empty(&self) -> bool {
        false
    }
}

/// 分段常量（规格 §5.1）。
/// **≥2 MB 才分段** —— 小文件分段的收益被连接开销吃掉。
pub const SEGMENT_MIN_TOTAL: u64 = 2 * 1024 * 1024;
/// **尾部 <64 KB 不单独成段** —— 一个 8 KB 的段会占一个 worker 却几乎不产生吞吐。
pub const SEGMENT_MIN_TAIL: u64 = 64 * 1024;

/// **决定要不要分段、分几段。**
///
/// 返回 `None` 表示**不分段**（单连接顺序下载）。判据三条，缺一不可：
///
/// | 判据 | 为什么 |
/// |---|---|
/// | **服务端支持 Range** | 不支持时分段根本发不出（I4） |
/// | **总大小 ≥ 2 MB** | 小文件分段的收益被连接开销吃掉 |
/// | **知道总大小** | 不知道就没法切（`Content-Length` 缺失） |
///
/// `want` 是爬坡的输出（想要几段），而实际段数会被总大小压住 ——
/// **段太小时把段数降下来**，而不是产出很多 4 KB 的段。
pub fn plan_segments(total: u64, want: u32, range_supported: bool) -> Option<Vec<Segment>> {
    if !range_supported || total < SEGMENT_MIN_TOTAL || want < 2 {
        return None;
    }
    // 段数不能多到让每段小于"最小整段"（用 MIN_TAIL 作为下限：
    // 一段小于 64 KB 就没有单独存在的意义）
    let by_size = (total / SEGMENT_MIN_TAIL).max(1);
    let n = (want as u64).min(by_size).min(RAMP_MAX as u64) as u32;
    if n < 2 {
        return None;
    }

    let base = total / n as u64;
    let mut segs = Vec::with_capacity(n as usize);
    let mut start = 0u64;
    for i in 0..n {
        let mut end = start + base - 1;
        if i == n - 1 {
            // 最后一段吃掉余数，保证**覆盖到 total-1 且不越界**
            end = total - 1;
        }
        // **尾部过短就并进上一段**，而不是单独成段
        if i == n - 2 && total - 1 - end < SEGMENT_MIN_TAIL {
            end = total - 1;
            segs.push(Segment {
                index: i,
                start,
                end_inclusive: end,
            });
            return Some(segs);
        }
        segs.push(Segment {
            index: i,
            start,
            end_inclusive: end,
        });
        start = end + 1;
        if start >= total {
            return Some(segs);
        }
    }
    Some(segs)
}

/// 校验分段计划的**完整性**：恰好覆盖 `[0, total)`、无重叠、无空洞、按序。
///
/// 它存在的理由很直接：**一个漏了几百字节的分段计划会产出长度不足的文件**，
/// 而那个文件随后被校验拦住 —— 表现为"反复校验失败"，
/// 而真正的原因在分段算术里。所以算术要被直接检验。
pub fn segments_cover_exactly(segs: &[Segment], total: u64) -> Result<(), String> {
    if segs.is_empty() {
        return Err("分段计划为空".to_string());
    }
    let mut expect = 0u64;
    for (i, s) in segs.iter().enumerate() {
        if s.index != i as u32 {
            return Err(format!("第 {i} 段的 index 是 {}，应当是 {i}", s.index));
        }
        if s.start != expect {
            return Err(format!(
                "第 {i} 段从 {} 开始，应当是 {expect}（有空洞或重叠）",
                s.start
            ));
        }
        if s.end_inclusive < s.start {
            return Err(format!(
                "第 {i} 段的结束 {} 小于开始 {}",
                s.end_inclusive, s.start
            ));
        }
        expect = s.end_inclusive + 1;
    }
    if expect != total {
        return Err(format!("分段覆盖到 {expect}，应当是 {total}"));
    }
    Ok(())
}

// ───────────────────────────── 字节口径（I7）─────────────────────────────

/// **两个字节口径必须分开表达**（不变量 I7）。
///
/// 设计规格把它单独列出来，因为**这是 LeviLauncher 的一个易错点**：
/// 它把 `downloaded` 在续传时重置成 `cur`，写法自洽但脆弱 ——
/// 界面上的"总进度"会**突然从 40% 跳回 10%**，
/// 而用户看到的是"下载倒退了"。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    /// **文件总进度**（含续传历史）—— 界面进度条用它
    pub file_bytes: u64,
    /// **本次会话新增** —— "本次下载了 X MB"用它
    pub session_bytes: u64,
    /// 文件总大小（未知时为 `None`）
    pub total: Option<u64>,
}

impl Progress {
    pub const fn new(total: Option<u64>) -> Self {
        Self {
            file_bytes: 0,
            session_bytes: 0,
            total,
        }
    }

    /// 写入 `n` 字节。
    pub fn add(&mut self, n: u64) {
        self.file_bytes = self.file_bytes.saturating_add(n);
        self.session_bytes = self.session_bytes.saturating_add(n);
    }

    /// **续传起点**：把 `file_bytes` 设成已有的字节数，而 `session_bytes` **保持 0**。
    ///
    /// 这就是两个口径分开的意义：界面看到 40%（file），而"本次下载"从 0 开始。
    pub fn resume_from(&mut self, already: u64) {
        self.file_bytes = already;
        self.session_bytes = 0;
    }

    /// 完成比例（0.0–1.0）。总大小未知时返回 `None`。
    pub fn ratio(&self) -> Option<f64> {
        let t = self.total?;
        if t == 0 {
            return Some(1.0);
        }
        Some((self.file_bytes as f64 / t as f64).clamp(0.0, 1.0))
    }

    /// 是否已经完整（知道总大小时）。
    pub fn is_complete(&self) -> bool {
        matches!(self.total, Some(t) if self.file_bytes >= t)
    }
}

// ───────────────────────────── 进度节流（I6）─────────────────────────────

/// **双层节流里的引擎侧**（宿主侧另有一层，见规格 I6）。
///
/// 规格给的来源：NexBox `segment_coordinator.rs:814`（200ms）
/// + `download_accelerator.rs:709/734`（200ms）。
///
/// **为什么要双层而不是一层**：引擎按"字节到达"发事件，
/// 而宿主按"界面能画的频率"消费。两层各节流一次，
/// 任何一层的调用方变了都不会把另一层冲垮。
#[derive(Debug, Clone, Copy)]
pub struct Throttle {
    /// 最小间隔（毫秒）
    pub min_interval_ms: u64,
    last_emit_ms: u64,
    /// **是否已经发过至少一次。**
    ///
    /// ⚠️ 第一版**没有这个字段**，而是用 `last_emit_ms == 0` 当"还没发过"的哨兵 ——
    /// 而 `now_ms` 本身就可能为 0（时钟起点，而且有些平台精度很粗），
    /// 于是那个哨兵**会一直是真**，节流彻底失效：
    /// `allow(0)` 之后 `allow(0)` 仍然返回 true。
    ///
    /// 这个缺陷是 `时钟为零也不会让节流失效` 那条测试抓出来的。
    /// 教训：**用"某个合法取值范围里的值"当哨兵，迟早会撞上它。**
    emitted: bool,
}

/// 进度事件的默认节流间隔（规格 I6：200ms ⇒ ≤5 次/s）。
pub const THROTTLE_ENGINE_MS: u64 = 200;

impl Throttle {
    pub const fn new(min_interval_ms: u64) -> Self {
        Self {
            min_interval_ms,
            last_emit_ms: 0,
            emitted: false,
        }
    }

    /// 现在能不能发事件（`now_ms` 是单调时钟的毫秒值）。
    ///
    /// **第一次总是允许**：否则界面上会有一段"完全没有进度"的时间，
    /// 而用户会以为卡住了。
    pub fn allow(&mut self, now_ms: u64) -> bool {
        if !self.emitted || now_ms.saturating_sub(self.last_emit_ms) >= self.min_interval_ms {
            self.last_emit_ms = now_ms;
            self.emitted = true;
            return true;
        }
        false
    }

    /// **终态必须无条件放行**（完成/失败/取消）。
    ///
    /// 否则一个恰好在节流窗口内到达的"完成"事件会被丢掉，
    /// 而界面上任务会永远停在 99%。
    pub fn force(&mut self, now_ms: u64) -> bool {
        self.last_emit_ms = now_ms;
        self.emitted = true;
        true
    }
}

// ───────────────────────────── 续传前置条件（I5）─────────────────────────────

/// 服务端对一次请求的**可续传性**回答（I5）。
///
/// 规格原文：
///
/// > **续传前必须校验服务端是否真的支持**：`accept-ranges: bytes` +
/// > `Content-Encoding: identity` + `content-length` +（强 `ETag` **或** `Last-Modified`），
/// > **任一缺失即放弃续传**
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerResumeSignals {
    /// `Accept-Ranges: bytes`（注意：**不是** `none`，也不是缺失）
    pub accept_ranges_bytes: bool,
    /// `Content-Encoding` 是否为 `identity`（压缩过的响应**不能**续传）
    pub content_encoding_identity: bool,
    /// 是否给出了 `Content-Length`
    pub has_content_length: bool,
    /// 强 `ETag`（**弱 ETag `W/"..."` 不算** —— 它不保证字节一致）
    pub strong_etag: Option<String>,
    pub last_modified: Option<String>,
}

/// 续传判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeVerdict {
    /// 可以续传
    Ok,
    /// **不可以**，必须重头下。带上**具体的缺失项**，因为
    /// "为什么不能续传"决定了这是"服务端不支持"还是"我们的请求有问题"。
    Restart { missing: Vec<&'static str> },
}

impl ServerResumeSignals {
    /// 判据。
    ///
    /// **顺序不是随意的**：先报"服务端根本不支持 Range"，
    /// 因为那是**最常见且无需排查**的原因；把"缺 ETag"报在前面会让用户去查一个次要条件。
    pub fn verdict(&self) -> ResumeVerdict {
        let mut missing = Vec::new();
        if !self.accept_ranges_bytes {
            missing.push("accept-ranges: bytes 缺失或不是 bytes");
        }
        if !self.content_encoding_identity {
            missing.push("Content-Encoding 不是 identity（压缩响应无法续传）");
        }
        if !self.has_content_length {
            missing.push("缺少 Content-Length");
        }
        if self.strong_etag.is_none() && self.last_modified.is_none() {
            missing.push("既无强 ETag 也无 Last-Modified（无法确认服务端内容未变）");
        }
        if missing.is_empty() {
            ResumeVerdict::Ok
        } else {
            ResumeVerdict::Restart { missing }
        }
    }

    /// 一个"服务端忽略 Range"的应答形态（I4 要处理的情形）。
    ///
    /// **它返回的是 `Restart`**，而调用方必须**删掉临时文件**再重头下 ——
    /// 一个"接着写"的实现会在临时文件后面追加一份从头开始的数据，
    /// 产出**长度对、内容错**的文件。
    pub fn ignored_range() -> Self {
        Self {
            accept_ranges_bytes: false,
            content_encoding_identity: true,
            has_content_length: true,
            strong_etag: None,
            last_modified: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ───────────────── 状态机 ─────────────────

    #[test]
    fn 正常路径逐级前进() {
        let s = TaskState::Queued;
        let s = step(s, Request::Resume, false).state().unwrap();
        assert_eq!(s, TaskState::Queued, "排队中 resume 还是排队");
        let s = step(TaskState::Downloading, Request::TransferDone, false)
            .state()
            .unwrap();
        assert_eq!(s, TaskState::Verifying);
        let s = step(s, Request::Verified, false).state().unwrap();
        assert_eq!(s, TaskState::Committing);
        let s = step(s, Request::Committed, false).state().unwrap();
        assert_eq!(s, TaskState::Done);
        assert!(s.is_terminal());
    }

    #[test]
    fn 校验期间暂停是排队而不是立刻生效() {
        // 规格 §2 的原文注释：**校验期间暂停不能回退成重新下载**。
        // 立刻切到 Paused 会丢掉"已完整传输"这个事实 ——
        // 而那是几个 GB 的传输。
        let t = step(TaskState::Verifying, Request::Pause, false);
        assert!(t.is_deferred(), "{t:?}");
        assert_eq!(t.state(), Some(TaskState::Verifying), "状态不变");
    }

    #[test]
    fn 校验通过后若排了暂停则先落暂停() {
        // **提交是不可逆的**（rename 之后临时文件就没了），
        // 所以"校验已过 + 用户按了暂停"时必须先停 ——
        // 重跑一次校验是免费的（规格要求"恢复时重跑校验"），而提交不是。
        let t = step(TaskState::Verifying, Request::Verified, true);
        assert_eq!(t.state(), Some(TaskState::Paused), "{t:?}");

        // 没排暂停就正常进提交
        let t2 = step(TaskState::Verifying, Request::Verified, false);
        assert_eq!(t2.state(), Some(TaskState::Committing));
    }

    #[test]
    fn 提交期间取消与暂停都被拒绝而不是排队() {
        // I2：提交与取消**互斥**。这里**连待办都不排队** ——
        // rename 一旦成功就不可逆，排一个取消只是徒劳。
        for req in [Request::Pause, Request::Cancel] {
            let t = step(TaskState::Committing, req, false);
            assert!(
                matches!(t, Transition::Ignored { .. }),
                "{req:?} 应当被忽略，实际 {t:?}"
            );
        }
    }

    #[test]
    fn 终态不接受任何请求() {
        for s in [TaskState::Done, TaskState::Cancelled, TaskState::Failed] {
            assert!(s.is_terminal());
            for req in [
                Request::Pause,
                Request::Resume,
                Request::Cancel,
                Request::TransferDone,
                Request::Verified,
                Request::Committed,
                Request::VerifyFailed,
                Request::Failed,
            ] {
                let t = step(s, req, false);
                assert!(
                    matches!(t, Transition::Ignored { .. }),
                    "{s:?} + {req:?} 应当被忽略，实际 {t:?}"
                );
            }
        }
    }

    #[test]
    fn 暂停中不能暂停已下载中不能恢复() {
        assert!(matches!(
            step(TaskState::Paused, Request::Pause, false),
            Transition::Ignored { .. }
        ));
        assert!(matches!(
            step(TaskState::Downloading, Request::Resume, false),
            Transition::Ignored { .. }
        ));
    }

    #[test]
    fn 校验失败直接失败而不是回到下载() {
        // 规格 §2 的重试策略是"删临时文件 + retry_count += 1"，
        // 而**那是上层的事**（它要新建一个任务）。状态机这一层的答案是"失败"——
        // 让状态机自己回到 Downloading 会让"重试几次"这个计数无处安放。
        let t = step(TaskState::Verifying, Request::VerifyFailed, false);
        assert_eq!(t.state(), Some(TaskState::Failed));
    }

    #[test]
    fn 任意非终态都可以取消或被标为失败() {
        for s in [TaskState::Queued, TaskState::Downloading, TaskState::Paused] {
            assert_eq!(
                step(s, Request::Cancel, false).state(),
                Some(TaskState::Cancelled),
                "{s:?} 应当能取消"
            );
            assert_eq!(
                step(s, Request::Failed, false).state(),
                Some(TaskState::Failed),
                "{s:?} 应当能失败"
            );
        }
    }

    #[test]
    fn 只有排队与下载中能立刻暂停() {
        assert!(TaskState::Queued.can_pause_now());
        assert!(TaskState::Downloading.can_pause_now());
        // 这两个阶段**不能**立刻停
        assert!(!TaskState::Verifying.can_pause_now());
        assert!(!TaskState::Committing.can_pause_now());
        // 终态谈不上"暂停"
        assert!(!TaskState::Done.can_pause_now());
    }

    #[test]
    fn 状态机的全部组合都有定义() {
        // **穷举**：8 状态 × 8 请求 = 64 个组合，每一个都要有明确结果。
        // 这是把"竞态"变成"可穷举"的收益 —— 竞态是下载器里最难复现的一类缺陷。
        let states = [
            TaskState::Queued,
            TaskState::Downloading,
            TaskState::Paused,
            TaskState::Verifying,
            TaskState::Committing,
            TaskState::Done,
            TaskState::Cancelled,
            TaskState::Failed,
        ];
        let reqs = [
            Request::Pause,
            Request::Resume,
            Request::Cancel,
            Request::TransferDone,
            Request::Verified,
            Request::VerifyFailed,
            Request::Committed,
            Request::Failed,
        ];
        for s in states {
            for r in reqs {
                // 不该 panic
                let t = step(s, r, false);
                let _ = t.state();
                // 键必须正确
                assert!(!s.key().is_empty());
            }
        }
    }

    // ───────────────── 爬坡 ─────────────────

    #[test]
    fn 首次观测只记基线不调整() {
        let r = Ramp::new();
        assert_eq!(r.current, RAMP_INITIAL);
        let r2 = r.observe(1_000_000);
        assert_eq!(r2.current, RAMP_INITIAL, "没有上一次可比，不该调整");
        assert_eq!(r2.last_throughput, 1_000_000);
    }

    #[test]
    fn 明显改善就扩容() {
        let r = Ramp {
            current: 4,
            last_throughput: 1_000_000,
            consecutive_worse: 0,
        };
        // 好 50%
        let r2 = r.observe(1_500_000);
        assert_eq!(r2.current, 5, "4 × 1.25 = 5");
        assert_eq!(r2.consecutive_worse, 0);
    }

    #[test]
    fn 噪声区间不调整() {
        // 在 5% 以内应当**不动** —— 单次测量在网络上噪声很大，
        // 而每一次调整都要重建连接，抖动本身会因此更糟。
        let r = Ramp {
            current: 8,
            last_throughput: 1_000_000,
            consecutive_worse: 0,
        };
        for t in [1_000_000u64, 1_020_000, 999_000] {
            let r2 = r.observe(t);
            assert_eq!(r2.current, 8, "吞吐 {t} 在噪声区间内，不该调整");
        }
    }

    #[test]
    fn 一次劣化不缩_连续两次才缩() {
        // 一次就缩会让并发数在抖动里来回跳。
        let r = Ramp {
            current: 16,
            last_throughput: 1_000_000,
            consecutive_worse: 0,
        };
        let r1 = r.observe(500_000);
        assert_eq!(r1.current, 16, "第一次劣化不缩");
        assert_eq!(r1.consecutive_worse, 1);

        let r2 = r1.observe(400_000);
        assert_eq!(r2.current, 10, "16 × 0.65 = 10.4 → 10");
        assert_eq!(r2.consecutive_worse, 0, "缩过之后计数清零");
    }

    #[test]
    fn 扩容有上限且收缩有下限() {
        let mut r = Ramp {
            current: RAMP_MAX,
            last_throughput: 1,
            consecutive_worse: 0,
        };
        for _ in 0..20 {
            r = r.observe(r.last_throughput.saturating_mul(2).max(2));
        }
        assert!(r.current <= RAMP_MAX, "不该超过上限：{}", r.current);

        let mut r2 = Ramp {
            current: 1,
            last_throughput: 1_000_000,
            consecutive_worse: 0,
        };
        for _ in 0..20 {
            r2 = r2.observe(1);
        }
        assert!(r2.current >= 1, "不该低于 1：{}", r2.current);
    }

    #[test]
    fn 改善会清掉劣化计数() {
        // 否则"抖动 3 次"会被当成"连续劣化 3 次"而误缩。
        let r = Ramp {
            current: 8,
            last_throughput: 1_000_000,
            consecutive_worse: 1,
        };
        let r2 = r.observe(2_000_000);
        assert_eq!(r2.consecutive_worse, 0);
    }

    // ───────────────── 分段 ─────────────────

    #[test]
    fn 小文件与不支持_range_都不分段() {
        assert!(plan_segments(1024, 8, true).is_none(), "1 KB 太小");
        assert!(
            plan_segments(SEGMENT_MIN_TOTAL - 1, 8, true).is_none(),
            "刚好差一字节也不分"
        );
        assert!(
            plan_segments(10 * 1024 * 1024, 8, false).is_none(),
            "不支持 Range 时分段发不出请求"
        );
        assert!(
            plan_segments(10 * 1024 * 1024, 1, true).is_none(),
            "只要 1 段"
        );
    }

    #[test]
    fn 分段恰好覆盖整个文件() {
        // **这是分段算术唯一必须成立的性质。**
        // 漏几百字节 → 文件长度不足 → 被校验拦住 → 表现为"反复校验失败"，
        // 而真正的原因在算术里。
        for total in [
            SEGMENT_MIN_TOTAL,
            3 * 1024 * 1024 + 7,
            10 * 1024 * 1024,
            123_456_789,
        ] {
            for want in [2u32, 3, 4, 8, 16, 64] {
                let segs = plan_segments(total, want, true).expect("应当能分段");
                segments_cover_exactly(&segs, total)
                    .unwrap_or_else(|e| panic!("total={total} want={want}: {e}"));
                // 段大小不应当小于最小尾部
                for s in &segs {
                    assert!(
                        s.len() >= SEGMENT_MIN_TAIL,
                        "total={total} want={want} 里出现了 {}-byte 的段",
                        s.len()
                    );
                }
            }
        }
    }

    #[test]
    fn 段数被文件大小压住而不是硬给() {
        // 一个 2 MB 的文件不该被切成 64 段（每段 32 KB）。
        let segs = plan_segments(SEGMENT_MIN_TOTAL, 64, true).unwrap();
        assert!(
            segs.len() < 64,
            "2 MB 的文件不该被切成 64 段，实际 {}",
            segs.len()
        );
        segments_cover_exactly(&segs, SEGMENT_MIN_TOTAL).unwrap();
    }

    #[test]
    fn 尾部过短时并进上一段() {
        // 构造一个"除以 n 之后尾部很碎"的大小
        let total = 10 * 1024 * 1024 + 8 * 1024; // 多出 8 KB
        let segs = plan_segments(total, 4, true).unwrap();
        segments_cover_exactly(&segs, total).unwrap();
        for s in &segs {
            assert!(
                s.len() >= SEGMENT_MIN_TAIL,
                "不该有 {}-byte 的碎段",
                s.len()
            );
        }
    }

    #[test]
    fn 越界的分段计划会被检查出来() {
        // 这条测试证明**检查函数本身能红** —— 否则它只是一段永远通过的代码。
        let bad = vec![
            Segment {
                index: 0,
                start: 0,
                end_inclusive: 99,
            },
            // 空洞：从 200 开始
            Segment {
                index: 1,
                start: 200,
                end_inclusive: 299,
            },
        ];
        let e = segments_cover_exactly(&bad, 300).unwrap_err();
        assert!(e.contains("空洞") || e.contains("重叠"), "{e}");

        // 覆盖不足
        let short = vec![Segment {
            index: 0,
            start: 0,
            end_inclusive: 99,
        }];
        let e2 = segments_cover_exactly(&short, 300).unwrap_err();
        assert!(e2.contains("应当"), "{e2}");

        // index 乱序
        let bad_idx = vec![Segment {
            index: 3,
            start: 0,
            end_inclusive: 99,
        }];
        assert!(segments_cover_exactly(&bad_idx, 100).is_err());
    }

    // ───────────────── 字节口径（I7）─────────────────

    #[test]
    fn 总进度与本次会话分开表达() {
        // 规格 I7 专门列出这一点：LeviLauncher 把 downloaded 在续传时重置成 cur，
        // 于是界面上的总进度**突然从 40% 跳回 10%**，用户看到"下载倒退了"。
        let mut p = Progress::new(Some(1000));
        p.add(100);
        assert_eq!((p.file_bytes, p.session_bytes), (100, 100));
        assert_eq!(p.ratio(), Some(0.1));

        // 续传：已有 400 字节
        p.resume_from(400);
        assert_eq!(p.file_bytes, 400, "总进度含历史");
        assert_eq!(p.session_bytes, 0, "本次会话从 0 开始");
        assert_eq!(p.ratio(), Some(0.4), "进度条不该倒退");

        // 继续下 50
        p.add(50);
        assert_eq!((p.file_bytes, p.session_bytes), (450, 50));
        assert_eq!(p.ratio(), Some(0.45));
    }

    #[test]
    fn 总大小未知时比例是_none_而不是零() {
        // 返回 0.0 会让界面画一个"永远 0%"的进度条，
        // 而用户会以为卡住了。`None` 让界面能用"不确定"的呈现。
        let p = Progress::new(None);
        assert_eq!(p.ratio(), None);
        assert!(!p.is_complete());
    }

    #[test]
    fn 零字节文件算完整() {
        let p = Progress::new(Some(0));
        assert_eq!(p.ratio(), Some(1.0));
        assert!(p.is_complete());
    }

    #[test]
    fn 超出总大小不会画出超过百分之百的进度条() {
        let mut p = Progress::new(Some(100));
        p.add(500);
        assert_eq!(p.ratio(), Some(1.0));
        assert!(p.is_complete());
    }

    // ───────────────── 节流（I6）─────────────────

    #[test]
    fn 第一次总是放行且随后立刻节流() {
        // 两件事都要成立，而它们看起来矛盾：
        //  - **第一次必须放行**，否则界面会有一段"完全没有进度"的时间，用户以为卡住；
        //  - **随后必须立刻节流**，否则每秒几千个字节事件会把界面冲垮。
        //
        // ⚠️ 这条测试的第一版把第二句写成了 `assert!(t.allow(50))` ——
        // 那等于断言"节流不生效"。我当时在断言消息里还留了个问号
        // （"第一次（last=0）之后应当立刻又能发一次？"），而**带问号的断言就是没想清**。
        let mut t = Throttle::new(THROTTLE_ENGINE_MS);
        assert!(t.allow(1000), "第一次必须放行");
        assert!(!t.allow(1050), "50ms 后不该再放行（间隔是 200ms）");
        assert!(!t.allow(1199), "还没到 200ms");
        assert!(t.allow(1200), "刚好到间隔，应当放行");
    }

    #[test]
    fn 节流窗口内的后续事件被挡住() {
        let mut t = Throttle::new(200);
        assert!(t.allow(1000));
        assert!(!t.allow(1050), "50ms < 200ms，应当被挡");
        assert!(!t.allow(1199));
        assert!(t.allow(1200), "刚好 200ms，应当放行");
    }

    #[test]
    fn 终态必须无条件放行() {
        // 否则一个恰好在节流窗口内到达的"完成"事件会被丢掉，
        // 而界面上任务会永远停在 99%。
        let mut t = Throttle::new(200);
        assert!(t.allow(1000));
        assert!(!t.allow(1010));
        assert!(t.force(1010), "force 必须放行");
        // 而且它会更新窗口
        assert!(!t.allow(1050));
    }

    #[test]
    fn 时钟为零也不会让节流失效() {
        // 有些平台上的单调时钟精度很粗（例如 15ms），而且它**可能从 0 开始**。
        //
        // ⚠️ 这条测试的第一版写错了：它先 `allow(0)` 再断言 `!allow(0)`，
        // 而那次 `allow(0)` 恰好把 `last_emit_ms` 设成 0 —— 于是断言失败。
        // 而失败的原因不是代码错，是**测试自己制造了一个歧义状态**。
        // 改成按真实语义测：从 0 开始，两次 tick 之间必须被挡住。
        let mut t = Throttle::new(200);
        assert!(t.allow(0), "第一次必须放行（否则界面会有一段完全没有进度）");
        assert!(!t.allow(0), "同一时刻不该重复放行");
        assert!(!t.allow(100), "100ms < 200ms，应当被挡");
        assert!(t.allow(200), "刚好 200ms，应当放行");
        assert!(!t.allow(200), "同一时刻不该重复放行");
    }

    // ───────────────── 续传判定（I5）─────────────────

    #[test]
    fn 全部信号齐全才允许续传() {
        let ok = ServerResumeSignals {
            accept_ranges_bytes: true,
            content_encoding_identity: true,
            has_content_length: true,
            strong_etag: Some("\"abc\"".to_string()),
            last_modified: None,
        };
        assert_eq!(ok.verdict(), ResumeVerdict::Ok);

        // 用 Last-Modified 代替强 ETag 也可以
        let ok2 = ServerResumeSignals {
            strong_etag: None,
            last_modified: Some("Wed, 21 Oct 2026 07:28:00 GMT".to_string()),
            ..ok.clone()
        };
        assert_eq!(ok2.verdict(), ResumeVerdict::Ok);
    }

    #[test]
    fn 任一缺失就放弃续传并说清缺什么() {
        let base = ServerResumeSignals {
            accept_ranges_bytes: true,
            content_encoding_identity: true,
            has_content_length: true,
            strong_etag: Some("\"x\"".to_string()),
            last_modified: None,
        };
        let cases: [(ServerResumeSignals, &str); 4] = [
            (
                ServerResumeSignals {
                    accept_ranges_bytes: false,
                    ..base.clone()
                },
                "accept-ranges",
            ),
            (
                ServerResumeSignals {
                    content_encoding_identity: false,
                    ..base.clone()
                },
                "Content-Encoding",
            ),
            (
                ServerResumeSignals {
                    has_content_length: false,
                    ..base.clone()
                },
                "Content-Length",
            ),
            (
                ServerResumeSignals {
                    strong_etag: None,
                    last_modified: None,
                    ..base.clone()
                },
                "ETag",
            ),
        ];
        for (sig, needle) in cases {
            match sig.verdict() {
                ResumeVerdict::Restart { missing } => {
                    assert!(
                        missing.iter().any(|m| m.contains(needle)),
                        "缺失项里应当提到 {needle}：{missing:?}"
                    );
                }
                other => panic!("应当 Restart，实际 {other:?}"),
            }
        }
    }

    #[test]
    fn 服务端忽略_range_的形态被判为必须重头下() {
        // I4：**必须删掉临时文件重头下**，不许接着写。
        // 一个"接着写"的实现会在临时文件后面追加一份从头开始的数据，
        // 产出**长度对、内容错**的文件 —— 而它随后被校验拦住，
        // 表现为"反复校验失败"，真正的原因在几百行外。
        let sig = ServerResumeSignals::ignored_range();
        assert!(matches!(sig.verdict(), ResumeVerdict::Restart { .. }));
    }

    #[test]
    fn 多种缺失会一次全报出来() {
        // 只报一个的话，用户修一个再试一次又失败 ——
        // 那会把一次能说完的事变成 N 轮。
        let sig = ServerResumeSignals {
            accept_ranges_bytes: false,
            content_encoding_identity: false,
            has_content_length: false,
            strong_etag: None,
            last_modified: None,
        };
        match sig.verdict() {
            ResumeVerdict::Restart { missing } => assert_eq!(missing.len(), 4, "{missing:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn 判定顺序把最常见的原因放前面() {
        // "服务端根本不支持 Range"是最常见且无需排查的原因；
        // 把"缺 ETag"报在前面会让用户去查一个次要条件。
        let sig = ServerResumeSignals {
            accept_ranges_bytes: false,
            content_encoding_identity: true,
            has_content_length: true,
            strong_etag: None,
            last_modified: None,
        };
        match sig.verdict() {
            ResumeVerdict::Restart { missing } => {
                assert!(missing[0].contains("accept-ranges"), "{missing:?}");
            }
            other => panic!("{other:?}"),
        }
    }
}
