//! # 任务队列与进度（**纯规则，零 IO**）
//!
//! M1 交付物之一。设计规格 §7 给了三条继承自参照实现的纪律，本模块把它们落地：
//!
//! | # | 纪律（规格原文） | 落点 |
//! |---|---|---|
//! | 1 | **`EventSink` 是 trait，契约写明"同步 fire-and-forget，实现不得阻塞或长持锁"** | [`EventSink`] |
//! | 2 | 本 crate **重导出自己的** HTTP client 类型，宿主不得混用 | 不适用（我们只有一个 `http::Transport`） |
//! | 3 | **`fraction: Option<f64>`，`None` 表示"总量未知"** | [`TaskProgress::fraction`] |
//!
//! ## 第 3 条值得单独说一句
//!
//! 规格写：*Axolotl 约定 `None` 为完成；**我们改成 `None = 未知总量**，
//! **因为"未知总量"在我们场景里更常见** —— 镜像不返回 `Content-Length` 时。*
//!
//! 也就是说：**同一个 `Option` 的语义在两个项目里必须相反**，
//! 而理由是**各自场景里"哪个更常见"**。这不是口味问题 ——
//! 若照抄 Axolotl 的约定，那么**每一个镜像下载都会在结束时显示 0%**，
//! 因为它们的 `fraction` 一直是 `None` 直到完成。
//!
//! ## 为什么调度是纯函数
//!
//! 因为"并发闸门 + 依赖 + 取消"三件事叠起来是**竞态的高发区**：
//! 一个任务被取消时，它的依赖者该不该启动？一个依赖者被取消时，
//! 它自己的依赖者怎么办？这些问题的答案必须是**可穷举的**，
//! 而不是"跑起来看看"。所以 [`Scheduler`] 只产出"下一批该跑哪些"，
//! 而**不自己跑**。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 任务标识。**用自增整数而不是 UUID**：
/// 它只在一次会话内有效（跨会话的集合是"待下载清单"，那是另一件事），
/// 而 UUID 会让日志与测试里的对照变得难读。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TaskId(pub u64);

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "T{}", self.0)
    }
}

/// 任务状态（复用下载引擎 `TaskState` 的语义，但队列层要多两个）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum QueueState {
    /// **依赖还没满足** —— 与 `Ready` 的区别决定"能不能派发"
    Blocked,
    /// 依赖已满足，等并发闸门放行
    Ready,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl QueueState {
    pub const fn key(self) -> &'static str {
        match self {
            QueueState::Blocked => "blocked",
            QueueState::Ready => "ready",
            QueueState::Running => "running",
            QueueState::Done => "done",
            QueueState::Failed => "failed",
            QueueState::Cancelled => "cancelled",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            QueueState::Done | QueueState::Failed | QueueState::Cancelled
        )
    }

    /// 它是否算"成功结束"（依赖判断只看这个）。
    pub const fn is_success(self) -> bool {
        matches!(self, QueueState::Done)
    }

    /// 它是否占了并发额度。
    pub const fn occupies_slot(self) -> bool {
        matches!(self, QueueState::Running)
    }
}

/// 一个任务的登记项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskEntry {
    pub id: TaskId,
    /// 给人看的名字（日志与界面都要它）
    pub label: String,
    /// **它依赖哪些任务先成功。**
    pub deps: Vec<TaskId>,
    /// 它属于哪个"组"（用于界面按组折叠，例如"这个实例的全部库文件"）
    pub group: Option<String>,
    pub state: QueueState,
    /// 失败原因（终止态才有意义）
    pub reason: Option<String>,
}

/// 进度快照（**两个字节口径 + `fraction: Option<f64>`**）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskProgress {
    /// **文件总进度**（含续传历史）—— 进度条用它
    pub file_bytes: u64,
    /// **本次会话新增**
    pub session_bytes: u64,
    /// 文件总大小。`None` = **未知**（规格 §7 第 3 条）
    pub total: Option<u64>,
}

/// 一行进度的总量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    pub tasks_total: u64,
    pub tasks_done: u64,
    pub tasks_failed: u64,
    pub tasks_cancelled: u64,
    pub bytes_total: Option<u64>,
    pub bytes_done: u64,
    pub session_bytes: u64,
}

impl Totals {
    /// **完成比例。`None` = 总量未知。**
    ///
    /// ⚠️ **这里就是规格 §7 第 3 条的落点**：`None` 表示"未知总量"，
    /// **不是**"已完成"。规格原文说明为什么我们与 Axolotl 相反：
    /// *"因为'未知总量'在我们场景里更常见 —— 镜像不返回 `Content-Length` 时。"*
    ///
    /// 若照抄 Axolotl（`None` = 完成），那么**每一个镜像下载都会在结束时显示 0%**。
    pub fn fraction(&self) -> Option<f64> {
        match self.bytes_total {
            Some(t) if t > 0 => Some((self.bytes_done as f64 / t as f64).clamp(0.0, 1.0)),
            // 总量未知 → **返回 None 而不是猜一个**
            Some(_) => Some(1.0),
            None => None,
        }
    }

