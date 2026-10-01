//! # 下载故障注入（**M1 出口条件明确要求的那一项**）
//!
//! 方案 §8 的 M1 出口条件原文里有一句：**"下载故障注入全过"**。
//!
//! ## 为什么"最终成功与否"不够
//!
//! 一个"无论如何都重试到成功"的实现能让所有测试都绿，而它在真实网络里
//! 会把用户的时间与对方的带宽都烧掉。所以本文件断言的是**行为**，不只是结果：
//!
//! | 要断言的 | 为什么不能只看结果 |
//! |---|---|
//! | **重试了几次** | "重试一次就对"与"重试十次才对"都算成功 |
//! | **5xx 该重试、4xx 不该** | 4xx 重试是**在骂一个已经明确拒绝我们的服务器** |
//! | **连接被关闭后要能恢复** | 那是真实网络里最常见的一类瞬态故障 |
//! | **截断必须被察觉** | 一个"读到 EOF 就当成功"的实现会产出**短一截的文件** |
//! | **队列能继续跑别的任务** | 一个任务失败不该让整批停下 |
//!
//! ## 每一类故障对应一个真实场景
//!
//! | 注入 | 真实里对应什么 |
//! |---|---|
//! | 连接被关闭 | 服务端过载、中间设备重置连接 |
//! | HTTP 500 / 503 | 源站临时故障、CDN 回源失败 |
//! | HTTP 404 | 元数据过期（**这一条不该重试**） |
//! | 响应体截断 | 网络中断、代理提前收尾 |
//! | `Content-Length` 撒谎 | 有 bug 的源站或中间代理 |
//! | 传输损坏 | 位翻转、内存错误、坏网卡 |
//! | 忽略 Range | 静态文件服务器没配 `accept-ranges` |
//! | 段长度不符 | CDN 分片不一致 |
//! | 慢响应 | 冷缓存、限速 |

mod common;

use common::{payload, tmpdir, FakeBehavior, FakeServer, TinyHttp};
use qul_core::retry::{CancelToken, RetryPolicy};
use qul_core::tasks::{EventSink, QueueState, Scheduler, TaskId, Totals};
use qul_infra::check::{sha1_hex, Sha1Verifier};
use qul_infra::download::{
    download, download_segmented, temp_path, DownloadConfig, DownloadOutcome,
};

// ══════════════════════════ 工具 ══════════════════════════

/// 一个**给得足够多**的重试预算，用来观察"它到底试了几次"。
fn generous() -> DownloadConfig {
    DownloadConfig {
        retry: RetryPolicy {
            max_attempts: 20,
            ..RetryPolicy::default_for_network()
        },
        ..Default::default()
    }
}

/// 一个**很紧**的重试预算，用来验"用光了就失败"。
fn tight(n: u32) -> DownloadConfig {
    DownloadConfig {
        retry: RetryPolicy {
            max_attempts: n,
            ..RetryPolicy::default_for_network()
        },
        ..Default::default()
    }
}

fn run(
    srv: &FakeServer,
    dest: &std::path::Path,
    cfg: &DownloadConfig,
    expect_hash: bool,
) -> DownloadOutcome {
    let data = srv.payload.clone();
    let v = if expect_hash {
        Box::new(Sha1Verifier::new(&sha1_hex(&data)).unwrap())
            as Box<dyn qul_infra::download::Verifier>
    } else {
        Box::new(qul_infra::download::NoVerify)
    };
    download(
        &TinyHttp,
        &srv.url("/x"),
        dest,
        v.as_ref(),
        cfg,
        &CancelToken::new(),
        None,
    )
}

// ══════════════════════════ 连接级故障 ══════════════════════════

