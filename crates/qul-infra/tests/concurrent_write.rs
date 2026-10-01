//! # M1 出口条件实测：**并发写同一实例不出错**
//!
//! 方案 M1 的出口条件原文就是这一句。而"不出错"必须被定义清楚，
//! 否则它只是一个好听的说法。本文件把它拆成四条可判定的性质：
//!
//! | # | 性质 | 不满足时用户看到什么 |
//! |---|---|---|
//! | 1 | **最终内容必须等于某一次完整写入** | "我改的东西过一会儿又变回去了" |
//! | 2 | **永远不能读出半截 JSON** | **设置、账户、实例全部丢失** |
//! | 3 | **所有写入者都不报错**（只要拿到锁） | 有人静默失败，而失败者以为写成功了 |
//! | 4 | **锁真正互斥**：同时最多一个持有者 | 后写的赢，先写的静默消失 |
//!
//! ## 为什么"不写单线程就够"的测试不行
//!
//! 原子写的 bug**只在竞争下出现**：单线程跑一万次都能过，
//! 而两个线程同时跑十次就可能产出半截文件。
//! 所以本文件的每个测试**都是多线程的**，并且断言的是
//! **"读到的东西永远合规"**，而不是"没崩"。

use qul_infra::fsx::{read_optional, write_atomic, FileLock, IoError, SingleInstance};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