    /// 还在场上（未到终态）的任务数。
    pub fn running_or_waiting(&self) -> u64 {
        self.tasks_total
            .saturating_sub(self.tasks_done)
            .saturating_sub(self.tasks_failed)
            .saturating_sub(self.tasks_cancelled)
    }
}

/// **事件出口**（规格 §7 第 1 条）。
///
/// ## 契约（规格原文，必须写在 trait 上）
///
/// > **同步 fire-and-forget，实现不得阻塞或长持锁**
///
/// 这两句话各自防一件事：
///
/// | 要求 | 不遵守会怎样 |
/// |---|---|
/// | **同步** | 用异步通道会让"事件顺序"与"状态变化顺序"解耦，而界面会看到**倒退的进度** |
/// | **fire-and-forget** | 实现若返回值给调度器，调度就会依赖界面 —— 那是反向依赖 |
/// | **不得阻塞** | 实现里做 UI 重排或网络请求会**拖慢下载本身** |
/// | **不得长持锁** | 实现若在持内部锁时调 `emit`，而 `emit` 又反过来查调度器 → **死锁** |
///
/// **这些不是建议，是 trait 的契约。** 一条"实现方自己注意"的纪律
/// 等于没有纪律 —— 所以它写在 trait 文档里，并且 [`super::NoopSink`] 是一个合规的最小实现。
pub trait EventSink: Send + Sync {
    /// 一个任务的状态变了。
    fn task_state(&self, id: TaskId, label: &str, state: QueueState);

    /// 进度更新。**引擎侧已经节流过**（规格 §2 的 I6 双层节流），
    /// 所以实现**不要**再假定"它很少被调用"。
    fn progress(&self, totals: &Totals);
}

/// 一个什么都不做的出口（测试与"不需要事件"的场合用）。
///
/// 它存在的意义：让 [`Scheduler`] 的每个调用点都能传一个合法值，
/// 而不必到处写 `Option<&dyn EventSink>` —— 后者会让每个事件点都多一个分支。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopSink;

impl EventSink for NoopSink {
    fn task_state(&self, _: TaskId, _: &str, _: QueueState) {}
    fn progress(&self, _: &Totals) {}
}

/// 一个**把事件收集起来**的出口（测试用）。
///
/// ⚠️ 它住在生产代码里而不是 `#[cfg(test)]`，理由是**它有用**：
/// 诊断模式可以把事件流也记进日志。而把它藏在测试里会让
/// "诊断要记录事件流"这件事需要重写一遍。
#[derive(Debug, Default)]
pub struct RecordingSink {
    states: std::sync::Mutex<Vec<(TaskId, QueueState)>>,
    progress: std::sync::Mutex<Vec<Totals>>,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn states(&self) -> Vec<(TaskId, QueueState)> {
        self.states.lock().map(|v| v.clone()).unwrap_or_default()
    }
    pub fn progress_calls(&self) -> usize {
        self.progress.lock().map(|v| v.len()).unwrap_or(0)
    }
}

impl EventSink for RecordingSink {
    fn task_state(&self, id: TaskId, _label: &str, state: QueueState) {
        if let Ok(mut v) = self.states.lock() {
            v.push((id, state));
        }
    }
    fn progress(&self, totals: &Totals) {
        if let Ok(mut v) = self.progress.lock() {
            v.push(*totals);
        }
    }
}

/// 加任务时的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddError {
    /// 依赖不存在的任务
    UnknownDep { dep: TaskId },
    /// 自环
    SelfDep,
    /// **会形成环**。带上一段能看懂的环路径。
    Cycle { path: Vec<TaskId> },
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddError::UnknownDep { dep } => write!(f, "依赖了不存在的任务 {dep}"),
            AddError::SelfDep => write!(f, "任务不能依赖自己"),
            AddError::Cycle { path } => {
                let s: Vec<String> = path.iter().map(|t| t.to_string()).collect();
                write!(f, "依赖成环：{}", s.join(" → "))
            }
        }
    }
}

impl std::error::Error for AddError {}

