# MoLaunch 源码调研

> **来源**：`https://github.com/MoTeam-cn/MoLaunch`（克隆于 `repos/MoLaunch`，HEAD `6ae12b6`，2026-08-22）
> **许可**：**「MoLaunch 分发有限许可证」——自定义许可，不是开源许可**。条款见 §1。
> **规模**：60.9 MB / 1801 个文件 / Tauri 2 + Vue 3 + Rust
> **性质**：**只读参照**。本文只记"它怎么做"，不改我们的任何既定决策。

---

## 1. 🔴 许可证（**这一节最先读，它决定能借鉴到什么程度**）

**「MoLaunch 分发有限许可证」**（`LICENSE`，62 行，中文原创条款）。关键条文：

| 条款 | 原文要点 |
|---|---|
| 定义 2 | 「二次开发版本」= **基于本项目修改，或「参考本项目较大部分内容」而制作的作品** |
| 二 | **原样副本**：可下载/存储/备份/运行；**非商业**前提下可传播 |
| **三.1–2** | 二次开发**必须公开完整源代码**（含修改/添加/删除） |
| 三.3 | 发布页必须声明「**本项目是基于 MoLaunch 的第三方二次开发版本，与 MoTeam 官方版本无关**」 |
| **三.4** | 名称**必须以「基于 MoLaunch 的第三方版本」这类表述开头或显著包含** |
| 三.5 | 不得用「MoLaunch 官方版 / 3 / 手机版 / Pro / ++」等**易被误认为官方的名称** |
| 三.6 | **不得移除原有版权、许可证、商标、来源说明** |
| 三.7 | **不得商业使用**（含捐赠/赞助/会员/广告作为收费主体） |
| 四 | 第三方内容**仍按其各自许可**；清单见 `src-tauri/resources/about/licenses.txt` |
| 五 | **不授予任何商标权** |

### 1.1 对我们项目的直接影响（**必须由用户决策**）

> **若我们"参考较大一部分"，三.4 要求发布名改成类似
> 「**基于 MoLaunch 的第三方版本 · 秦墨**」。**
>
> **而"秦墨 / Qinmo Ultimate Launcher"这个名字用户已经定死**：
> GitHub 仓库名、Azure 应用注册（`Qinmo Ultimate Launcher`）、README 全用它。
>
> **这是一个真实的命名冲突。** 三种出路：
> | 出路 | 代价 |
> |---|---|
> | **① 不参考代码，只借鉴"思路与机制"** | 需严守"思路可借鉴、表达不复制"的界线。**机制/算法/架构思想不受版权保护，具体代码受** |
> | ② 接受三.4，改名 | 与已定的仓库名/应用注册名冲突，用户明确说名字就是这个 |
> | ③ 联系 MoTeam 取得书面授权 | 不适用个人项目 |
>
> **建议走 ①**，并在 `docs/来源记录.md` 里**明确登记**"本项目的 X 机制受 MoLaunch 启发"——
> **主动标注来源比"模糊地不提"更安全，也更诚实**。

---

## 2. 它是什么（**规模远超"启动器"**）

**Minecraft Java 版启动器**，但它自带一整套**云端基础设施**（`api-server`，独立仓库）：

| 它有 | 说明 |
|---|---|
| **联机体系** | FRP 内网穿透 + EasyTier 组网 + WebRTC Mesh + 自建信令（websocket）+ NAT 检测 + PoW 挑战 + ECIES 信封 |
| **自建服务端** | axum 服务：登录 / 联机调度 / FRP 调度 / 更新分发；JWT+CSRF+限流+PoW 中间件 |
| **插件系统** | 插件 SDK 沙箱 + 权限判定 + `spawn.rs`(344 行) |
| **AI 对话引擎** | `ai_core/`：OpenAI 兼容、SSE 流式、多轮 tool calling、上下文压缩、会话存储 |
| **模组翻译** | `mod_translation/`：直接改 jar 里的类与 lang 文件（配合 AI） |
| **其他工具** | 种子地图（cubiomes → WASM）、NBT/MCA 读写、结构地图（openlayers）、皮肤披风、整合包导入（PCL/HMCL/MultiMC/CurseForge） |

**→ 这是一个"平台"，不是"启动器"。** 我们的定位是**个人自用的编排器**，两者的目标不同。

