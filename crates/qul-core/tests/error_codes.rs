//! # 错误码体系的穷举测试（M1 · 方案 §5.7）
//!
//! ## 这些断言在防什么
//!
//! 方案 §5.7 说"级别是**错误码的属性**，不是调用点的临场判断"。
//! **一句话不能强制任何事**；能被强制的只有断言。所以本文件把那条纪律变成：
//!
//! | 纪律 | 断言 |
//! |---|---|
//! | 同一级别共享同一容器 | 每个码的 `presentation` 必须由 `severity` 唯一决定 |
//! | 级别是码的属性 | **改变一个码的级别只能改 `severity()` 一处**；本文件断言每个码恰好一个级别且无重号 |
//! | 只有原因没有建议 = 把问题丢回用户 | **非 Silent / Notice 级别必须有非空建议** |
//! | 编号是稳定标识 | **无重号**、格式统一、兜底码在 |
//!
//! ## 为什么"无重号"值得单独测
//!
//! 错误码会出现在**诊断包、日志、用户报错截图**里。
//! 两个码共用一个编号 → 拿到编号的人**无法判断是哪个故障**，
//! 而这个错误只有在**事后排查**时才会暴露。

use qul_core::error::{ErrorCode, QulError, Severity};

#[test]
fn 每个码都有唯一编号且格式统一() {
    let mut seen = std::collections::BTreeMap::new();
    for code in ErrorCode::ALL {
        let id = code.as_str();
        // 格式 QUL-<领域>-<4 位>
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 3, "{id} 格式应为 QUL-<领域>-<4 位>");
        assert_eq!(parts[0], "QUL", "{id} 前缀应为 QUL");
        assert!(
            (2..=6).contains(&parts[1].len()) && parts[1].chars().all(|c| c.is_ascii_uppercase()),
            "{id} 的领域段应为 2-6 个大写字母"
        );
        assert_eq!(parts[2].len(), 4, "{id} 的序号段应为 4 位");
        assert!(
            parts[2].chars().all(|c| c.is_ascii_digit()),
            "{id} 的序号段应为数字"
        );

        if let Some(prev) = seen.insert(id.to_string(), format!("{code:?}")) {
            panic!("编号 {id} 同时属于 {prev} 与 {code:?} —— 重号会让诊断包无法定位故障");
        }
    }
}

#[test]
fn 兜底码必须存在且编号固定() {
    // 未登记的错误必须有一个确定的去处，否则调用点会各自造码。
    assert_eq!(ErrorCode::Unregistered.as_str(), "QUL-GEN-0000");
    assert!(ErrorCode::ALL.contains(&ErrorCode::Unregistered));
}

#[test]
fn 呈现容器由级别唯一决定() {
    // 纪律 1：同一级别共享同一容器。若某个码能拿到与同级别不同的容器，
    // 说明容器不再由级别决定 —— 那条纪律就破了。
    for code in ErrorCode::ALL {
        let sev = code.severity();
        let e = QulError::new(*code, "测试原因", "测试建议");
        assert_eq!(
            e.presentation,
            sev.presentation(),
            "{code:?}（{sev:?}）的容器与它的级别不一致"
        );
        assert_eq!(e.severity, sev, "{code:?} 的级别必须来自码本身");
        assert_eq!(e.id, code.as_str(), "{code:?} 的编号必须来自码本身");
    }
}

#[test]
fn 五档级别都有码用到() {
    // 某档一个码都没有，说明它要么是多余的，要么是有人忘了登记。
    // 两种都该在这里被看见。
    for sev in [
        Severity::Silent,
        Severity::Notice,
        Severity::Panel,
        Severity::Blocked,
        Severity::CrashReport,
    ] {
        let n = ErrorCode::ALL
            .iter()
            .filter(|c| c.severity() == sev)
            .count();
        if sev == Severity::CrashReport {
            // 崩溃报告不由业务错误码触发（它是 panic hook 的产物），
            // 所以它没有业务码是**预期的**，但容器必须仍然有定义。
            assert!(!sev.presentation().is_empty());
            continue;
        }
        assert!(n > 0, "{sev:?} 这一档没有任何错误码");
    }
}