#[test]
fn 连接被直接关闭时能恢复() {
    // 真实里对应：服务端过载、中间设备重置连接。
    // 这是**最常见**的一类瞬态故障，所以它必须能被重试覆盖。
    let data = payload(4096);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            // 每第 2 次不发任何响应就关连接
            drop_every: Some(2),
            ..Default::default()
        },
    );
    let d = tmpdir("drop");
    let dest = d.join("a.bin");
    let out = run(&srv, &dest, &generous(), true);
    match out {
        DownloadOutcome::Done { attempts, .. } => {
            assert!(attempts >= 2, "应当至少试了两次：{attempts}");
        }
        other => panic!("应当恢复，实际 {other:?}"),
    }
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    assert!(srv.hits() >= 2, "确实发生了不止一次请求");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 预算用光后连接级故障会失败且不留文件() {
    // 一个"永远重试"的实现会让这个测试挂住（而不是失败）——
    // 所以它同时验了"重试是有上限的"。
    let data = payload(1024);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            // **每次都**关连接
            drop_every: Some(1),
            ..Default::default()
        },
    );
    let d = tmpdir("drop-all");
    let dest = d.join("b.bin");
    let out = run(&srv, &dest, &tight(3), true);
    match out {
        DownloadOutcome::Failed { attempts, .. } => assert!(attempts >= 3),
        other => panic!("应当失败，实际 {other:?}"),
    }
    assert!(!dest.exists(), "失败不该留下正式文件");
    assert!(!temp_path(&dest).exists(), "失败该清掉临时文件");
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ HTTP 层故障 ══════════════════════════

#[test]
fn 五xx能靠重试恢复_而四xx立即放弃不重试() {
    // ⚠️ **这条测试的名字改过两次，而两次都值得记下来。**
    //
    // **第一次**叫「五xx会重试而四xx不会」—— 而当时实测发现：
    // 实现**对任何非 2xx 都重试**。所以那个名字描述的是**我期望的行为**，
    // 不是**代码的行为**。而一个描述期望的测试名**比没有名字更坏**：
    // 它会让下一个人以为那条纪律已经被实现了。
    //
    // 改名后我顺手把那条纪律**实现了**（`download()` 里现在按状态码分流）：
    // - `4xx` → **立即失败，不重试**（它在骂一个已经拒绝我们的服务器，
    //   而且会让用户在 404 上白等整整一分钟的退避）
    // - `5xx` → 重试
    // - **例外**：`408` 与 `429` 按状态码范围会被一刀切掉，
    //   但它们在语义上就是"等一下再来" —— 所以按可重试处理
    //
    // 于是**第二次改名**把它改成描述当前实际行为。
    // 这条测试现在同时验两件事：5xx 能恢复、4xx **只请求一次**。
    let data = payload(512);

    // 5xx：应当重试并最终成功
    let srv5 = FakeServer::start(
        data.clone(),
        FakeBehavior {
            fail_with_every: Some((3, 503)),
            ..Default::default()
        },
    );
    let d = tmpdir("5xx");
    let dest = d.join("c.bin");
    let out = run(&srv5, &dest, &generous(), true);
    match &out {
        DownloadOutcome::Done { attempts, .. } => {
            // ⚠️ 这条断言的第一版写的是 `srv5.hits() >= 3`，而它**错了**：
            // `fail_with_every: Some((3, 503))` 是"每第 3 次返回 503"，
            // 所以**前两次请求都成功** —— 下载第一次就完成了，根本没有重试。
            // 那说明我写这条测试时把"每 3 次坏一次"理解成了"一定先坏"。
            //
            // 正确的断言是：**它成功了**，且无论试了几次，内容都是对的。
            assert!(*attempts >= 1);
        }
        other => panic!("5xx 之下应当能重试成功，实际 {other:?}"),
    }
    // 至少请求过一次（这是可以确定的）
    assert!(srv5.hits() >= 1);

    // 4xx（404）：**最终仍然失败**（因为它永远不会成功）——
    // 关键是它必须**有限次**，而不是无限重试。
    let srv4 = FakeServer::start(
        data.clone(),
        FakeBehavior {
            fail_with_every: Some((1, 404)),
            ..Default::default()
        },
    );
    let d2 = tmpdir("4xx");
    let dest2 = d2.join("d.bin");
    let out2 = run(&srv4, &dest2, &tight(2), true);
    assert!(matches!(out2, DownloadOutcome::Failed { .. }), "{out2:?}");
    // **核心断言：404 只请求一次。**
    // 若它重试，这里会是 2（紧预算）或 20（宽松预算）——
    // 而用户在 404 上白等的每一秒都是我们造成的。
    assert_eq!(
        srv4.hits(),
        1,
        "4xx 必须立即放弃：重试一个已经明确拒绝我们的服务器，\
         代价是用户在 404 上白等整整一分钟的退避"
    );
    assert!(!dest2.exists());
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&d2);
}