---

## 3. ⚠️ 与我们的红线冲突的部分（**明确不借鉴**）

| 它的功能 | 我们的红线（方案 §1.4） | 判定 |
|---|---|---|
| **自建信令 + FRP 隧道 + EasyTier 组网** | **不做自建联机中继** | ❌ **不借鉴**。即便它是"按需拉起的 P2P 隧道"而非常驻中继，**信令与调度仍由自有服务端承担**，落在红线语义内 |
| **自建云端（登录 / 更新分发 / CDN）** | 本项目的定位是**单机自用**，不运营服务 | ❌ 不借鉴 |
| **模组翻译（改 jar 内 class）** | 不修改游戏二进制 | ⚠️ **边界案例**：改 **mod 的 jar** ≠ 改**游戏二进制**，严格说未触红线。但它引入"AI 批量改第三方代码"这一整类风险（误改、签名破坏、分发责任）。**建议不做** |

**纪律**：**它的核心竞争力（联机 + 云端）恰好是我们主动放弃的部分。**
看它**不是为了抄它的产品**，而是为了看**一个同类技术栈的项目怎么把工程组织起来**。

---

## 4. ✅ 可借鉴的部分（按价值排序）

### 4.1 ⭐ 配置的"权威层"模式（**最有价值的一条**）

它**禁止 `set_*` / `get_*` 单字段命令**，改为：

```
get_config()                          → 返回**全量快照**（ProxySnapshot / DownloadSnapshot / MemorySnapshot / CommunitySnapshot ...）
apply_config(patch)                   → 用一个 patch 原子地改
```

配套类型：`snapshot.rs`(204 行) / `patch.rs`(173 行) / `entry.rs`(114 行) / `validate.rs`(71 行) / `secure.rs`(165 行)。

**为什么这比"一堆 getter/setter"好**（我认为这是它最值得学的一处）：

| # | 好处 |
|---|---|
| 1 | **不存在"字段级一致性"问题**——读永远是**一个一致的快照**，不会读到"一半新一半旧" |
| 2 | **patch 可以校验**（`validate.rs`）→ 非法值**在写入前**被拒，而不是写进去之后再发现 |
| 3 | **可以留快照与回滚**（`snapshot.rs` + `apply/flow.rs`） |
| 4 | **表面积小**：新增一个配置项**不需要新增一个命令**，只改类型 |

**对我们的意义**：我们**还没有定配置权威层**（§5.5 只定了实例数据的 `profile.json`）。
**这条可以直接采纳其形态**——尤其"**新增配置项不新增命令**"这一条，
正是方案 §3.3「加能力不用改主程序别的地方」的同一种思路。

### 4.2 ⭐ IPC 三层封装 + 业务组件禁止直接调 IPC

```
views → components → composables → stores → utils/api（三层封装）
```
> 「**业务组件不允许直接依赖 IPC**；IPC 全部经 `utils/api/` 统一封装并由类型声明约束。」

**对我们的意义**：我们的纪律是"**前端零业务逻辑**"，
**它这条是更进一步、且更可执行的版本**：
> **不是"前端不许有逻辑"，而是"前端只许经一个受类型约束的出口说话"。**

**建议采纳措辞**：把它写成我们的第三条 lint 规则（前两条是"无字面颜色值""组件必须声明层级"）。

### 4.3 状态与并发的工程纪律

| 它的做法 | 为什么值得学 |
|---|---|
| `AppState { config: Arc<Mutex<..>>, ... }` + **helper 集中在 `state/`** | 避免 `lock/clone/drop` 三件套**到处重复**（重复就会有人忘记 drop） |
| 「**锁内操作尽量短，clone 后立即 drop**」写进规范 | 这是能写成检查项的具体纪律 |
| 所有命令 `Result<T, String>` | 与我们"错误说明不能丢"一致 |

### 4.4 安全基线（**它做得比我们规格更细，值得补**）

| 它做了 | 我们的现状 |
|---|---|
| **CSP 全局策略**（`tauri.conf.json` 里显式列出 `connect-src` 白名单，`object-src 'none'`、`frame-ancestors 'none'`） | 我们 §5.8 有安全基线，**但没有具体的 CSP 条款** → **应补** |
| **日志脱敏** `logger/sanitize` + `utils/tokens.ts` | 我们 §5.8 有"诊断脱敏" → **它有具体做法** |
| **deeplink 安全校验** | 我们**没有**深链接规格（当前也不需要） |
| 凭据本地加密（`clave`） | 我们定了 Windows 凭据管理器（§5.8）→ **一致** |