#[test]
fn 需要用户动作的级别必须有建议() {
    // "只有原因没有建议，等于把问题丢回给用户"。
    // Silent / Notice 的语义是"用户无需动作"，所以它们豁免。
    for code in ErrorCode::ALL {
        let sev = code.severity();
        if matches!(sev, Severity::Silent | Severity::Notice) {
            continue;
        }
        // 这里断言的是"级别允许有建议"，而具体文案由构造点负责。
        // 用一个空建议构造，确认它不会因为级别而被悄悄接受：
        let e = QulError::new(*code, "原因", "");
        assert!(
            !e.suggestion.is_empty() || !e.is_user_visible(),
            "{code:?}（{sev:?}）是用户可见且需处理的，建议不该为空"
        );
    }
}

#[test]
fn 静默级别不打扰用户() {
    // 纪律 2 的落点：已自动恢复的错误不该弹窗，哪怕它技术上很严重。
    let e = QulError::new(ErrorCode::AuthUserCancelled, "用户取消了登录", "");
    assert_eq!(e.severity, Severity::Silent);
    assert!(!e.is_user_visible(), "静默级别不该在界面上出现");

    let e2 = QulError::new(ErrorCode::IoDiskFull, "磁盘空间不足", "请清理空间后重试");
    assert!(e2.is_user_visible(), "阻断级别必须让用户看到");
}

#[test]
fn 可恢复集合是保守的() {
    // "可恢复"意味着**我们会在不打扰用户的前提下重试**。
    // 把它扩大是危险的：不可恢复的错误被重试，用户看到的是"卡住"。
    // 所以这条断言防的是"有人顺手把新码加进可恢复名单"。
    let recoverable: Vec<String> = ErrorCode::ALL
        .iter()
        .filter(|c| c.recoverable())
        .map(|c| format!("{c:?}"))
        .collect();
    assert!(
        recoverable.len() <= 8,
        "可恢复集合膨胀到 {} 项，请逐项确认它们真的可自动恢复：{recoverable:?}",
        recoverable.len()
    );
    // 明确不可恢复的两类：磁盘满、配置解析失败 —— 重试没有意义。
    assert!(!ErrorCode::IoDiskFull.recoverable());
    assert!(!ErrorCode::CfgParseFailed.recoverable());
}

#[test]
fn 显示文本必须带编号() {
    // 用户报错时通常只报一句话；编号是唯一能定位的东西。
    let e = QulError::new(ErrorCode::JavaNotFound, "未找到 Java", "请安装 Java 21");
    let s = format!("{e}");
    assert!(s.contains("QUL-JAVA-0001"), "显示文本必须含编号：{s}");
    assert!(s.contains("未找到 Java"), "{s}");
    assert!(s.contains("请安装 Java 21"), "建议必须在显示文本里：{s}");
}

#[test]
fn 序列化给界面的字段齐全() {
    // 界面拿到的是已经算好的结论（方案 §3.3 的"界面只渲染"），
    // 所以码、级别、容器、原因、建议、操作都必须在 JSON 里。
    let e = QulError::new(
        ErrorCode::DlChecksumMismatch,
        "校验不通过",
        "将自动重新下载",
    )
    .with_action("立即重试", "download.retry")
    .with_action("查看日志", "open.logs");
    let v = serde_json::to_value(&e).expect("必须能序列化");
    for key in [
        "code",
        "id",
        "severity",
        "presentation",
        "reason",
        "suggestion",
        "actions",
    ] {
        assert!(v.get(key).is_some(), "JSON 缺少字段 {key}");
    }
    assert_eq!(v["id"], "QUL-DL-0002");
    assert_eq!(v["presentation"], "island-expanded");
    assert_eq!(v["actions"].as_array().unwrap().len(), 2);
}
