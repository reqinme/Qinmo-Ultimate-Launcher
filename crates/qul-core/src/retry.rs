//! # 重试与取消策略（**纯策略，零 IO**）
//!
//! 方案 M1 的出口条件里有一句：**"并发写同一实例不出错"**，
//! 而它旁边还写着**"取消/重试语义"**。这两件事是一体两面：
//! **一个"取消"如果没规定"已完成的那些字节怎么办"，就会变成"重试时从头再来"**——
//! 那在 0.28 MB/s 的链路上等于把 33 分钟变成 66 分钟。
//!
//! ## 为什么策略要在内核而 IO 在 infra
//!
//! "第几次重试、退避多久、什么错误该重试"是**可以穷举验证的规则**；
//! "怎么写盘、怎么加锁"**只能实测**。混在一起就没法对规则本身写测试。
//! 这与 `java.rs` 的切分是同一个理由（见 `qul-core/src/java.rs` 的模块文档）。
//!
//! ## 一条纪律：**取消不是失败**
//!
//! 用户按了取消，是**用户的意愿**，不是系统出了问题。
//! 所以本模块把"取消"与"错误"分成两个类型——
//! **它们会走到界面的不同级别**（取消 → `Silent`，错误 → 看具体码）。

use serde::{Deserialize, Serialize};

/// 退避策略。**刻意只有两种**：固定与指数。
///
/// 不加"线性"或"抖动"这些选项，因为**没有数据支持它们**，
/// 而多一个选项就多一条没人验证的路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Backoff {
    /// 固定间隔。用于**已知会很快恢复**的情形（如短超时）。
    Fixed { ms: u64 },
    /// 指数退避，**带上限**。上限是必须的：
    /// 没有上限的话第 10 次重试要等 17 分钟，用户看到的是"卡死"。
    Exponential {
        first_ms: u64,
        factor: u32,
        cap_ms: u64,
    },
}

impl Backoff {
    /// 第 `attempt` 次重试（**从 0 开始**）之前应等待的毫秒数。
    ///
    /// `attempt = 0` 表示"第一次重试之前"，通常不该等待——**先立刻试一次**。
    /// 因为很多失败是瞬时的（连接被复用后失效），立刻重试就成功了，
    /// 而等一秒再试只是让用户多等一秒。
    pub fn delay_ms(self, attempt: u32) -> u64 {
        match self {
            Backoff::Fixed { ms } => {
                if attempt == 0 {
                    0
                } else {
                    ms
                }
            }
            Backoff::Exponential {
                first_ms,
                factor,
                cap_ms,
            } => {
                if attempt == 0 {
                    return 0;
                }
                // 用 checked 运算：attempt 很大时不能溢出成一个小数，
                // 那会让"最后一次重试"变成"立刻重试"，反而更糟。
                let mut v = first_ms;
                for _ in 1..attempt {
                    match v.checked_mul(u64::from(factor)) {
                        Some(next) => v = next,
                        None => return cap_ms,
                    }
                }
                v.min(cap_ms)
            }
        }
    }
}

/// 重试策略：最多几次、怎么退避、哪些错误值得重试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// **总尝试次数**（含第一次）。1 表示不重试。
    pub max_attempts: u32,
    pub backoff: Backoff,
}

impl RetryPolicy {
    /// 默认网络重试：**6 次尝试（= 5 次重试）+ 指数退避（0 / 2 / 4 / 8 / 16 / 30 s）**。
    ///
    /// ## ⚠️ 这两个值在 M1 下载引擎落地时被**更正过一次**
    ///
    /// 第一版写的是 **4 次尝试 / 500ms 起点**，而《下载引擎设计规格》§4 的表格写的是
    /// **最大重试 5**（NexBox `:822`；HMCL 也用 5）与**退避基础 2s**（NexBox `:823`，
    /// 明确优于 HMCL 的固定 `Thread.sleep(200)`）。
    ///
    /// **偏差是怎么被发现的**：写 `qul_core::timeout::TimeoutPolicy` 时
    /// 把规格的取值抄成常量并加了断言，于是两处数字**并排出现**，
    /// 差一个立刻可见。
    ///
    /// **教训**：同一个数字在两处定义时，**它们的差值不会自己浮现** ——
    /// 必须有一个地方**把两个定义放在一起看**（这里是测试里的常量断言）。
    ///
    /// 总等待 = 0+2+4+8+16+30 = **60 秒**（有封顶；没有封顶的话是 62 秒，
    /// 而那条差别不重要——重要的是**没有封顶时它会随重试次数无限增长**）。
    pub const fn default_for_network() -> Self {
        Self {
            // 6 = 5 次重试 + 第一次。**`max_attempts` 含第一次**，
            // 这一条很容易搞反，而搞反的后果是"少试一次"。
            max_attempts: 6,
            backoff: Backoff::Exponential {
                first_ms: 2_000,
                factor: 2,
                cap_ms: 30_000,
            },
        }
    }