### 4.5 冷启动：**独立 splash 窗口**

它的窗口配置里有两个窗口：

```json
{ "label": "splashscreen", "url": "splash.html", "640x200",
  "decorations": false, "transparent": true, "alwaysOnTop": true, "skipTaskbar": true },
{ "label": "main", "1096x592", "decorations": false, "visible": false }
```

**做法**：开屏窗口**先出**（小、透明、置顶、不占任务栏），主窗口 `visible: false` **等准备好了再显示**。

**对我们的意义**：我们的预算里有「**冷启动 ≤700ms**」。
**"先出一个 200ms 能画出来的开屏，主窗口在后台备好再切"** 是达成感知目标的标准手段，
**而且我们已经在 S1 验证了 `decorations: false` + `transparent: true` 可用**。
**建议列入 M4 的启动体验设计。**

### 4.6 它的 Tauri 窗口配置**独立印证了 S1 的结论**

```json
{ "label": "main", "decorations": false, "transparent": false, ... }   ← 主窗口无边框
{ "label": "splashscreen", "decorations": false, "transparent": true } ← 开屏透明
```

**它也在用 `decorations: false`** —— 一个同类同栈项目做了同样选择，
**是"无边框可行"的旁证**（但**不是**材质可用的旁证：它的主窗口 `transparent` 是 `false`，
说明它**不依赖窗口透明**，也就没走我们这条 Mica 路线）。

### 4.7 仓库组织（**1800 文件的规模怎么分**）

```
src/  views / components / composables / stores / plugins / utils(api) / types
src-tauri/src/  commands/  ← 全部 IPC 入口，按域分
                minecraft/ ← 游戏核心
                online/ ai_core/ storage/ state/ config/ logger/ certs/
                http/ deeplink/ sdk/ migrations/ resources/
api-server/     ← 云端单独仓库
```

**与我们方案 §3.5 的对照**：我们分 `qul-core`（内核）/ `qul-infra` / `qul-app` / `qul-provider-*`，
**它分 `commands` / `minecraft` / 各 support 模块**。

| 差异 | 评价 |
|---|---|
| 它**没有"零游戏词汇的内核"** | `minecraft/` 是核心且**充满游戏概念**。**我们的"内核零 MC 词汇"是更严的约束，我们不改** |
| 它的 `commands/` 层**独立于业务** | 与我们的 `src-tauri` 薄壳**一致** ✅ |
| 它有 **`migrations/`**（数据迁移） | 与我们的实例数据版本化（§4.7）**同一件事** ✅ |
| 它有 **`config/` 独立模块 + `state/`** | **我们还没有**，见 §4.1、§4.3 → **应补** |

### 4.8 AI 协作规范（**这份文档存在，但被 gitignore 了**）

蓝图里引用三份文档：
- `DEVELOPMENT_BLUEPRINT.md` —— 架构蓝图（**已读，本文主体**）
- `DEVELOPMENT_GUIDELINES.md` —— 开发规范（存在，21 KB）
- `AI_AGENT_GUIDELINES.md` —— **AI 协作行为约束**（仓库里**不存在**，被排除）

**它的 AI 协作要点**（从蓝图附录可见）：

> - 动手前读蓝图建立整体认知，读规范获得风格与过关要求，读 AI 约束获得行为边界
> - **所有修改必须同步 `CHANGELOG.md`**；提交默认不带 `!c`；**每完成一批同性质修改拆一个 commit**
> - **复用既有组件、函数、Hook、IPC 命令；不重复造轮子**
> - **最小验证：每步修改跑对应 typecheck / lint / clippy / test**

**对照我们的 `SESSION.md` 纪律**：高度重合（一 commit 一单元、失败即停、最小验证）。
**它多出的一条我们没有**：**"复用既有组件，不重复造轮子"** ——
我们目前只有"不新增平行入口"（UI 侧），**没有"代码侧先找再写"的条款**。→ **可补。**

---

## 5. 下载引擎的对照（**这一节最有工程价值**）

