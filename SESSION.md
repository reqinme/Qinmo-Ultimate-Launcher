# 上次收工：2026-10-02 · M1 交付物在可自验范围内全部完成；S7 取得真实命令行基准并被一个具体条件阻断

## 上次做到哪

- **M0 出口条件全部闭合**：11 个尖刺都有结论（**S7 的结论是"被具体条件阻断"**，
  外加证据）；材质路线已定；预算已实测校准；审计已跑；**Mojang 审批已于 2026-10-01 23:59 提交**
- **M1 交付物在"我能自验的范围"内全部完成**（14 项）；只差
  **Tauri 安全基线 + 前端命令层门禁**，而那硬依赖 M4 的前端存在
- **一条命令验证**：`pwsh -File tools/verify.ps1` → **8/8 全绿**
- **测试 524 项全绿**；`clippy -D warnings` 0；`fmt --check` 通过；文档 21 份 / 610 标题全绿
- `main` 已推送，**没有未提交的工作区**

### M1 交付物清单

| 交付物 | 落点 | 状态 |
|---|---|---|
| 错误码体系（码 + 原因 + 建议 + 级别） | `qul-core/src/error.rs` | ✅ |
| 任务队列与进度 | `qul-core/src/tasks.rs` | ✅ |
| 下载引擎（规则 + 传输 + 超时 + 进度存储） | `qul-core/src/download.rs` · `qul-infra/src/download.rs` · `store.rs` | ✅ |
| 校验（SHA-1 流式 + 清单） | `qul-infra/src/check.rs` | ✅ |
| **解压**（ZIP 解析 + inflate + 落盘防护） | `qul-core/src/{zip,inflate}.rs` · `qul-infra/src/zip.rs` | ✅ |
| 脱敏日志 | `qul-core/src/scrub.rs` · `qul-infra/src/logging.rs` | ✅ |
| **i18n 框架** | `qul-core/src/i18n.rs` · `locales/zh-CN.json`（52 键） | ✅ |
| 并发写安全（原子写 + 锁 + 单实例 + 取消/重试） | `qul-infra/src/fsx.rs` · ADR-0013 | ✅ |
| 迁移链（版本化 + 备份 + 失败不阻断） | `qul-core/src/migrate.rs` · `qul-infra/src/instance.rs` | ✅ |
| 崩溃捕获（panic hook + WebView2 检测 + marker + 脱敏） | `qul-core/src/crash.rs` · `qul-infra/src/crash.rs` | ✅ |
| **实例通用模型** | `qul-core/src/instance.rs` | ✅ |
| 引擎与界面解耦（Provider 窄 trait + Mock） | `qul-core/src/provider.rs` · `qul-provider-mock` | ✅ |
| **Tauri 安全基线** | — | ⛔ **依赖 M4** |
| **前端无法绕过命令层** | — | ⛔ **依赖 M4** |

### 三层架构守卫（"内核零 MC 词汇"）

| 层 | 位置 | 它多管什么 |
|---|---|---|
| 1 | `crates/qul-core/tests/architecture.rs` | 跑在 `cargo test` 里；另管依赖方向、能力表、文件名纪律 |
| 2 | `tools/scan-core-vocabulary.ps1` | **标识符边界** + **硬编码主机名** |
| 3 | `tools/_probe-scan.ps1` · `tests/vocabulary_probe.rs` | **反证**：检查器必须能被踩红 |

## 下一步第一件事

**等你定 S7 的走法**（三种见 `spikes/s7-official-baseline/结论.md`）：

| 方案 | 做什么 |
|---|---|
| **A（推荐）** | S7 就地挂起，**先推进 M2 → M3**；等 M2/M3 有了离线身份，回头一次做完 S7 第 5–8 步 |
| **B** | 现在就把最小离线身份补上（专门为 S7 服务，会提前 M3 的一部分） |
| **C** | 用官方启动器的账号令牌做比对 —— ⚠️ 我不建议（借用户真实令牌实验） |

**若选 A**：下一件事是 **M2 第一块**（Java 版元数据：版本清单与详情、跨年代结构差异）。

## 卡在哪

| 卡点 | 性质 |
|---|---|
| **S7 第 5–8 步** | 阻断：game 参数需要账号令牌，而**离线身份属于 M2/M3，尚未实现** |
| **M1 的最后两项** | 阻断：**必须有前端**（M4）才能验 |
| **S10 干净机器分发** | 环境降级：只有这一台电脑，**WebView2 缺失项无法覆盖**（已如实标注） |
| **Mojang 审批** | 等外部：**已提交，无编号、无进度面板** ⇒ 无法主动探测，只有实际登录时才知道 |

---

## 附：S7 本轮取证到的关键事实（勿丢）

| 事实 | 值 |
|---|---|
| 游戏真的进过主菜单 | `.minecraft/logs/latest.log`：`Setting user: qinme` … `Stopping!`（**游戏进程自己写的**） |
| 官方 JVM 参数基准 | **20 条**，落在 `spikes/s7-official-baseline/baseline-args.txt`（含 6829 字符 classpath） |
| classpath 组成 | 74 条 windows 库 + `versions\26.3\26.3.jar` |
| 官方 JVM 选项 | `-Xms2G -Xmx4G -XX:+UseZGC -XX:+AlwaysPreTouch -XX:+UseStringDeduplication -XX:+UseCompactObjectHeaders` |
| 官方 Java 运行时 | **自己的 LocalCache** 里的 `java-runtime-epsilon`（**Java 25**），不是系统 JDK |
| natives 去处 | `.minecraft\bin\<hash>\<hash>\{java,jna,lwjgl,netty}`；**内层哈希 = SHA1(版本id)** |
| **`libraries` 一直是完整的** | JSON 声明 114 → Windows 需要 **74** → 磁盘 **74**。**74 = 74** |
| 游戏参数**不在**日志里 | 正确的安全做法：令牌不该进日志 ⇒ **S7 第 5 步只能比 JVM 参数那一半** |