    /// 不重试。用于**重试没有意义**的操作（如"路径越界"）。
    pub const fn none() -> Self {
        Self {
            max_attempts: 1,
            backoff: Backoff::Fixed { ms: 0 },
        }
    }

    /// **还有没有尝试额度？**（`attempts_done` 是**已完成的**尝试次数）
    ///
    /// 这个函数名改过一次：原来叫 `should_retry`，于是 `max_attempts = 1`
    /// 时 `should_retry(0)` 返回 `true` 看起来像 bug——其实不是：
    /// **"已完成 0 次"意味着第一次还没做，当然该做。**
    /// 名字让人误判语义，就是名字的错，所以改成现在这个不会误读的形式。
    ///
    /// 换句话说：`max_attempts` 是**总尝试次数（含第一次）**，
    /// 不是"重试次数"。想表达"不重试"就填 `max_attempts = 1`。
    pub fn has_attempt_left(&self, attempts_done: u32) -> bool {
        attempts_done < self.max_attempts
    }

    /// 还要等多久才该做第 `attempts_done + 1` 次尝试。
    pub fn delay_before(&self, attempts_done: u32) -> u64 {
        self.backoff.delay_ms(attempts_done)
    }

    /// 已用完的尝试次数达到上限时的总等待（用于预估"最坏要等多久"）。
    pub fn total_wait_ms(&self) -> u64 {
        (1..self.max_attempts)
            .map(|a| self.backoff.delay_ms(a))
            .sum()
    }
}

/// 一次操作的结局。**三态，不是 `Result`**——
/// 因为"用户取消"既不是成功也不是失败，而把它塞进 `Err` 会让
/// 上层把它当故障上报（然后用户会被自己按的取消弹一次窗）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T> {
    Done(T),
    /// **用户取消。不是错误。**
    Cancelled,
    Failed(String),
}

impl<T> Outcome<T> {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Outcome::Cancelled)
    }
    pub fn into_result(self) -> Result<T, String> {
        match self {
            Outcome::Done(v) => Ok(v),
            Outcome::Cancelled => Err("已取消".to_string()),
            Outcome::Failed(e) => Err(e),
        }
    }
}

/// 取消信号。**用 `AtomicBool` 而不是 `Channel`**：
/// 取消是"一次性置位、多处读取"的语义，用通道反而要处理"谁先读到"。
#[derive(Debug, Default)]
pub struct CancelToken {
    flag: std::sync::atomic::AtomicBool,
}

