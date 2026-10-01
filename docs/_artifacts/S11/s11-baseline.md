# S11 · 官方启动器能力基线（M9 验收依据）

> **执行**：2026-10-01 · M0 尖刺 S11 · 环境：本机（官方启动器已安装、已用微软账户登录、正在下载 26.3）
> **取证方式**：**官方启动器自己写下的文件与日志**（`%APPDATA%\.minecraft\`）
> **产物**：`s11-baseline.json`（原始证据摘录）
>
> ## ⚠️ 先说清本文件的证据等级（**不许当成"截图取证"**）
>
> 任务书要求"逐项操作并截图"。**截图只能由人看屏幕获得**，本轮无法产生。
> 所以本文件用的是**另一类证据**：官方启动器的**配置、状态、日志与内部接口方法名**。
>
> | 证据等级 | 来源 | 能证明什么 | 不能证明什么 |
> |---|---|---|---|
> | **A · 配置/状态文件** | 启动器自己写出的 JSON | 能力**存在且被执行过**，含确切字段与取值 | 界面上**在哪点** |
> | **A · 日志中的接口调用** | `launcher_log0.txt`（2099 行，316 KB） | 启动器**真的调用了**哪些服务与方法 | 调用结果的界面呈现 |
> | **B · 界面事件名** | 日志中的 `coreEvent.*` | 界面**能触发**哪些能力 | 呈现在哪个页面 |
> | **C · 界面截图** | 需人工操作 | **在哪做**、几步、长什么样 | — |
>
> **所以本基线在"能做什么/不能做什么"上是 A 级证据**（比截图更精确：截图看不出"调了哪个接口"），
> **在"在哪做/几步"上是空的**，标为 **待人工补**。
> **把这两件事混起来的基线，会让 M9 验收变成一个无法执行的承诺。**

---

## 1. 八项基线表

| # | 能力项 | 官方启动器**能**做什么 | **不能**做什么 | 证据 | 界面位置 |
|---|---|---|---|---|---|
| 1 | **发现已安装的 Java / 基岩版** | 用 **`launcher_product_state.json`** 分别记录两类产品的更新检查时间：**`java-retail-Piston`** 与 **`bedrock-retail-Uwp`** | — | **A**（配置） | 待人工补 |
| 2 | **安装 Java 版并启动** | `launcher_profiles.json` 里以两条 profile 承载 `latest-release` 与 `latest-snapshot`；`enableReleases` / `enableSnapshots` / `enableHistorical` / `enableAdvanced` 四个开关控制**展示哪些版本** | — | **A**（配置） | 待人工补 |
| 3 | **基岩版启动** | 同一套 profile 机制与同一个启动器承载基岩版（`bedrock-retail-Uwp`）；**额外**调用 `gamecoreLauncherInstallState` 查询 **GameCore 安装状态** | — | **A**（日志） | 待人工补 |
| 4 | **直接进服务器 / 直接进世界** | **支持**。存在 **`quickPlay` 机制**：配置开关 `quickPlayEnabled`，目录 `.minecraft\quickPlay\java\`（**按产品分子目录**），界面事件 **`coreEvent.quickPlayData`** | — | **A**（配置 + 目录 + 事件） | 待人工补 |
| 5 | **内容管理（资源包 / 行为包）** | ⚠️ **基岩版有 `addon` 概念**（日志 3 处命中） | **Java 版没有资源包/行为包/整合包管理** —— `resourcePack` / `behaviorPack` / `modpack` 在 2099 行日志里 **0 命中** | **A**（日志） | 待人工补 |
| 6 | **世界管理** | ⚠️ **只有 Realms 世界**。接口方法 **`getRealmsWorlds`** / `getRealmsInvites` / `getRealmsTrialEligibility` / `getRealmsActivePlayerCounts`，服务 `java.frontendlegacy.realms.minecraft-services.net`，事件 `coreEvent.realmsData` ×21 | **没有任何本地世界管理** —— `localWorld` / `saves` / `backup` / `export` / `import` **全部 0 命中** | **A**（日志） | 待人工补 |
| 7 | **账户管理** | **只有微软账户**。`loginWithXbox` / `getXboxXBLXToken` / `getMCToken` / `getAccountData`；凭据落在 `launcher_msa_credentials_microsoft_store.bin`；**账户文件缺失时只警告不阻断**（`Unable to find accounts file`） | **没有离线账户**（无任何离线相关字段或接口） | **A**（日志 + 配置） | 待人工补 |
| 8 | **皮肤** | **能查看**。接口方法 **`getActiveSkin`** / **`getCapes`**，界面事件 **`coreEvent.skins`** | **没有更换皮肤的能力**（无 `setSkin` / `uploadSkin` 一类方法） | **A**（日志） | 待人工补 |

---

## 2. 三条**超出任务书八项**的发现（都有依据）

### 2.1 官方启动器**没有任何本地世界管理**

这一条对产品定位有直接意义。

| 查了什么 | 结果 |
|---|---|
| `localWorld` / `LocalWorld` / `saves` / `Saves` | **0 命中** |
| `backup` / `Backup` | **0 命中** |
| `export` / `Export` / `import` / `Import` | **0 命中** |
| 世界相关接口 | **只有** `getRealmsWorlds` 等 Realms 系列 |

**也就是说：任务书 S11 第 6 项"世界管理——列出/备份/导入/导出是否支持"，
官方启动器的答案是「不支持本地世界，只支持 Realms」。**

**这对我们意味着两件事**：

1. **我们的"世界级备份"（方案里作为基岩版隔离失败时的替代路径）不是"追平"，而是"超出"。**
   所以它的验收标准**不能写成"与官方一致"**——官方根本没有这个能力，
   写"一致"会让这一项永远无法判定。
2. **它同时解释了为什么 PCL / HMCL 这类第三方启动器都把世界管理做成核心功能**：
   那不是"抄来的功能"，是**官方留出的空位**。

### 2.2 官方启动器的服务面（它依赖哪些外部服务）

| 服务 | 次数 | 用途 |
|---|---|---|
| `launchercontent.mojang.com` | **423** | 启动器内容（新闻/更新说明/界面素材） |
| `redstone-launcher.mojang.com` | **308** | **启动器自身更新与产品启动配置**（`/release/v2/products/launch...`） |
| `api.minecraftservices.com` | 41 | 账户、授权、权益 |
| `launchermeta.mojang.com` | 15 | 启动器元数据 |
| `java.frontendlegacy.realms.minecraft-services.net` | 12 | Java 版 Realms |
| `authorization.franchise.minecraft-services.net` | 9 | 授权 |
| `piston-meta.mojang.com` | 8 | **版本清单（我们已实测的那个源）** |
| `exp.franchise.minecraft-services.net` | 8 | 实验性服务 |
| `payments.realms.minecraft-services.net` | 3 | Realms 支付 |
| `client.discovery.minecraft-services.net` | 3 | 服务发现 |
| `bedrock.frontendlegacy.realms.minecraft-services.net` | 3 | 基岩版 Realms |

**注意 `redstone-launcher.mojang.com`**：这是**我们此前没有记录过的服务**。
官方启动器用它做**自身更新**与**产品启动配置**（`/release/v2/products/launch`）。
我们的更新机制（方案 §4.8）**不打算用它的通道**，但**它解释了官方启动器为什么能静默更新**。

### 2.3 界面能力面：22 个 `coreEvent.*` 事件名

这是官方启动器**界面能触发的全部能力**（从日志里穷举）：

```
configurationData  connectionStatus  constants  credits  downloads
gameInstances(63)  gameVersions(2)  latentTelemetryData  launcherSettings
modelInfo  msiPatch  preferencesData  productInfo(78)  quickPlayData(2)
realmsData(21)  skins  systemInfo  treatmentTags  users  versions  volumes
```

**用法说明**：这张表是**"官方启动器界面有多少块内容"的答案**。
括号里是本次会话出现的次数，**不代表重要性**（`productInfo` 高是因为轮询）。
**它可以直接用于 M4 界面的"我们要不要这一块"的逐项判断**——
比截图更适合做这个判断，因为它是一份**完备清单**，而截图永远只是抽样。

---

## 3. 结论槽

```
基线表完成度：8/8（"能做什么/不能做什么"维度，证据 A 级）
              ⚠️ "在哪做/几步/长什么样"维度：0/8（需人工截图，本轮无法产生）