/// **调度器**：只决定"下一批该跑哪些"，**不自己跑**。
///
/// 这样做的收益很具体：**并发闸门 + 依赖 + 取消** 三件事叠起来是竞态高发区，
/// 而做成纯函数之后，全部组合都可以被穷举断言。
#[derive(Debug, Default)]
pub struct Scheduler {
    next_id: u64,
    tasks: BTreeMap<TaskId, TaskEntry>,
    /// 并发闸门：最多这么多任务同时 `Running`
    concurrency: u32,
    /// 进度累计
    bytes_done: u64,
    session_bytes: u64,
    /// 总量是否**全都已知**（有一个未知就整体未知）
    bytes_total_known: bool,
    bytes_total: u64,
}

impl Scheduler {
    /// 建一个调度器。`concurrency` 为 0 会被抬到 1 ——
    /// 一个"永远不会派发任何任务"的调度器**不是合法配置**，
    /// 而它会表现为"下载卡住"，极难归因。
    pub fn new(concurrency: u32) -> Self {
        Self {
            next_id: 1,
            tasks: BTreeMap::new(),
            concurrency: concurrency.max(1),
            bytes_done: 0,
            session_bytes: 0,
            bytes_total_known: true,
            bytes_total: 0,
        }
    }

    pub fn concurrency(&self) -> u32 {
        self.concurrency
    }

    /// 是否可以调高并发（自适应爬坡的输出会喂到这里）。
    pub fn set_concurrency(&mut self, n: u32) {
        self.concurrency = n.max(1);
    }

    /// 加一个任务。**返回它的 id**。
    ///
    /// 三条校验都在这里做，因为**它们都是"加的时候就错"的错误**：
    ///
    /// | 校验 | 若不在加的时候做 |
    /// |---|---|
    /// | 依赖存在 | 会在调度时静默地永远不满足 |
    /// | 无自环 | 同 |
    /// | 无环 | 同 —— **而"永远不满足"的表现是"下载卡住"，不是报错** |
    pub fn add(
        &mut self,
        label: impl Into<String>,
        deps: Vec<TaskId>,
        group: Option<String>,
    ) -> Result<TaskId, AddError> {
        for d in &deps {
            if !self.tasks.contains_key(d) {
                return Err(AddError::UnknownDep { dep: *d });
            }
        }
        let id = TaskId(self.next_id);
        if deps.contains(&id) {
            return Err(AddError::SelfDep);
        }
        // 有依赖就意味着"加的时候它还 Blocked" —— 至于状态由 `refresh` 统一算，
        // 因为"依赖是否已满足"是**会随别的任务完成而变**的。
        if !deps.is_empty() {
            if let Some(path) = self.find_cycle(id, &deps) {
                return Err(AddError::Cycle { path });
            }
        }
        self.next_id += 1;
        self.tasks.insert(
            id,
            TaskEntry {
                id,
                label: label.into(),
                deps,
                group,
                // 先标 Blocked，交给 `refresh` 算成 ReadY 或留 Blocked。
                // **不在这里自己算**：算状态的逻辑只能有一处。
                state: QueueState::Blocked,
                reason: None,
            },
        );
        self.refresh();
        Ok(id)
    }

    /// 新任务是否会造成环（它依赖 `deps`，而 `deps` 里有没有转回来指向它）。
    ///
    /// 因为新任务还没有"被依赖"，所以只有一种环：**它的某个依赖（传递地）依赖它** ——
    /// 而它还没入表，所以那个依赖不可能指向它。**结论是此刻不可能成环。**
    ///
    /// 但**这个函数仍然保留**，因为上面的论证依赖"新任务尚未在表里"这个实现细节，
    /// 而那个细节会在某次重构里被无声地破坏。这里改成**主动做一次可达性检查**，
    /// 于是它对实现细节不敏感。
    fn find_cycle(&self, _id: TaskId, deps: &[TaskId]) -> Option<Vec<TaskId>> {
        // 从每个依赖出发做 DFS，看能不能回到任一依赖 —— 那说明旧图里已有环
        // （旧图有环是我们自己的 bug，但**报出来比静默卡住好**）。
        for start in deps {
            let mut stack = vec![*start];
            let mut seen = Vec::new();
            while let Some(cur) = stack.pop() {
                if seen.contains(&cur) {
                    let mut path = seen.clone();
                    path.push(cur);
                    return Some(path);
                }
                seen.push(cur);
                if let Some(t) = self.tasks.get(&cur) {
                    for d in &t.deps {
                        stack.push(*d);
                    }
                }
            }
        }
        None
    }