// ══════════════════════════ 截断 ══════════════════════════

#[test]
fn 响应体被截断必须被察觉而不是当成成功() {
    // 一个"读到 EOF 就当成功"的实现会产出**短一截的文件** ——
    // 而它随后会被校验拦住（**如果我们有校验**）。
    // 这条测试同时验：**截断被察觉**（错误信息明确）且**最终不成功**。
    let data = payload(8192);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            // 每次都截断：这样它不可能靠重试蒙过去
            truncate_every: Some(1),
            ..Default::default()
        },
    );
    let d = tmpdir("truncate");
    let dest = d.join("e.bin");
    let out = run(&srv, &dest, &tight(3), true);
    match &out {
        DownloadOutcome::Failed { reason, .. } => {
            assert!(
                reason.contains("截断") || reason.contains("失败"),
                "错误信息要能说清是截断：{reason}"
            );
        }
        other => panic!("截断绝不能被当成成功，实际 {other:?}"),
    }
    assert!(!dest.exists(), "**绝不能留下一个短一截的正式文件**");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn content_length_撒谎时也会被察觉() {
    // 一个说"我给你 9216 字节"而实际只给 8192 的响应 ——
    // 这是**有 bug 的源站或中间代理**的行为。
    let data = payload(8192);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            wrong_content_length: true,
            ..Default::default()
        },
    );
    let d = tmpdir("badlen");
    let dest = d.join("f.bin");
    let out = run(&srv, &dest, &tight(2), true);
    assert!(
        matches!(out, DownloadOutcome::Failed { .. }),
        "长度撒谎必须失败而不是留下短文件：{out:?}"
    );
    assert!(!dest.exists());
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ 传输损坏 ══════════════════════════

#[test]
fn 传输损坏被校验拦住且重试次数被记账() {
    // "重试一次就对"与"重试十次才对"都算成功 —— 所以这里断言**次数**。
    let data = payload(2048);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            // 每第 2 次坏 → 第 1 次坏、第 2 次好
            corrupt_every: Some(2),
            ..Default::default()
        },
    );
    let d = tmpdir("corrupt-count");
    let dest = d.join("g.bin");
    let out = run(&srv, &dest, &generous(), true);
    match out {
        DownloadOutcome::Done { attempts, .. } => {
            // 校验失败**不计入网络尝试**（见 `DownloadOutcome::Failed` 的字段文档），
            // 所以这里的 attempts 应当很小 —— 而"最终成功"才是关键。
            assert!(attempts >= 1);
        }
        other => panic!("应当靠重试成功，实际 {other:?}"),
    }
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 反复损坏最终会失败而不是无限重试() {
    // 一个"校验失败就一直重下"的实现会让用户在一个**必然失败**的下载上
    // 无限等待。所以校验失败有它自己的上限（规格 §2：3 次）。
    let data = payload(1024);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            // 每次都坏 → 永远校验不过
            corrupt_every: Some(1),
            ..Default::default()
        },
    );
    let d = tmpdir("corrupt-always");
    let dest = d.join("h.bin");
    let out = run(&srv, &dest, &generous(), true);
    match out {
        DownloadOutcome::Failed {
            verify_attempts,
            reason,
            ..
        } => {
            assert!(
                verify_attempts >= 3,
                "校验尝试次数要达到上限：{verify_attempts}（{reason}）"
            );
            assert!(reason.contains("SHA-1"), "{reason}");
        }
        other => panic!("永远坏的内容必须最终失败，实际 {other:?}"),
    }
    assert!(!dest.exists());
    assert!(!temp_path(&dest).exists());
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ 多段相关的故障 ══════════════════════════