impl CancelToken {
    pub const fn new() -> Self {
        Self {
            flag: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// 请求取消。**可重复调用**（用户可能连点几次）。
    pub fn cancel(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// 可中断的下载点：**在一个已写好的字节数处继续**。
///
/// 这是"取消可恢复"的关键数据。方案 §M1 要求**可中断可恢复**，
/// 而"恢复"需要的不是"从头再来"，是**记得上次写到哪**。
///
/// **注意 `bytes_done` 是"已完整落盘的字节数"**，不是"已收到但还在缓冲区的"——
/// 两者的差在断电时会变成**文件里一段损坏的数据**，
/// 而校验和会把它当成"下载失败"，于是**全部重来**。
/// 所以增量必须是"已经过的校验的那部分"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumePoint {
    pub bytes_done: u64,
    /// 对端是否支持 Range（决定能不能续传）
    pub range_supported: bool,
}

impl ResumePoint {
    pub const fn start() -> Self {
        Self {
            bytes_done: 0,
            range_supported: false,
        }
    }

    /// 该从哪个字节开始请求。
    ///
    /// **对端不支持 Range 时必须从 0 开始**——否则我们会拿到整个文件
    /// 却当成"从中间开始的片段"拼进去，得到一个**看似成功、内容错乱**的文件。
    /// 那是比"重下"坏得多的结局。
    pub const fn request_from(&self) -> u64 {
        if self.range_supported {
            self.bytes_done
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 第一次重试之前不等待() {
        // 很多失败是瞬时的（连接被复用后失效），立刻重试就成功了。
        // 等一秒再试只是让用户多等一秒。
        //
        // ⚠️ 从第 2 次起的取值在 M1 被更正过（见 `default_for_network` 的文档）：
        // 原来是 500/1000/2000，而规格 §4 写的是**退避基础 2s**。
        let p = RetryPolicy::default_for_network();
        assert_eq!(p.backoff.delay_ms(0), 0);
        assert_eq!(p.backoff.delay_ms(1), 2_000);
        assert_eq!(p.backoff.delay_ms(2), 4_000);
        assert_eq!(p.backoff.delay_ms(3), 8_000);
    }

    #[test]
    fn 退避必须有上限() {
        // 没有上限的话第 20 次重试要等很久，用户看到的是"卡死"。
        let b = Backoff::Exponential {
            first_ms: 500,
            factor: 2,
            cap_ms: 30_000,
        };
        assert_eq!(b.delay_ms(10), 30_000);
        assert_eq!(b.delay_ms(60), 30_000);
        // 极大 attempt 不能溢出成小数（那会变成"立刻重试"，反而更糟）
        assert_eq!(b.delay_ms(u32::MAX), 30_000);
    }

    #[test]
    fn 默认策略的总等待是一分钟且不会更长() {
        // ⚠️ 这条测试原来叫「最坏等待是秒级而不是分钟级」，断言 `total < 5000`。
        // 而规格 §4 给的是**退避基础 2s、最多 5 次重试** ——
        // 也就是总等待**恰好是一分钟**。所以旧名字与旧断言都错了：
        // **一分钟正是规格接受的那个数**，而"秒级"从来不是规格的目标。
        //
        // 这条测试现在的意义是：把"一分钟"这个已决策的数字钉住，
        // 并保证**它不会因为重试次数或退避系数被改而悄悄变长**。
        let p = RetryPolicy::default_for_network();
        let total = p.total_wait_ms();
        assert_eq!(
            total,
            2_000 + 4_000 + 8_000 + 16_000 + 30_000,
            "= 60 秒，且最后一次被 cap 封顶（不封顶会是 32 秒）"
        );
        assert_eq!(total, 60_000);
        assert!(
            total <= 60_000,
            "总等待 {total} ms 已经超过规格接受的一分钟"
        );
    }

    #[test]
    fn 不重试策略的总尝试次数是一() {
        let p = RetryPolicy::none();
        assert_eq!(p.max_attempts, 1, "不重试 = 总尝试次数 1（含第一次）");
        // 第一次还没做，所以有额度；做完了就没有了。
        assert!(p.has_attempt_left(0), "第一次还没做，该做");
        assert!(!p.has_attempt_left(1), "第一次做完就到顶了，不该有第二次");
        assert_eq!(p.total_wait_ms(), 0, "不重试就没有等待");
    }

    #[test]
    fn has_attempt_left_的次数语义() {
        let p = RetryPolicy {
            max_attempts: 3,
            backoff: Backoff::Fixed { ms: 100 },
        };
        // 3 次总尝试：已完成 0 / 1 / 2 时都还有额度，完成 3 次就没了。
        assert!(p.has_attempt_left(0));
        assert!(p.has_attempt_left(1));
        assert!(p.has_attempt_left(2));
        assert!(!p.has_attempt_left(3), "3 次总尝试做完就到顶");
    }

    #[test]
    fn 等待时长与已完成次数对应() {
        // delay_before(n) 是"做第 n+1 次尝试之前"的等待。
        let p = RetryPolicy::default_for_network();
        assert_eq!(p.delay_before(0), 0, "第一次尝试立刻做");
        assert_eq!(p.delay_before(1), 2_000, "第二次（=第一次重试）前等 2 秒");
    }

    #[test]
    fn 取消不是失败() {
        // 把它塞进 Err 会让上层当故障上报，然后用户会被自己按的取消弹一次窗。
        let o: Outcome<u32> = Outcome::Cancelled;
        assert!(o.is_cancelled());
        assert!(!matches!(o, Outcome::Failed(_)));

        // 取消对应到错误码体系里的静默级别（见 qul_core::error）
        let e = crate::error::QulError::new(
            crate::error::ErrorCode::AuthUserCancelled,
            "用户取消了操作",
            "",
        );
        assert_eq!(e.severity, crate::error::Severity::Silent);
        assert!(!e.is_user_visible(), "用户自己按的取消不该再弹一次");
    }

    #[test]
    fn 取消令牌可重复调用且立即可见() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        t.cancel();
        t.cancel(); // 用户可能连点几次，不该出错
        assert!(t.is_cancelled());
    }

    #[test]
    fn 对端不支持_range_时必须从零开始() {
        // 否则我们会拿到整个文件却当成"从中间开始的片段"拼进去，
        // 得到一个**看似成功、内容错乱**的文件 —— 比"重下"坏得多。
        let r = ResumePoint {
            bytes_done: 12345,
            range_supported: false,
        };
        assert_eq!(r.request_from(), 0, "不支持 Range 时必须从头下");

        let r2 = ResumePoint {
            bytes_done: 12345,
            range_supported: true,
        };
        assert_eq!(r2.request_from(), 12345);
        assert_eq!(ResumePoint::start().request_from(), 0);
    }
}