    /// 查一个任务。
    pub fn get(&self, id: TaskId) -> Option<&TaskEntry> {
        self.tasks.get(&id)
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// 全部任务（按 id 有序 —— `BTreeMap` 保证）。
    pub fn iter(&self) -> impl Iterator<Item = &TaskEntry> {
        self.tasks.values()
    }

    /// **重算全部任务的"就绪性"**。
    ///
    /// 它把 `Blocked` ↔ `Ready` 之间的迁移一次算完，而 `Running` 与终态**不动**。
    ///
    /// 三种依赖结果：
    ///
    /// | 依赖里出现 | 该任务变成 | 为什么 |
    /// |---|---|---|
    /// | 有 `Failed`/`Cancelled` | **`Cancelled`** | 依赖不会成功了，等下去是**永远卡住** |
    /// | 有非终态 | 留 `Blocked` | 还有希望 |
    /// | 全部 `Done` | `Ready` | 可以跑了 |
    ///
    /// ## ⚠️ 它必须**迭代到不动点**（这一条是测试逼出来的）
    ///
    /// 第一版只做**一趟**：对每个任务读"依赖们的**当前**状态"。
    /// 于是 `a → b → c` 里 `a` 失败时：
    ///
    /// ```text
    ///     处理 b：依赖 a 是 Failed（终态）→ b 变 Cancelled  ✅
    ///     处理 c：依赖 b 是 **Blocked**（本趟开始时的旧值）→ c 留 Blocked  ❌
    /// ```
    ///
    /// 也就是**级联只传一跳**。而它的表现是：进度条永远停在某个百分比，
    /// 而用户不知道在等什么 —— 正是这条规则存在的理由被破坏掉了。
    ///
    /// 迭代到不动点之后，传递下游也能被正确取消。因为依赖图**无环**
    /// （`add` 会拒绝成环的依赖），所以它**一定收敛**；下面那个 `guard`
    /// 只是防御"某天 `add` 的检查被绕过"。
    pub fn refresh(&mut self) {
        // 无环图里传播最多 N 跳（N = 任务数），而每跳我们重算一遍全部任务。
        // 取 `2N + 4` 是**明确地宽裕**：目标不是"刚好够"，而是"不可能因为
        // 算错一跳而静默停住"。真正的环由 `add` 拒绝，这里只是防御。
        let mut guard = self.tasks.len() * 2 + 4;
        loop {
            guard -= 1;
            if guard == 0 {
                return;
            }
            let states: BTreeMap<TaskId, QueueState> =
                self.tasks.iter().map(|(k, v)| (*k, v.state)).collect();
            let mut changed = false;
            for t in self.tasks.values_mut() {
                if t.state.is_terminal() || t.state == QueueState::Running {
                    continue;
                }
                let mut blocked = false;
                let mut doomed = false;
                for d in &t.deps {
                    match states.get(d) {
                        Some(QueueState::Done) => {}
                        Some(s) if s.is_terminal() => {
                            doomed = true;
                            break;
                        }
                        _ => blocked = true,
                    }
                }
                let next = if doomed {
                    // **级联取消**：依赖永远不会成功了。
                    // 标成 `Cancelled` 而不是一直 `Blocked` ——
                    // 后者会让"进度条永远停在 90%"而用户不知道在等什么。
                    QueueState::Cancelled
                } else if blocked {
                    QueueState::Blocked
                } else {
                    QueueState::Ready
                };
                if next != t.state {
                    t.state = next;
                    changed = true;
                }
            }
            if !changed {
                return;
            }
        }
    }

    /// **下一批该跑哪些**（受并发闸门限制）。
    ///
    /// 返回值**不改变状态** —— 调用方跑完之后自己调 [`Scheduler::mark_running`]。
    /// 这与"产出计划"和"执行计划"分开是同一条纪律（见 `plan.rs`）。
    pub fn next_batch(&self) -> Vec<TaskId> {
        let running = self
            .tasks
            .values()
            .filter(|t| t.state.occupies_slot())
            .count() as u32;
        let free = self.concurrency.saturating_sub(running);
        if free == 0 {
            return Vec::new();
        }
        self.tasks
            .values()
            .filter(|t| t.state == QueueState::Ready)
            .take(free as usize)
            .map(|t| t.id)
            .collect()
    }

    /// 标成运行中。
    pub fn mark_running(&mut self, id: TaskId, sink: &dyn EventSink) {
        if let Some(t) = self.tasks.get_mut(&id) {
            if t.state == QueueState::Ready {
                t.state = QueueState::Running;
                sink.task_state(id, &t.label, QueueState::Running);
            }
        }
    }

    /// 标成完成并**刷新依赖**（让依赖它的任务变成 Ready）。
    pub fn mark_done(&mut self, id: TaskId, sink: &dyn EventSink) {
        if let Some(t) = self.tasks.get_mut(&id) {
            if t.state.is_terminal() {
                return;
            }
            t.state = QueueState::Done;
            let label = t.label.clone();
            sink.task_state(id, &label, QueueState::Done);
        }
        self.refresh();
        // 刷新可能让一批任务从 Blocked 变 Ready —— 那个变化也要报出去，
        // 否则界面上的"等待中"会停在旧状态。
        self.emit_ready(sink);
    }

    /// 标成失败。**依赖它的任务会被级联取消。**
    pub fn mark_failed(&mut self, id: TaskId, reason: impl Into<String>, sink: &dyn EventSink) {
        let reason = reason.into();
        let mut affected = Vec::new();
        if let Some(t) = self.tasks.get_mut(&id) {
            if t.state.is_terminal() {
                return;
            }
            t.state = QueueState::Failed;
            t.reason = Some(reason);
            let label = t.label.clone();
            sink.task_state(id, &label, QueueState::Failed);
        }
        // 先 refresh，再找出**被级联取消的**那些并报出去
        let before: BTreeMap<TaskId, QueueState> =
            self.tasks.iter().map(|(k, v)| (*k, v.state)).collect();
        self.refresh();
        for (k, v) in &self.tasks {
            if before.get(k) != Some(&v.state) && v.state == QueueState::Cancelled {
                affected.push((*k, v.label.clone()));
            }
        }
        for (k, label) in affected {
            sink.task_state(k, &label, QueueState::Cancelled);
        }
        self.emit_ready(sink);
    }

    /// 撤销一个任务（用户主动取消）。
    ///
    /// **与失败同一条级联规则**：依赖它的任务也不能再等了。
    pub fn cancel(&mut self, id: TaskId, sink: &dyn EventSink) {
        if let Some(t) = self.tasks.get_mut(&id) {
            if t.state.is_terminal() {
                return;
            }
            t.state = QueueState::Cancelled;
            let label = t.label.clone();
            sink.task_state(id, &label, QueueState::Cancelled);
        }
        let before: BTreeMap<TaskId, QueueState> =
            self.tasks.iter().map(|(k, v)| (*k, v.state)).collect();
        self.refresh();
        for (k, v) in &self.tasks {
            if before.get(k) != Some(&v.state) && v.state == QueueState::Cancelled {
                let label = v.label.clone();
                sink.task_state(*k, &label, QueueState::Cancelled);
            }
        }
        self.emit_ready(sink);
    }

    /// 把"刚变成 Ready"的任务报出去。
    fn emit_ready(&self, sink: &dyn EventSink) {
        for t in self.tasks.values() {
            if t.state == QueueState::Ready {
                sink.task_state(t.id, &t.label, QueueState::Ready);
            }
        }
    }

    /// 更新一个任务的进度。
    ///
    /// `total` 的合并规则：**有一个未知就整体未知** ——
    /// 因为"部分已知"的进度条会出现"先显示 60% 然后跳回未知"这种倒退。
    pub fn update_progress(&mut self, file_bytes: u64, session_bytes: u64, total: Option<u64>) {
        // `file_bytes` 是**这个任务的文件总进度**（含续传历史），而多个任务会累加 ——
        // 所以这里的语义是"把这份文件的当前进度并入总量"。
        //
        // ⚠️ 它**不是**幂等的：同一个任务报告两次 400 会累成 800。
        // 调用方应当传**增量**（`Progress::session_bytes`），或用
        // [`Scheduler::set_task_progress`] 按任务 id 覆盖。
        self.bytes_done = self.bytes_done.saturating_add(file_bytes);
        self.session_bytes = self.session_bytes.saturating_add(session_bytes);
        match total {
            Some(t) => {
                if self.bytes_total_known {
                    self.bytes_total = self.bytes_total.saturating_add(t);
                }
            }
            None => {
                // 有一个未知 ⇒ 整体未知
                self.bytes_total_known = false;
            }
        }
    }

    /// 当前总量快照。
    pub fn totals(&self) -> Totals {
        let mut t = Totals {
            tasks_total: self.tasks.len() as u64,
            bytes_done: self.bytes_done,
            session_bytes: self.session_bytes,
            bytes_total: if self.bytes_total_known {
                Some(self.bytes_total)
            } else {
                None
            },
            ..Default::default()
        };
        for e in self.tasks.values() {
            match e.state {
                QueueState::Done => t.tasks_done += 1,
                QueueState::Failed => t.tasks_failed += 1,
                QueueState::Cancelled => t.tasks_cancelled += 1,
                _ => {}
            }
        }
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink() -> RecordingSink {
        RecordingSink::new()
    }

    // ───────────────── 加任务的三条校验 ─────────────────

    #[test]
    fn 依赖不存在的任务会被拒绝() {
        let mut s = Scheduler::new(4);
        let e = s.add("a", vec![TaskId(99)], None).unwrap_err();
        assert!(matches!(e, AddError::UnknownDep { .. }), "{e:?}");
        assert!(!e.to_string().is_empty());
    }

    #[test]
    fn 无依赖的任务立刻变成就绪() {
        let mut s = Scheduler::new(2);
        let id = s.add("a", vec![], None).unwrap();
        assert_eq!(s.get(id).unwrap().state, QueueState::Ready);
    }

    #[test]
    fn 有依赖的任务先阻塞() {
        let mut s = Scheduler::new(2);
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![a], None).unwrap();
        assert_eq!(s.get(a).unwrap().state, QueueState::Ready);
        assert_eq!(s.get(b).unwrap().state, QueueState::Blocked);
    }

    // ───────────────── 并发闸门 ─────────────────

    #[test]
    fn 下一批受并发闸门限制() {
        let mut s = Scheduler::new(2);
        for i in 0..5 {
            s.add(format!("t{i}"), vec![], None).unwrap();
        }
        let batch = s.next_batch();
        assert_eq!(batch.len(), 2, "并发是 2，只该给 2 个");
    }

    #[test]
    fn 已经在跑的任务占住额度() {
        let mut s = Scheduler::new(2);
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![], None).unwrap();
        let _c = s.add("c", vec![], None).unwrap();
        let sk = sink();
        s.mark_running(a, &sk);
        s.mark_running(b, &sk);
        assert!(s.next_batch().is_empty(), "两个额度都被占了");
        // 完成一个就腾出一个
        s.mark_done(a, &sk);
        let batch = s.next_batch();
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn 并发为零被抬到一() {
        // 一个"永远不会派发任何任务"的调度器**不是合法配置**，
        // 而它会表现为"下载卡住"，极难归因。
        let s = Scheduler::new(0);
        assert_eq!(s.concurrency(), 1);
    }

    #[test]
    fn 并发可以随爬坡调整() {
        let mut s = Scheduler::new(4);
        s.set_concurrency(16);
        assert_eq!(s.concurrency(), 16);
        s.set_concurrency(0);
        assert_eq!(s.concurrency(), 1, "下限仍然是 1");
    }

    #[test]
    fn 下一批不改变状态() {
        // 产出计划与执行计划分开 —— 与 `plan.rs` 同一条纪律。
        let mut s = Scheduler::new(1);
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![], None).unwrap();
        let first = s.next_batch();
        let second = s.next_batch();
        assert_eq!(first, second, "重复查应当得到同样的结果（没有副作用）");
        assert_eq!(first, vec![a], "按 id 有序");
        let _ = b;
    }