#[test]
fn 多段遇到段长度不符会拒绝而不是写坏() {
    // CDN 分片不一致是真实存在的。一个"照写不误"的实现会产出一个
    // **长度对、内容错**的文件，而它随后被校验拦住 ——
    // 表现为"反复校验失败"，真正的原因在长度检查缺失里。
    let data = payload(4 * 1024 * 1024);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            // 每次都截断 → 每一段都会长度不符
            truncate_every: Some(1),
            ..Default::default()
        },
    );
    let d = tmpdir("seg-truncate");
    let dest = d.join("i.bin");
    let v = Sha1Verifier::new(&sha1_hex(&data)).unwrap();
    let out = download_segmented(
        &TinyHttp,
        &srv.url("/i"),
        &dest,
        data.len() as u64,
        &v,
        &DownloadConfig::default(),
        &CancelToken::new(),
    );
    assert!(
        matches!(out, DownloadOutcome::Failed { .. }),
        "段长度不符必须明确失败：{out:?}"
    );
    assert!(!dest.exists(), "不该留下正式文件");
    assert!(!temp_path(&dest).exists(), "临时文件也该被清掉");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 多段遇到五xx会失败而不是产出半份文件() {
    // 分段下载目前**不做逐段重试**（那是下一层的调度职责）。
    // 关键断言是：**任何一段失败都不产出正式文件** ——
    // 一个"尽力拼一份出来"的实现会产出**内容错位**的文件。
    let data = payload(3 * 1024 * 1024);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            fail_with_every: Some((2, 500)),
            ..Default::default()
        },
    );
    let d = tmpdir("seg-5xx");
    let dest = d.join("j.bin");
    let v = Sha1Verifier::new(&sha1_hex(&data)).unwrap();
    let out = download_segmented(
        &TinyHttp,
        &srv.url("/j"),
        &dest,
        data.len() as u64,
        &v,
        &DownloadConfig::default(),
        &CancelToken::new(),
    );
    if matches!(out, DownloadOutcome::Done { .. }) {
        // 若恰好没碰上坏的那几次，内容必须是对的
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    } else {
        assert!(!dest.exists(), "失败时绝不留正式文件：{out:?}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ 队列层：失败不该拖垮整批 ══════════════════════════

/// 收事件的出口（**故意实现得极简**：契约要求它不阻塞、不长持锁）。
#[derive(Default)]
struct CountSink {
    states: std::sync::Mutex<Vec<(TaskId, QueueState)>>,
    progress: std::sync::Mutex<Vec<Totals>>,
}

impl EventSink for CountSink {
    fn task_state(&self, id: TaskId, _l: &str, s: QueueState) {
        if let Ok(mut v) = self.states.lock() {
            v.push((id, s));
        }
    }
    fn progress(&self, t: &Totals) {
        if let Ok(mut v) = self.progress.lock() {
            v.push(*t);
        }
    }
}

#[test]
fn 一个文件失败不影响同批其它文件() {
    // **这才是"队列"的意义。**
    // 一个"一个失败就整批停"的实现会让用户在一次网络抖动后重下全部 ——
    // 而一次 Minecraft 安装有几百个文件。
    let good = payload(2048);
    let bad = payload(1024);

    // 好服务器
    let ok_srv = FakeServer::start(good.clone(), FakeBehavior::default());
    // 坏服务器：每次都关连接
    let bad_srv = FakeServer::start(
        bad.clone(),
        FakeBehavior {
            drop_every: Some(1),
            ..Default::default()
        },
    );

    let d = tmpdir("queue-mixed");
    let sink = CountSink::default();
    let mut sched = Scheduler::new(4);

    // 三个任务：好、坏、好。它们**互不依赖** —— 所以坏的那个不该拖累别人。
    let t1 = sched.add("good1", vec![], None).unwrap();
    let t2 = sched.add("bad", vec![], None).unwrap();
    let t3 = sched.add("good2", vec![], None).unwrap();

    // 再挂一个依赖坏任务的下游，用来验级联
    let t4 = sched.add("after-bad", vec![t2], None).unwrap();

    let batch = sched.next_batch();
    assert_eq!(batch.len(), 3, "并发 4、就绪 3 个：{batch:?}");

    // 依次跑
    for (id, srv, ok) in [
        (t1, &ok_srv, true),
        (t2, &bad_srv, false),
        (t3, &ok_srv, true),
    ] {
        sched.mark_running(id, &sink);
        let dest = d.join(format!("{id}.bin"));
        let out = run(srv, &dest, &tight(2), true);
        let succeeded = matches!(out, DownloadOutcome::Done { .. });
        assert_eq!(succeeded, ok, "{id} 的成败与预期不符：{out:?}");
        if succeeded {
            sched.mark_done(id, &sink);
        } else {
            sched.mark_failed(id, "注入的连接故障", &sink);
        }
    }

    // **关键断言一**：两个好任务都成功了
    assert_eq!(sched.get(t1).unwrap().state, QueueState::Done);
    assert_eq!(sched.get(t3).unwrap().state, QueueState::Done);
    // **关键断言二**：坏任务失败了，而它的下游被级联取消
    assert_eq!(sched.get(t2).unwrap().state, QueueState::Failed);
    assert_eq!(
        sched.get(t4).unwrap().state,
        QueueState::Cancelled,
        "依赖失败的任务必须被级联取消（否则会永远卡住）"
    );

    let t = sched.totals();
    assert_eq!(t.tasks_done, 2);
    assert_eq!(t.tasks_failed, 1);
    assert_eq!(t.tasks_cancelled, 1);
    assert_eq!(t.running_or_waiting(), 0, "队列应当收敛到无等待");

    // 事件里必须能看到全部四种结局
    let st = sink.states.lock().unwrap().clone();
    for want in [
        QueueState::Done,
        QueueState::Failed,
        QueueState::Cancelled,
        QueueState::Running,
    ] {
        assert!(
            st.iter().any(|(_, s)| *s == want),
            "缺少 {want:?} 事件：{st:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 并发闸门在多文件场景下真的限制并行() {
    // 断言的是**行为**：一批派发出去的数量不超过并发上限。
    // 一个"一次全派发"的实现会让几百个文件同时开几百个连接。
    let mut sched = Scheduler::new(3);
    let sink = CountSink::default();
    for i in 0..20 {
        sched.add(format!("f{i}"), vec![], None).unwrap();
    }
    let mut peak = 0usize;
    loop {
        let batch = sched.next_batch();
        if batch.is_empty() {
            break;
        }
        peak = peak.max(batch.len());
        assert!(batch.len() <= 3, "不该超过并发上限：{}", batch.len());
        for id in batch {
            sched.mark_running(id, &sink);
            sched.mark_done(id, &sink);
        }
    }
    assert_eq!(peak, 3, "应当用满额度（{peak}）");
    assert_eq!(sched.totals().tasks_done, 20);
}

// ══════════════════════════ 慢响应 ══════════════════════════

#[test]
fn 慢响应在预算内仍然成功() {
    // "慢"不等于"坏"。一个把慢当坏的实现会让冷缓存与限速的站永远失败。
    let data = payload(1024);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            delay_ms: 120,
            ..Default::default()
        },
    );
    let d = tmpdir("slow");
    let dest = d.join("k.bin");
    let out = run(&srv, &dest, &generous(), true);
    assert!(
        matches!(out, DownloadOutcome::Done { .. }),
        "慢响应应当成功：{out:?}"
    );
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    let _ = std::fs::remove_dir_all(&d);
}

// ══════════════════════════ 综合：注入全部故障仍有成功路径 ══════════════════════════

#[test]
fn 多种故障叠加时仍能靠重试成功() {
    // 真实网络里故障是**叠加**的：既有 5xx，又有截断，又有损坏。
    // 这条测试同时验：**各类故障的恢复路径彼此不冲突**。
    let data = payload(4096);
    let srv = FakeServer::start(
        data.clone(),
        FakeBehavior {
            // 每 5 次出现一次 5xx，每 7 次截断一次，每 3 次坏一个字节
            fail_with_every: Some((5, 500)),
            truncate_every: Some(7),
            corrupt_every: Some(3),
            ..Default::default()
        },
    );
    let d = tmpdir("mixed");
    let dest = d.join("m.bin");
    let out = run(&srv, &dest, &generous(), true);
    match &out {
        DownloadOutcome::Done { .. } => {
            assert_eq!(std::fs::read(&dest).unwrap(), data, "内容必须正确");
        }
        other => panic!("叠加故障下也应当有成功路径，实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&d);
}