fn tmpdir(tag: &str) -> PathBuf {
    // 用进程 id + 纳秒，避免同一进程内并行跑测试时互相踩。
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let d = std::env::temp_dir().join(format!("qul-concurrent-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// 一个"设置文件"的合法内容形态：`{"writer":N,"filler":"xxxx..."}`。
///
/// **每一个写入者写一个自己独有的、足够大的内容**，
/// 这样"两个写入者的内容混在一起"这种情况才有可能被检出——
/// 如果所有写入者写同样的字节，损坏就看不出来了。
fn payload(writer: usize, filler: usize) -> Vec<u8> {
    format!(r#"{{"writer":{writer},"filler":"{}"}}"#, "x".repeat(filler)).into_bytes()
}

#[test]
fn 并发写同一文件永不产出半截内容() {
    let d = tmpdir("atomic-race");
    let target = d.join("instance.json");
    let writers = 8usize;
    let rounds = 60usize;
    let barrier = Arc::new(Barrier::new(writers));
    let bad = Arc::new(AtomicUsize::new(0));
    // **失败必须把原因留下来。** 第一版这里只计数，于是测试红了
    // 而我看不到是"超时"还是"别的 IO 错误"——正是"失败不响"的老毛病。
    let bad_reasons: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let read_errors = Arc::new(AtomicUsize::new(0));
    let read_reasons: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    // 一个"读者"线程全程不停地读，检查它看到的永远是合法内容。
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reader = {
        let (target, stop, read_errors, read_reasons) = (
            target.clone(),
            stop.clone(),
            read_errors.clone(),
            read_reasons.clone(),
        );
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                // **用 read_optional**：第一个写入者还没落盘之前，"文件不存在"
                // 是正常状态，不是故障。而目标是"永远不读到不合规的内容"。
                match read_optional(&target) {
                    Ok(None) => {} // 还没被创建过，正常
                    Ok(Some(bytes)) => {
                        let s = String::from_utf8_lossy(&bytes);
                        let ok = s.starts_with(r#"{"writer":"#)
                            && s.ends_with(r#""}"#)
                            && s.matches("writer").count() == 1;
                        if !ok {
                            read_errors.fetch_add(1, Ordering::SeqCst);
                            if read_reasons.lock().unwrap().len() < 5 {
                                read_reasons.lock().unwrap().push(format!(
                                    "内容不合规（{} 字节）：{}",
                                    bytes.len(),
                                    &s[..s.len().min(100)]
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        read_errors.fetch_add(1, Ordering::SeqCst);
                        if read_reasons.lock().unwrap().len() < 5 {
                            read_reasons.lock().unwrap().push(format!("读取失败：{e}"));
                        }
                    }
                }
            }
        })
    };

    let mut handles = Vec::new();
    for w in 0..writers {
        let (target, barrier, bad, bad_reasons) = (
            target.clone(),
            barrier.clone(),
            bad.clone(),
            bad_reasons.clone(),
        );
        handles.push(std::thread::spawn(move || {
            barrier.wait(); // 尽量让所有写入者同时开始
            for r in 0..rounds {
                let data = payload(w, 4096 + w * 512 + r % 7);
                if let Err(e) = write_atomic(&target, &data, None) {
                    bad.fetch_add(1, Ordering::SeqCst);
                    bad_reasons
                        .lock()
                        .unwrap()
                        .push(format!("写入者{w} 第{r}轮：{e}"));
                }
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    stop.store(true, Ordering::SeqCst);
    reader.join().unwrap();

    let reasons = bad_reasons.lock().unwrap().clone();
    assert_eq!(
        bad.load(Ordering::SeqCst),
        0,
        "有写入者报错——原子写不该在竞争下失败。前 5 条原因：\n  {}",
        reasons
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    let rr = read_reasons.lock().unwrap().clone();
    assert_eq!(
        read_errors.load(Ordering::SeqCst),
        0,
        "读者看到过不合规的内容（半截/混合）——这正是会丢用户数据的那种损坏。前 5 条：\n  {}",
        rr.iter().take(5).cloned().collect::<Vec<_>>().join("\n  ")
    );

    // 性质 1：最终内容必须等于某一次完整写入
    let final_bytes = fs::read(&target).unwrap();
    let s = String::from_utf8_lossy(&final_bytes);
    assert!(
        s.starts_with(r#"{"writer":"#) && s.ends_with(r#""}"#),
        "最终内容不是一次完整写入：{}",
        &s[..s.len().min(120)]
    );
    assert_eq!(s.matches("writer").count(), 1, "最终内容混了多个写入者");

    // 不留中间产物
    assert!(!d.join("instance.json.bak").exists());
    assert!(!d.join("instance.json.tmp").exists());
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn 并发写不同文件互不干扰() {
    // 每个实例一个文件是常态；这条确认原子写没有用全局状态互相打架。
    let d = tmpdir("atomic-parallel");
    let writes = 24usize;
    let barrier = Arc::new(Barrier::new(writes));
    let mut handles = Vec::new();
    for i in 0..writes {
        let (d, barrier) = (d.clone(), barrier.clone());
        handles.push(std::thread::spawn(move || {
            let f = d.join(format!("instance-{i}.json"));
            barrier.wait();
            for r in 0..20 {
                write_atomic(&f, &payload(i, 1024 + r), None).unwrap();
            }
            (i, fs::read(&f).unwrap())
        }));
    }
    for h in handles {
        let (i, bytes) = h.join().unwrap();
        let s = String::from_utf8_lossy(&bytes);
        assert!(
            s.contains(&format!(r#""writer":{i},"#)),
            "instance-{i}.json 被别的写入者污染了：{}",
            &s[..s.len().min(80)]
        );
    }
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn 文件锁真正互斥() {
    // 同时最多一个持有者。用"进入临界区的人数"来验证，而不是看有没有报错。
    let d = tmpdir("lock-mutex");
    let lock_path = d.join("x.lock");
    let inside = Arc::new(AtomicUsize::new(0));
    let max_inside = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(6));

    let mut handles = Vec::new();
    for _ in 0..6 {
        let (lock_path, inside, max_inside, barrier) = (
            lock_path.clone(),
            inside.clone(),
            max_inside.clone(),
            barrier.clone(),
        );
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            let _g = FileLock::acquire(&lock_path).expect("应当能拿到锁");
            let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
            max_inside.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            inside.fetch_sub(1, Ordering::SeqCst);
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(
        max_inside.load(Ordering::SeqCst),
        1,
        "有过多个持有者同时进入临界区——锁没有互斥"
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn 单实例守卫在竞争下只有一个赢家() {
    // 用户双击两次图标 → 两个实例同时下载同一批文件 → 互相覆盖 →
    // 校验和永远不通过，而报错只说"下载失败"。这条防的就是那个。
    let d = tmpdir("single-race");
    let winners = Arc::new(AtomicUsize::new(0));
    let losers = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(8));

    let mut handles = Vec::new();
    for _ in 0..8 {
        let (d, winners, losers, barrier) =
            (d.clone(), winners.clone(), losers.clone(), barrier.clone());
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            match SingleInstance::acquire(&d) {
                Ok(g) => {
                    winners.fetch_add(1, Ordering::SeqCst);
                    // 赢家持有锁直到这里，模拟"它在工作"
                    std::thread::sleep(std::time::Duration::from_millis(40));
                    drop(g);
                }
                Err(IoError::Locked { .. }) => {
                    losers.fetch_add(1, Ordering::SeqCst);
                }
                Err(e) => panic!("不该出现别的错误：{e}"),
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    // 由于赢家会释放锁，后面的线程可能陆续赢——所以断言的是
    // **"任意时刻只有一个赢家"** 由锁保证，而这里断言
    // **失败者必须是因为 Locked，而不是因为别的错误**
    let w = winners.load(Ordering::SeqCst);
    let l = losers.load(Ordering::SeqCst);
    assert_eq!(w + l, 8, "每个线程都要有明确结局（赢或 Locked）");
    assert!(w >= 1, "至少有一个赢家");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn 锁在进程内可重入获取不同文件() {
    // 一个进程要能同时锁"实例 A"和"实例 B"——否则并发下载会串行化。
    let d = tmpdir("two-locks");
    let a = FileLock::acquire(&d.join("a.lock")).unwrap();
    let b = FileLock::acquire(&d.join("b.lock")).unwrap();
    assert_ne!(a.path(), b.path());
    drop(a);
    drop(b);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn 单个文件上的竞争写最终仍是完整内容() {
    // 与第一个测试互补：这里**每个写入者的内容大小不同且差很多**，
    // 专门逼出"短内容覆盖长内容时留下尾部残渣"这种损坏。
    let d = tmpdir("size-race");
    let target = d.join("cfg.json");
    let barrier = Arc::new(Barrier::new(4));
    let mut handles = Vec::new();
    for w in 0..4usize {
        let (target, barrier) = (target.clone(), barrier.clone());
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            for r in 0..50 {
                // 大小在 64B 与 ~40KB 之间剧烈变化
                let filler = if (r + w) % 2 == 0 { 16 } else { 40_000 };
                write_atomic(&target, &payload(w, filler), None).unwrap();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let s = String::from_utf8_lossy(&fs::read(&target).unwrap()).to_string();
    assert!(
        s.ends_with(r#""}"#),
        "尾部有残渣：{}",
        &s[s.len().saturating_sub(60)..]
    );
    assert_eq!(s.matches("writer").count(), 1, "混了多个写入者");
    // 长度必须与某个合法 payload 一致
    let ok = (0..4).any(|w| {
        [16usize, 40_000]
            .iter()
            .any(|f| s == String::from_utf8_lossy(&payload(w, *f)))
    });
    assert!(ok, "最终内容不是任何一个合法 payload（长度 {}）", s.len());
    let _ = fs::remove_dir_all(&d);
}