    // ───────────────── 依赖推进 ─────────────────

    #[test]
    fn 依赖完成后下游变成就绪并报出事件() {
        let mut s = Scheduler::new(4);
        let sk = sink();
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![a], None).unwrap();
        assert_eq!(s.get(b).unwrap().state, QueueState::Blocked);

        s.mark_done(a, &sk);
        assert_eq!(s.get(b).unwrap().state, QueueState::Ready);
        // **"变成 Ready"这件事也要报出去**，否则界面上的"等待中"会停在旧状态
        assert!(
            sk.states().contains(&(b, QueueState::Ready)),
            "下游就绪必须发事件：{:?}",
            sk.states()
        );
    }

    #[test]
    fn 依赖失败会级联取消下游() {
        // **这是本模块最重要的一条规则。**
        // 若下游一直 `Blocked`，进度条会**永远停在 90%** 而用户不知道在等什么。
        let mut s = Scheduler::new(4);
        let sk = sink();
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![a], None).unwrap();
        let c = s.add("c", vec![b], None).unwrap();

        s.mark_failed(a, "校验不匹配", &sk);
        assert_eq!(s.get(a).unwrap().state, QueueState::Failed);
        assert_eq!(
            s.get(b).unwrap().state,
            QueueState::Cancelled,
            "直接下游要被级联取消"
        );
        assert_eq!(
            s.get(c).unwrap().state,
            QueueState::Cancelled,
            "传递下游也要（refresh 是全局重算）"
        );
        // 级联取消也要报事件
        assert!(sk.states().contains(&(b, QueueState::Cancelled)));
        assert!(sk.states().contains(&(c, QueueState::Cancelled)));
    }

    #[test]
    fn 用户取消也会级联() {
        let mut s = Scheduler::new(4);
        let sk = sink();
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![a], None).unwrap();
        s.cancel(a, &sk);
        assert_eq!(s.get(a).unwrap().state, QueueState::Cancelled);
        assert_eq!(
            s.get(b).unwrap().state,
            QueueState::Cancelled,
            "与失败同一条级联规则"
        );
    }

    #[test]
    fn 一个失败不会影响无关的兄弟() {
        // 级联只沿依赖边走 —— 否则一次失败会把整个队列清空。
        let mut s = Scheduler::new(4);
        let sk = sink();
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![], None).unwrap();
        let a2 = s.add("a2", vec![a], None).unwrap();
        let b2 = s.add("b2", vec![b], None).unwrap();

        s.mark_failed(a, "boom", &sk);
        assert_eq!(s.get(a2).unwrap().state, QueueState::Cancelled);
        assert_eq!(s.get(b).unwrap().state, QueueState::Ready, "兄弟不受影响");
        assert_eq!(
            s.get(b2).unwrap().state,
            QueueState::Blocked,
            "仍在等它的上游"
        );
    }

    #[test]
    fn 深链的级联取消要传到底() {
        // ⚠️ 这条测试是"refresh 必须迭代到不动点"的直接验收。
        //
        // 第一版只做一趟，于是 `a → b → c` 里 `c` 看不到 `b` 的新状态，
        // **级联只传一跳**。一条 8 跳的链会让这个缺陷暴露得非常清楚 ——
        // 它也正是"进度条永远停在某个百分比"的成因。
        let mut s = Scheduler::new(4);
        let sk = sink();
        let mut chain = Vec::new();
        let mut prev: Vec<TaskId> = vec![];
        for i in 0..8 {
            let id = s.add(format!("t{i}"), prev.clone(), None).unwrap();
            prev = vec![id];
            chain.push(id);
        }
        for id in &chain[1..] {
            assert_eq!(
                s.get(*id).unwrap().state,
                QueueState::Blocked,
                "{id} 应当阻塞"
            );
        }
        s.mark_failed(chain[0], "boom", &sk);
        for (i, id) in chain.iter().enumerate().skip(1) {
            assert_eq!(
                s.get(*id).unwrap().state,
                QueueState::Cancelled,
                "第 {i} 跳（{id}）必须被级联取消 —— 只传一跳会让它留 Blocked"
            );
        }
    }

    #[test]
    fn 深链全部完成后尾端就绪() {
        // 与上一条互补：正向传播也必须到底。
        let mut s = Scheduler::new(1);
        let sk = sink();
        let mut chain = Vec::new();
        let mut prev: Vec<TaskId> = vec![];
        for i in 0..6 {
            let id = s.add(format!("t{i}"), prev.clone(), None).unwrap();
            prev = vec![id];
            chain.push(id);
        }
        for id in &chain {
            let batch = s.next_batch();
            assert_eq!(batch, vec![*id], "{id} 应当轮到它");
            s.mark_running(*id, &sk);
            s.mark_done(*id, &sk);
        }
        assert_eq!(s.totals().tasks_done, 6);
    }

    #[test]
    fn 多个依赖要全部完成才就绪() {
        let mut s = Scheduler::new(4);
        let sk = sink();
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![], None).unwrap();
        let c = s.add("c", vec![a, b], None).unwrap();
        s.mark_done(a, &sk);
        assert_eq!(s.get(c).unwrap().state, QueueState::Blocked, "还差一个");
        s.mark_done(b, &sk);
        assert_eq!(s.get(c).unwrap().state, QueueState::Ready);
    }

    #[test]
    fn 终态的任务不会被再次改动() {
        // 一个已完成的任务被"再失败一次"会让依赖它的下游被无端取消。
        let mut s = Scheduler::new(4);
        let sk = sink();
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![a], None).unwrap();
        s.mark_done(a, &sk);
        s.mark_failed(a, "迟到的失败", &sk);
        assert_eq!(s.get(a).unwrap().state, QueueState::Done, "不该被改");
        assert_eq!(s.get(b).unwrap().state, QueueState::Ready, "下游不该被取消");
    }

    #[test]
    fn 运行中的任务不会被_refresh_改回就绪() {
        let mut s = Scheduler::new(4);
        let sk = sink();
        let a = s.add("a", vec![], None).unwrap();
        s.mark_running(a, &sk);
        s.refresh();
        assert_eq!(
            s.get(a).unwrap().state,
            QueueState::Running,
            "运行中不该被动"
        );
    }

    // ───────────────── 进度与 fraction ─────────────────

    #[test]
    fn fraction_的_none_表示总量未知而不是完成() {
        // ⚠️ **规格 §7 第 3 条的落点。**
        // Axolotl 约定 `None` = 完成，而**我们改成 `None` = 未知总量** ——
        // 因为"未知总量"在我们场景里更常见（镜像不返回 `Content-Length`）。
        //
        // 若照抄 Axolotl，**每一个镜像下载都会在结束时显示 0%**，
        // 因为它们的 `fraction` 一直是 `None` 直到完成。
        let t = Totals {
            tasks_total: 10,
            tasks_done: 9,
            bytes_total: None,
            bytes_done: 12345,
            ..Default::default()
        };
        assert_eq!(
            t.fraction(),
            None,
            "总量未知时必须是 None，不是 0.0 也不是 1.0"
        );
    }

    #[test]
    fn fraction_在总量已知时是比例() {
        let t = Totals {
            bytes_total: Some(1000),
            bytes_done: 250,
            ..Default::default()
        };
        assert_eq!(t.fraction(), Some(0.25));
    }

    #[test]
    fn 有一个未知就整体未知() {
        // "部分已知"的进度条会出现"先显示 60% 然后跳回未知"这种倒退。
        let mut s = Scheduler::new(4);
        s.update_progress(0, 0, Some(1000));
        s.update_progress(0, 0, Some(500));
        assert_eq!(s.totals().bytes_total, Some(1500));
        // 来一个未知的
        s.update_progress(0, 0, None);
        assert_eq!(s.totals().bytes_total, None, "有一个未知就必须整体未知");
        // 之后再来已知的也不会"救回来"
        s.update_progress(0, 0, Some(100));
        assert_eq!(s.totals().bytes_total, None);
    }

    #[test]
    fn 零总量算完成() {
        let t = Totals {
            bytes_total: Some(0),
            bytes_done: 0,
            ..Default::default()
        };
        assert_eq!(t.fraction(), Some(1.0));
    }

    #[test]
    fn 超出总量不会画出超过百分之百() {
        let t = Totals {
            bytes_total: Some(100),
            bytes_done: 500,
            ..Default::default()
        };
        assert_eq!(t.fraction(), Some(1.0));
    }

    #[test]
    fn 两个字节口径分开累计() {
        let mut s = Scheduler::new(4);
        s.update_progress(400, 0, Some(1000));
        s.update_progress(50, 50, Some(1000));
        let t = s.totals();
        assert_eq!(t.session_bytes, 50, "本次会话只算了新增的那 50");
        assert!(t.bytes_done >= 50);
    }

    #[test]
    fn 终态计数正确() {
        let mut s = Scheduler::new(4);
        let sk = sink();
        let a = s.add("a", vec![], None).unwrap();
        let b = s.add("b", vec![], None).unwrap();
        let c = s.add("c", vec![], None).unwrap();
        s.mark_done(a, &sk);
        s.mark_failed(b, "x", &sk);
        s.cancel(c, &sk);
        let t = s.totals();
        assert_eq!(t.tasks_total, 3);
        assert_eq!(t.tasks_done, 1);
        assert_eq!(t.tasks_failed, 1);
        assert_eq!(t.tasks_cancelled, 1);
        assert_eq!(t.running_or_waiting(), 0);
    }

    // ───────────────── 状态与键 ─────────────────

    #[test]
    fn 状态键是稳定_ascii_标识() {
        let all = [
            QueueState::Blocked,
            QueueState::Ready,
            QueueState::Running,
            QueueState::Done,
            QueueState::Failed,
            QueueState::Cancelled,
        ];
        let mut keys: Vec<&str> = all.iter().map(|s| s.key()).collect();
        for k in &keys {
            assert!(k.is_ascii(), "{k}");
        }
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), n, "状态键有重复");
        // 终态与成功、占额度三个判定的语义
        for s in all {
            assert_eq!(
                s.is_terminal(),
                matches!(
                    s,
                    QueueState::Done | QueueState::Failed | QueueState::Cancelled
                )
            );
            assert_eq!(s.is_success(), matches!(s, QueueState::Done));
            assert_eq!(s.occupies_slot(), matches!(s, QueueState::Running));
        }
    }

    #[test]
    fn 任务id_的显示形式() {
        assert_eq!(TaskId(7).to_string(), "T7");
    }

    // ───────────────── 空出口 ─────────────────

    #[test]
    fn 空出口不产生任何副作用() {
        // 它让每个调用点都能传一个合法值，而不必到处写
        // `Option<&dyn EventSink>` —— 后者会让每个事件点都多一个分支。
        let mut s = Scheduler::new(2);
        let a = s.add("a", vec![], None).unwrap();
        s.mark_running(a, &NoopSink);
        s.mark_done(a, &NoopSink);
        assert_eq!(s.get(a).unwrap().state, QueueState::Done);
    }

    #[test]
    fn 记录出口能收到状态与进度() {
        let mut s = Scheduler::new(2);
        let sk = sink();
        let a = s.add("a", vec![], None).unwrap();
        s.mark_running(a, &sk);
        s.mark_done(a, &sk);
        s.update_progress(100, 100, Some(100));
        sk.progress(&s.totals());
        let st = sk.states();
        assert!(st.contains(&(a, QueueState::Running)));
        assert!(st.contains(&(a, QueueState::Done)));
        assert_eq!(sk.progress_calls(), 1);
    }
}
