# 上次收工：M3 的第 ⑤ 阶段走到了**资源加载**，而 natives 的两个真 bug 已修好

## 上次做到哪（本轮之后）

- **`qul launch` 出现了**，而它**真的把游戏拉起来了** —— 退出码与日志都拿到了
- 🔴 **游戏越过了 natives 加载，走到了资源加载（贴图）阶段**，然后因为
  **资产没装**而崩（`--with-assets` 默认关）。那就是当前**唯一**的阻塞点
- **两个真 bug 是"真跑一次"才发现的**（见下），而它们的症状都极具误导性

### 🔴 那两个 bug（值得单独记住）

| # | 我写的 | 真跑一次的结论 |
|---|---|---|
| ① | `-Djava.library.path` = `;` 拼起来的**目录列表** | 官方模板要的是**一个单一目录**（它会拼 `${natives_directory}/java`、`/jna`、`/lwjgl`、`/netty`）。一串会让 `Path.of()` 在第一个 `;` 处炸：<br>`InvalidPathException: Illegal char <:> at index 105` @ `NativeLibrariesBootstrap.configureLWJGLLibraryPath:180` |
| ② | natives 直接解到根、**保留 jar 内部路径** | 于是 `windows\x64\org\lwjgl\…` 里躺着 21 个 dll，而**根部只有 1 个**（`jtracy-jni-windows.dll`）。`java.library.path` **不递归** ⇒ 21 个全找不到 |

**修法**：解压时**按 jar 分名空间**（`natives/26.3/<jar 文件名>/…`），
而 `-Djava.library.path` = **那个单一的根**。它同时解决了 `lwjgl.dll` 的
**同名冲突**（实测两个不同 jar 都含它）。

**而 ② 的症状最误导**：它不报"natives 没找到"，而是报 `InvalidPathException`
—— 那看起来像"路径里有非法字符"，而真因是"那个值不该是一串"。

- 测试 **684 项全绿**；`clippy -D warnings` 0；`fmt --check` 通过
- 官方 12 个 natives 文件的架构对照**仍未做**（见下）
- `main` 已推送

### ⏭ 下一步：唯一剩下的一件事

**装资产**（`qul install 26.3 --with-assets` —— 约 5147 个对象），然后
`qul launch` 应当能走到**主菜单**。那一步走通，**M3 的验收点就闭合**。

而随后要处理的：

1. **架构感知的解压**（与官方部署的 12 个文件逐一比对）—— 见下
2. **资产应当是必需的，不是可选的** —— 实测"没有资产 ⇒ 游戏崩在
   `TextureManager`"。所以"资产是可选的"那个判断**被实测推翻了**：
   它能起是因为**崩之前就走到了资源加载**。要重新考虑那个默认值。

### ⚠️ 一条**已知偏差**（承上轮）

**natives 解压没有做架构感知。** 官方部署 12 个文件（**只有 x64**），
我们解出 22 个（x64 **与** arm64）。

- `natives-windows-arm64` 条目的 `rules` 是 `allow[windows/]` —— **`os.arch` 是空的**，
  而**官方也照样下载了那 10 个 arm64 jar**（实测）
- 官方是在**解压**时按宿主架构丢掉的
- **⚠️ 我差点加一条"跳过 natives-*-arm64"的规则来"修"它** ——
  而那条规则会让那 10 个 jar **下载不下来**，官方却下载了它们。
  依据是一个**我没有验证的猜测**（"官方没下"），而验证它只需要**一条目录列举**。
## 上次做到哪（本轮之后）

- **M3 的 `qul install` 真的跑通了** —— 从 `piston-meta.mojang.com` 装了
  `26.3`：**76 个文件 / 129,844,475 字节 / 缺口 0 / 解压 22 个 natives**，
  耗时 145.7 s（download 135.5 s · verify 4.2 s · extract 1.9 s）
- **幂等性已验**：再装一次 → **0 新下 / 0 缺口**
- **离线复核已验**：`--offline` → **0 字节下载，缺口 0**（`NoNetwork` 真的没联网）
- **零临时文件残留**（`.part` / `.download` / `.tmp` 一个都没有）
- **https 零新依赖**：`crates/qul-infra/src/winhttp.rs` 用 Windows 自带的 WinHTTP，
  **`Cargo.lock` 仍是 18 个包**（TLS 由系统栈做，Windows 更新维护它）
- **资产索引按哈希去重**：实测 5147 个逻辑名 → 去重后少下若干个
  （`crates/qul-core/src/assets.rs`，11 项测试）
- 测试 **684 项全绿**；`clippy -D warnings` 0；`fmt --check` 通过；文档 21 份 / 610 标题全绿
- `main` 已推送

### ⚠️ 一条**已知偏差**（记在这里，因为它必须在下一轮被处理）

**natives 解压没有做架构感知。**

| | 官方启动器 | 我们 |
|---|---|---|
| 下载 `natives-windows-arm64.jar` | **10 个（全下了）** | 10 个（一样） |
| 解压出的 DLL | **只有 x64（12 个文件）** | x64 **和** arm64（21 个） |

- `natives-windows-arm64` 条目的 `rules` 是 `allow[windows/]` —— **`os.arch` 是空的**，
  所以 `rules` 判不出来，而**官方也照样下载了那 10 个 jar**（实测）
- 官方是在**解压**时按宿主架构丢掉的
- 我们的行为**功能上是对的**：解出的路径保留了 jar 内部结构
  （`natives/26.3/windows/arm64/…`），而 `-Djava.library.path` 会指向架构对应的子目录，
  于是 x86_64 上那些 arm64 文件**永远不会被加载** —— 是 dead bytes，不是错误
- **⚠️ 而我差点加一条"跳过 natives-*-arm64"的规则来"修"它。**
  那条规则会让**那 10 个 jar 下载不下来**，而官方下载了它们 ——
  也就是说那个"修复"会让我们**偏离官方**，方向还是"少下了东西"。
  依据是一个**我没有验证的猜测**（"官方没下"），而验证它只需要**一条目录列举**。
- **留待**：做架构感知的解压并**实测**，然后与官方部署的 12 个文件逐一比对
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

**M3 的最后一环：`qul launch`。**

零件全了（启动计划组装 · 进程管理 · 五阶段流水线 · 真实网络 · 资产 · 离线身份），
而要闭合 M3 的验收点只差**把第 ⑤ 阶段接起来**：
用 `-Djava.library.path` 指向**架构对应的** natives 子目录，拉起进程，
确认能到主菜单。

顺带处理上面那条**已知偏差**（架构感知的解压），并与官方的 12 个文件逐一比对。
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