结论：官方启动器的能力边界是：
  · 账户：只有微软账户，没有离线账户
  · 内容：基岩版有 addon；Java 版没有资源包/行为包/整合包管理
  · 世界：只有 Realms，**没有任何本地世界管理**（含备份/导入/导出）
  · 皮肤：**只能看，不能换**
  · 启动参数：支持 quickPlay（直接进服务器/世界）
  · 产品：同一个启动器承载 Java(retail-Piston) 与 基岩(retail-Uwp) 两类，按产品分目录
```

## 4. 对本项目的直接影响

| 发现 | 影响 |
|---|---|
| 官方**没有本地世界管理** | 我们的"世界级备份"是**超出**而非追平 → **M9 验收标准不能写成"与官方一致"**，否则该项无法判定 |
| 官方**没有离线账户** | 我们的离线账户是**超出**。但它同时意味着**没有可对标的实现**，所以规格要自己写清（已写：`docs/身份与授权规格.md` §1.3） |
| 官方**皮肤只能看不能换** | 我们若做"更换皮肤"，同样是超出项，验收标准自定 |
| 官方支持 `quickPlay` | **我们必须支持**（否则是"不如官方"），且机制已知：配置开关 + 按产品的目录 + 界面事件 |
| 官方用 `launcher_product_state` 分产品记状态 | **印证我们的多产品架构**：官方自己就是"一个启动器 + 多产品 + 按产品分目录"，与方案 §3 的设计同构 |
| `redstone-launcher.mojang.com` 是官方更新通道 | 我们**不采用**（方案 §4.8 用自己的更新机制），但需登记该服务的存在以免误判 |

## 5. 未完成的部分（**不许省略**）

| 缺什么 | 为什么缺 | 怎么补 |
|---|---|---|
| **八项的界面位置与操作步数** | 截图须人工操作 | **需你操作一次**：我会输出一份"逐项点什么、截什么图"的清单 |
| **`enableAdvanced` 打开后的能力** | 当前为 `false`；打开后可能出现更多入口 | 你操作；或我下一轮先看它的字段含义 |
| **历史版本（`enableHistorical`）的行为** | 当前为 `false` | 同上 |
| **基岩版的 addon 管理在界面上长什么样** | 日志只有调用记录 | 你操作 |

## 6. 复现

```powershell
# 证据全部来自官方启动器自己写下的文件
Get-Content "$env:APPDATA\.minecraft\launcher_product_state_microsoft_store.json"
Get-Content "$env:APPDATA\.minecraft\launcher_settings.json"
Get-Content "$env:APPDATA\.minecraft\launcher_profiles.json"
Select-String -Path "$env:APPDATA\.minecraft\launcher_log0.txt" -Pattern 'ControllerInterface|coreEvent'
```

> **凭据检查（已做）**：该日志经 7 类模式扫描，
> **JWT / Bearer / access_token / refresh_token / 长哈希 / MSA 凭据路径 / 邮箱 —— 全部 0 命中**。
> `launcher_msa_credentials_microsoft_store.bin` **未被读取、未被引用**。