它的配置快照里有（`DownloadSnapshot`）：

```rust
mirror_url, source, meta_source, max_speed, max_threads, chunk_count
```

下载源模式（`minecraft/sources/mode.rs`）：

```rust
enum DownloadSourceMode { Official, Mirror, Smart }
// mirror: 只用镜像源，失败直接报错
// official: 只用官方源，失败直接报错
// smart:   官方优先，失败回退 BMCLAPI   ← 默认
```

**镜像实现方式**：`constants.rs` 里是**域名/前缀替换表**（BMCLAPI ↔ Mojang/Maven 各仓库）。
**这与我们定的"URL 前缀改写表"（HMCL 实证）是同一种做法** ✅。

### 5.1 ⚠️ 它**印证**了我们的分歧点

| | PCL2 | MoLaunch | **我们** |
|---|---|---|---|
| 源顺序 | 官方优先，失败切镜像 | `Smart` = 官方优先，失败切镜像 | **先测后定** |
| 分片 | `enableParallelChunks = true` | `chunk_count` | 单文件多段并行 |
| 线程/段数可配 | 63（默认） | `max_threads` + `chunk_count` | 自适应爬坡（初 4 / 上限 64） |

**两个成熟实现都把"官方优先"写死在逻辑里。**
**我们的"先测后定"是少数派**——但**用户的实际体验（镜像更快）与旧项目实测（官方快 8 倍）矛盾**，
说明**"谁快"取决于网络路径，不能写死**。**我们的做法给了这个矛盾的处理空间，不撤回。**

**同时**：`max_threads` + `chunk_count` 都**由用户配置**（而我们是自适应）。
它的选择是"把控制权给用户"，我们的是"自动"。**两种都对**；
我们保留自动（因为目标是"不折腾"），但**可以补一个高级选项**（见 §6 建议）。

### 5.2 它的下载实现规模

`commands/tools/download.rs`(219) · `minecraft/download/`（分片 + downloader）
· `commands/community/install/concurrent/`（`detect/` + `extract/` + `run.rs` 三件套）

**`install/concurrent/` 这个结构值得看**：`detect` 判断、`extract` 解压、`run` 执行，
**把"并发安装"拆成可分别测试的三段**。这比"一个函数做完"更可维护。

---

## 6. 结论：借鉴清单

| # | 借鉴什么 | 落到哪 | 优先级 |
|---|---|---|---|
| **1** | **配置权威层**：`get_config` 全量快照 + `apply_config` patch + 校验 + 可回滚 | **新增**：方案的配置章节（我们还没有） | **高** |
| **2** | **"业务组件不许直接调 IPC"** 写成 lint 规则 | UI 规格 §6.3 门禁 | **高** |
| **3** | **CSP 具体条款**（`connect-src` 白名单、`object-src 'none'`） | 方案 §5.8 安全基线 | **中** |
| **4** | **独立 splash 窗口**（`visible:false` 主窗 + 透明置顶开屏） | M4 启动体验（服务 §7 冷启动预算） | **中** |
| **5** | **`state/` 集中 lock/clone/drop helper** | `qul-infra` 设计 | **中** |
| **6** | **"复用既有组件，不重复造轮子"** 写进纪律 | 任务书纪律 | **低** |
| **7** | **`install/concurrent/` 三段拆分**（detect/extract/run） | 下载引擎规格的落盘部分 | **低** |
| — | **登记来源**：在 `docs/来源记录.md` 记一条"MoLaunch：借鉴机制、未复制代码" | 来源台账 | **必做** |

### 不做（红线或超范围）

- ❌ 联机（信令 + FRP + EasyTier + 自建云端）
- ❌ 自建更新/登录服务
- ⚠️ 模组翻译（改第三方 jar）—— 边界案例，**建议不做**
- ❌ 它的 UI 方向（单列 + 2px 圆角 + `#f0f5ff` 底）：**与我们的令牌体系完全不同，硬套会破坏我们的设计语言**

---

## 7. 一句话

> **它的产品（联机 + 云端）是我们主动放弃的部分；它的工程组织是我们该学的部分。**
> **最值钱的一条是"配置权威层"**——用"全量快照 + 原子 patch"取代"一堆 getter/setter"，
> 让**新增配置项不必新增命令**，这与我们「加功能不必改主程序」的思路是同一种。
