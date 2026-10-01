# XMCL 源码级调研（X Minecraft Launcher · MIT · Electron + TS/Vue）

> 只读源码，未改动任何方案或计划文件。所有结论带 `路径:行号`。
> **最重要的结构性发现**：XMCL **不是"一个 Electron 应用"**，而是 **34 个可独立发布的 npm 包（`@xmcl/*`）+ 运行时 + 两套 UI**。
> 这直接决定了"可直接拿来用的资产"非常多——见文末清单 A。

---

## 1. Electron 主进程/渲染进程分工

**三层分离**（`pnpm-workspace.yaml` + 各 `package.json`）：

| 层 | 包 | 职责 |
|---|---|---|
| **纯逻辑包** | `packages/*`（34 个 `@xmcl/*`） | 版本解析、安装、下载、NBT、mod 解析……**全部不依赖 Electron** |
| **运行时** | `xmcl-runtime`（`@xmcl/runtime`）+ `xmcl-runtime-api`（`@xmcl/runtime-api`） | 服务层与**类型契约层**分离 |
| **UI** | `xmcl-keystone-ui`（`@xmcl/keystone-ui`）、`deskgap-app`（`@xmcl/deskgap-app`） | **两套 UI**（Keystone + DeskGap） |
| 应用壳 | `xmcl-electron-app`（`xmcl`）、`xmcl-asar`（`@xmcl/app`） | Electron 主进程 + 打包 |

- **`xmcl-runtime-api` 单独成包**是关键：UI 只依赖**类型契约**，不依赖实现。→ 与我们"引擎/API/外壳分离"同构。
- IPC 组织：主进程逻辑在 `xmcl-runtime`，渲染进程通过 `xmcl-runtime-api` 的类型调用。

**对我们的意义**：这是"**契约独立成包**"的完整先例，比"抽象成 trait"更彻底（跨进程、跨语言都能用）。

---

## 2. 实例与版本模型

- **版本目录**：`packages/core/folder.ts`（183 行）`MinecraftFolder` 定义 `versions/`、`libraries/`、`assets/` 布局。
- **版本元数据模型**：`packages/core/version.ts:24` `ResolvedVersion`、`:110` `LibraryInfo`（含 `groupId/artifactId/classifier/isSnapshot/type/path`，`:177` 起 `LibraryInfo.resolveFromPath()` **从 maven 路径反解坐标**）。
- **错误类型是结构化判别式联合**（`version.ts:133-171`）：

```ts
BadVersionJsonError      { error:'BadVersionJson', missing:'MainClass'|'AssetIndex'|'Downloads', version }
CorruptedVersionJsonError{ error:'CorruptedVersionJson', version, json }
MissingVersionJsonError  { error:'MissingVersionJson', version, path }
CircularDependenciesError{ error:'CircularDependencies', version, chain:string[] }
type VersionParseError = (以上之一 & Error) | Error
```

**`CircularDependenciesError` 带 `chain`** —— 它把"`inheritsFrom` 成环"当成一等错误并给出环路径。
→ **这正是我们 M7（加载器元数据合成）必须处理的失败面**，且 `chain` 的设计比只报"循环依赖"有用得多。

- **实例模型** `packages/instance/instance.ts`（407 行）+ `internal_type.ts`（31 行）。
- **`files_manifest` / `manifest_generation.ts`（219 行）**：生成实例文件清单——**可复现性的实现**。
- **`files_integrity.ts`（38 行）**：完整性校验。
- **版本化/迁移**：`resource/core/migrate.ts`（282 行）——**资源层有独立迁移**，值得读（我未展开）。

---

## 3. "磁盘高效"到底怎么做的（**与你我此前的假设都不同**）

**⚠️ 结论：它不是靠硬链接/符号链接，而是"内容哈希 + SQLite 元数据引用"。**

我把整个 `instance` 包与 `resource` 包按 `hardlink|symlink|junction|dedup` 全文检索过：

| 检索结果 | 位置 |
|---|---|
| 唯一的 `symlink` 命中 | `packages/resource/core/watchResourcesDirectory.ts:213` `followSymlinks: true`（是**目录监视**选项，不是创建链接） |
| 唯一的 `dedup` 命中 | `watchResourcesDirectory.ts:346` / `:427` —— **指"去重的事件上报"，不是文件去重** |
| **没有** 硬链接创建 | 全包无 `link(`／`hardlink` 命中 |

**真实机制**（`packages/resource/`，36 个文件）：

| 文件 | 行数 | 作用 |
|---|---|---|
| `core/hashResource.ts` | 59 | **SHA-1 内容哈希**（见 §4） |
| `ResourceMetadata.ts` | 135 | 资源元数据（含来源：CurseForge/Modrinth/Git 坐标） |
| `ResourceManager.ts` | 228 | 资源管理器 |
| `core/sqlHelper.ts` | 232 | **SQLite 关联表** |
| `core/watchResourcesDirectory.ts` | 498 | 目录监视 + 增量扫描 |
| `Resource.ts` / `File.ts` / `ResourceDomain.ts` / `ResourceType.ts` | 80/38/18/10 | 模型 |
| `core/sweepCorruptedRefs.ts` | 65 | **清理损坏引用** |
| `core/takeSnapshot.ts` | 105 | 快照 |
| `parsers/*`（8 个） | 14–194 | 按类型解析 mod/资源包/光影包 |

- **`ResourceType`**（10 行，可直接抄的形状）：`Forge / Neoforge / Liteloader / Fabric / Quilt / ResourcePack / ShaderPack / Blueprint`
- **`ResourceDomain`**（18 行）：`Mods='mods' / ResourcePacks='resourcepacks' / ShaderPacks='shaderpacks' / Blueprints='schematics' / Unclassified`，
  并带 **`getResourceTaskPriority(domain)`**：`Mods=-1 > ResourcePacks=-2 > ShaderPacks=-3 > 其他=-4`
  → **扫描任务的优先级是模型自带的**，很实用。
  → 注意 `Blueprints` 的注释：*"Stored under the `schematics` folder so it lines up with **Litematica's** default location"* ——
  **与 Portal 的 `.litematic` 工具撞在同一处需求**，说明这是真实用户诉求。

**对我们的意义**：**"磁盘高效"的正解是内容哈希 + 元数据引用，而非建链接**。
链接方案（Polymerium 的 symlink / Axolotl 的 junction）是**另一种**做法，代价是 Windows 权限与跨卷问题。
**两条路我们应该选"哈希+引用"为主、"链接"为可选加速**——因为前者不需要开发者模式。

---

## 4. 下载与文件层

`packages/file-transfer/`（11 个文件）：

| 文件 | 行数 | 作用 |
|---|---|---|
| `download.ts` | **635** | 下载主流程 |
| `controlled_handler.ts` | 347 | 受控处理器 |
| `controller.ts` | 244 | 控制器 |
| `concurrency_dispatcher.ts` | 225 | **并发调度器**（继承 `undici.Dispatcher`） |
| `file_handler.ts` | 194 | 落盘 |
| `range_handler.ts` | 111 | Range 处理 |
| `range_policy.ts` | 75 | **Range 策略**（何时该用 Range） |
| `progress.ts` | 84 | 进度 |
| `error.ts` | 78 | 错误 |

**并发调度器的遥测结构**（`concurrency_dispatcher.ts:13-25`，**形状可直接借鉴**）：

```ts
interface ConcurrencyDispatcherTelemetry {
  requests, queuedRequests, queuedAborted
  maxActive, maxPending
  queueWaitMs, maxQueueWaitMs
  minLimit, maxLimit        // 并发上限的动态范围
}
```

**它把"队列等待时间"与"并发上限区间"当一等指标** —— 这正是我们 S5（下载总时长）需要的可观测性，
比只报"下载速度"更能说明瓶颈在哪（是带宽还是排队）。

- **`hashResource.ts`（59 行，纯算法，可直接移植）**：

```ts
const THREASHOLD = 65536 * 20   // ≈ 1.310 MB
// ≥ 1.31MB：流式 createReadStream + sha1（不全读内存）
// < 1.31MB：readFile 后直接 sha1
// 目录：递归 hash(文件名) + mtimes(Uint32Array)，返回 [hash, 'directory']
// 另有 hashAndFiletypeResource：用 file-type 嗅探真实扩展名（防扩展名伪装）
```

→ **三个可直接用的点**：① **1.31 MB 分流阈值**（避免小文件走流式开销）；② **目录哈希算法**（文件名 + mtime，含 `.DS_Store` 排除）；③ **file-type 嗅探**（.jar 可能其实是 .zip）。

---

## 5. 加载器与整合包

`packages/installer/`（36 个文件，最厚）：

| 加载器 | 文件 | 行数 |
|---|---|---|
| Forge | `forge.ts` / `forgeWorkflow.ts` / `forge.browser.ts` | 579 / 368 / 80 |
| NeoForge | `neoforge.ts` | 80 |
| Fabric | `fabric.ts` / `fabric.browser.ts` | 182 / 85 |
| Quilt | `quilt.ts` / `quilt.browser.ts` | 123 / 44 |
| OptiFine | `optifine.ts` | 184 |
| LiteLoader | （见 mod-parser `liteloader.ts`） | 47 |
| LabyMod | `labymod.ts` / `labymod.browser.ts` | 310 / 138 |
| Installer 流程 | `installManifest.ts` / `versionInstallManifest.ts` / `profile.ts` | **695** / 215 / **624** |
| Java | `java.ts` / `javaWorkflow.ts` / `javaInstallManifest.ts` / `java-runtime.browser.ts` | 241 / 188 / 95 / 250 |
| Java 运行时分发 | `zulu.ts` | 148 |
| 其它 | `assets.ts` 262 / `libraries.ts` 174 / `minecraft.ts` 114 / `move.ts` 36 / `tracker.ts` 95 / `diagnose.ts` 106 |

**整合包格式**（`packages/instance/parsers/`）：

| 解析器 | 行数 |
|---|---|
| `modrinth_parser.ts` | 189 |
| `multimc_parser.ts` | 220 |
| `curseforge_parser.ts` | 90 |
| `vanilla_parser.ts` | 124 |
| `modpack.ts`（主流程） | **709** |

**`launcher_parser.ts`（246 行）—— 交叉启动器识别，值得单列**：

```
:16  getMultiMCGameDirectory, isMultiMCInstance, parseMultiMCInstance, readMultiMCManifest
:19  Check if a path is a Modrinth instance
:26  Check if a path is a CurseForge instance
:33  Check if a path is a Vanilla Minecraft installation
:42  export function detectLauncherType(path): InstanceType | null      ← 自动识别
:72  export async function parseLauncherData(...)                        ← 解析出实例 + 共享目录
:223 export async function parseInstanceFiles(path, type?)               ← 解析单实例文件
```

→ **与 Axolotl 的 `linked_launcher`（直连外部启动器数据目录）是同一件事的两份独立实现**。
**两个项目都做了**，说明这不是花活，是真实需求。我们的"导入其它启动器"应该按这个形状设计。

---

## 6. 认证

`packages/user/`（`user` 包只有 `undici` 一个外部依赖 → **算法与流程可移植**）：

- 微软登录、Yggdrasil（**`user/yggdrasil.ts:15` 有 `createHash`** —— 外置登录的请求签名）、
  **`packages/user-offline-uuid/`（独立包！离线 UUID 生成规则单独成包）**。
- **令牌存储位置**：未在 `packages/user` 内发现持久化实现 → **存储在 `xmcl-runtime`（Electron 侧）**，属 Node 依赖层。
- **`user-offline-uuid` 单独成包**这件事本身值得注意：离线 UUID 的生成规则（`OfflinePlayer:<name>` 的 MD5 变体）
  **是跨启动器的兼容契约**，把它独立出来是对的。**我们应同样把它做成一个独立小模块 + 测试。**

---

## 7. 启动链路

`packages/core/`：

| 文件 | 行数 | 内容 |
|---|---|---|
| `launch.ts` | **1006** | 参数组装 + 启动 |
| `version.ts` | **1037** | 版本解析 + 继承链 + 库解析 |
| `folder.ts` | 183 | 目录布局 |
| `header.ts` | 213 | 认证头 |
| `platform.ts` | 37 | 平台判定 |
| `java.ts` | 25 | Java 接口 |
| `utils.ts` | 56 | **`checksum` / `validateSha1` / `isNotNull`** |

**可直接抄的四处（`launch.ts`）**：

1. **占位符替换就是一行正则**（`launch.ts:19-24`）：

```ts
function format(template: string, args: any) {
  return template.replace(/\$\{(.*?)}/g, (key) => {
    const value = args[key.substring(2).substring(0, key.length - 3)]
    return value || key           // 找不到就保留原样，不静默变成 undefined
  })
}
```

→ 注意最后 `return value || key`：**占位符未提供时保留 `${...}` 原文**，便于诊断。我们的实现应照做（不要替换成空串）。

2. **默认 JVM 参数是一个 frozen 常量**（`launch.ts:26-34`）：

```ts
export const DEFAULT_EXTRA_JVM_ARGS = Object.freeze([
  '-Xmx2G', '-XX:+UnlockExperimentalVMOptions', '-XX:+UseG1GC',
  '-XX:G1NewSizePercent=20', '-XX:G1ReservePercent=20',
  '-XX:MaxGCPauseMillis=50', '-XX:G1HeapRegionSize=32M',
])
```

3. **`EnabledFeatures` 是 rules 判定所需的 features 形状**（`launch.ts:35-42`）：

```ts
interface EnabledFeatures {
  has_custom_resolution?: { resolution_width: string; resolution_height: string }
  is_demo_user?: boolean
}
```

→ **这是 `rules.features` 那条判定链的输入类型**，我们 M2/M3 会需要一模一样的形状。

4. **`LaunchOption` 是完整的启动输入契约**（`launch.ts:48-100+`）：含 `gameProfile{name,id}`、`accessToken`、`userType`、`properties`、
   `launcherName`/`launcherBrand`、`versionName`/`versionType`（可覆盖，注释说是为了在欢迎界面显示自定义文案）、
   `gamePath`、`gameIcon`/`gameName`（Mac 专用）。

---

## 8. 状态管理与前端

- UI 包 `xmcl-keystone-ui`（Vue）与 `deskgap-app`（另一套）。
- `i18n/` 目录独立在仓库根部（不在 UI 包内）→ **语言资源与 UI 解耦**。
- 根目录 `protocol.json` —— **自定义协议的集中定义**（与我们规划的 `qul://` 类似）。

---

## 9. 错误与日志

**`packages/installer/error.ts`（74 行，全文读，**可直接抄的错误模型**）**：

```ts
class InstallError extends Error {
  constructor(public issue: InstallIssue = {}, message = '', cause?: Error) { ... }
}

interface InstallIssue {
  jar?: string                                     // 坏的游戏 jar
  forge?: { minecraft: string; version: string }   // 坏 forge 安装
  libraries?: ResolvedLibrary[]                    // 需要安装的库
  assets?: { name: string; hash: string; size: number }[]   // 失败的资源
  assetsIndex?: Version.AssetIndex                 // 坏的资源索引
  profile?: InstallProfile
  optifine?: string                                // e.g. "1.12.2_HD_U_G6_pre1"
}
```

三个可直接抄的设计：

1. **错误带结构化 `issue`，而不是一个字符串** —— 每个字段指明**失败面**（jar / forge / libraries / assets / profile / optifine）。
   → 这正是我们 §5.7"错误级别表"缺的另一半：**级别说明"多严重"，issue 说明"哪里坏了"**。
2. **`mergeInstallIssue(target, source)`（`:42-71`）** —— **多次失败可聚合**：
   `libraries`/`assets` 是**数组 concat**（保留全部失败项），其余是覆盖。
   → 用户能看到"3 个库 + 12 个资源"完整失败清单，而不是只报最后一条。
3. **`isInstallError(e)` 用"鸭子类型 + `name` 双重判定"**（`:73-74`）：
   `e instanceof InstallError || (typeof e === 'object' && 'issue' in e && e.name === 'InstallError')`
   → **跨进程/跨序列化边界后 `instanceof` 会失效**，所以补一层结构判定。**这对我们（Rust→TS 错误传递）直接适用。**

---

## A. 可直接用的资产（**与 Node/Electron 无关，或仅依赖可替换的小库**）

| # | 资产 | 位置 | 与 Node 的关系 | 能怎么用 |
|---|---|---|---|---|
| **A1** | **`@xmcl/semver`**（Fabric 语义化版本 + 区间匹配，**573 行**） | `packages/semver/{semver.ts:269, range.ts:186, operators.ts:83}` | **零依赖纯逻辑** | **最大的一项**。文件头写明 *"Copyright 2016 **FabricMC**, Apache-2.0"* → 它是 **Fabric 官方语义化版本实现的 TS 移植**。**我们不必自己实现 MC/Fabric 版本比较与区间匹配，直接按逻辑移植到 Rust 并保留 Apache 头** |
| **A2** | **`@xmcl/gamesetting`**（522 行） | `packages/gamesetting/index.ts` | **零依赖纯逻辑** | `options.txt` 的**完整解析/序列化 + 枚举值表**（`AmbientOcclusion{Off0,Min1,Max2}`、`Particles{Min2,Dec1,All0}`、`Difficulty{Peaceful..Hard}`、`MipmapLevel 0-4`、`RenderDistance 2-32`、`RenderDistances{Tiny2,Short4,Normal8,Far16,Extreme32}`）。**枚举数值是游戏的硬契约，抄对就省掉查 wiki** |
| **A3** | **`@xmcl/text-component`**（489 行） | `packages/text-component/index.ts` | **零依赖纯逻辑** | Minecraft **Raw JSON 文本格式**的完整类型定义 + 渲染。聊天/崩溃信息/tooltip 都涉及；**`TextComponent` 接口带逐字段注释，等于一份格式规格** |
| **A4** | **`@xmcl/nbt`**（18 KB，另含 `utils.ts` + 独立 `zlib/` 适配层） | `packages/nbt/` | 仅内部依赖 `@xmcl/bytebuffer`，**zlib 用 browser/node 双实现隔离** | **Java + 基岩双版 NBT 格式读写**。基岩版世界管理（我们 M9）**必然要用** |
| **A5** | **`@xmcl/asm`** | `packages/asm/` | **零依赖** | Java class 文件解析的最小实现。用于**从 mod jar 里读元数据而不解压执行** |
| **A6** | **`@xmcl/schematic`** | `packages/schematic/` | 仅依赖 `@xmcl/nbt` | 蓝图/原理图格式解析（`ResourceDomain.Blueprints` 对应的 `.litematic`/`.schematic`）。**百宝箱"投影材质查看"直接对应** |
| **A7** | **`@xmcl/bytebuffer`** | `packages/bytebuffer/` | **零依赖** | 二进制读写工具，NBT/ASM 的基础 |
| **A8** | **`@xmcl/mod-parser`**（`fabric.ts` 205 / `quilt.ts` 278 / `forge.ts` **826** / `liteloader.ts` 47 / `forgeConfig.ts` 179） | `packages/mod-parser/` | 仅依赖 `@xmcl/asm` + `@xmcl/system` | **五类 mod 的元数据解析规则**。特别是 `forge.ts:826 行`——**Forge mod 的元数据读取（`mcmod.info` / `MANIFEST` / `@Mod` 注解）规则表几乎不可能自己写对** |
| **A9** | **`@xmcl/resourcepack`** | `packages/resourcepack/` | 仅 `@xmcl/system` | 资源包 `pack.mcmeta` 解析 |
| **A10** | **`InstallIssue` + `mergeInstallIssue` + `isInstallError`** | `packages/installer/error.ts`（74 行全文） | **纯类型与纯函数** | **错误模型规格**：失败面分类 + 多次失败聚合 + 跨序列化边界的判别。**可直接翻译成 Rust enum + serde** |
| **A11** | **`VersionParseError` 联合 + `CircularDependenciesError.chain`** | `packages/core/version.ts:133-171` | **纯类型** | `inheritsFrom` 成环时的错误形状规格（带环路径） |
| **A12** | **`ResourceType` + `ResourceDomain` + `getResourceTaskPriority`** | `packages/resource/ResourceType.ts`（10 行）、`ResourceDomain.ts`（18 行） | **纯枚举 + 纯函数** | 资源类型/域分类 + **扫描优先级**规格。20 余行，直接照搬 |
| **A13** | **`hashResource` 三算法** | `packages/resource/core/hashResource.ts`（59 行） | Node 的 `crypto`/`fs` → **Rust 侧用 `sha1` + `tokio::fs` 等价替换即可** | ① **1.31 MB 流式/内存分流阈值** ② **目录哈希**（文件名 + mtime + 排除 `.DS_Store`）③ **file-type 魔数嗅探**防扩展名伪装 |
| **A14** | **`ConcurrencyDispatcherTelemetry`** | `packages/file-transfer/concurrency_dispatcher.ts:13-25` | 纯类型 | **下载可观测性指标集**（队列等待、并发上限区间、排队/中止数）。我们 S5 的埋点清单可直接照它 |
| **A15** | **`LibraryInfo.resolveFromPath`** | `packages/core/version.ts:177+` | 纯函数（字符串处理） | 从 maven 路径反解 `groupId/artifactId/version/classifier`。**加载器处理里到处要用** |
| **A16** | **`format()` 占位符替换 + `DEFAULT_EXTRA_JVM_ARGS` + `EnabledFeatures`** | `packages/core/launch.ts:19-42` | 纯函数/常量 | ① 占位符正则（**未命中保留原文，不替换成空**）② 默认 JVM 参数常量 ③ **`rules.features` 的输入类型** |
| **A17** | **`launcher_parser` 的识别器形状** | `packages/instance/launcher_parser.ts`（246 行） | 有 fs 依赖，但**接口形状可移植** | `detectLauncherType / parseLauncherData / parseInstanceFiles` 三函数契约——**做"导入别的启动器"照这个形状设计** |
| **A18** | **`user-offline-uuid` 独立成包** | `packages/user-offline-uuid/` | 纯逻辑 | 离线 UUID 生成规则是**跨启动器兼容契约**，值得独立成模块 + 测试 |
| **A19** | **可用的独立第三方依赖** | 各 `package.json` | — | ① **`file-type`**（魔数嗅探，magic bytes 表，跨语言可移植）② **`node-html-parser`**（`forge-site-parser` 用于解析 Forge 官网；Rust 侧对应 `scraper`/`html5ever`）③ **`yauzl`**（**惰性 ZIP 读取，不全量载入内存**——大整合包解压省内存）④ **`undici`**（高性能 HTTP 客户端；Rust 侧我们已有 reqwest） |
| **A20** | **`blueprint` → `schematics` 目录对齐 Litematica** | `packages/resource/ResourceDomain.ts:8-11` 注释 | — | **目录命名规格**：蓝图类文件放 `schematics/` 以对齐 Litematica 默认位置。**这类"生态约定"是自己查不出来的** |

---

## B. 值得抄的机制（12 条，按价值排序）

| # | 抄什么 | 抄到什么程度 | 证据 |
|---|---|---|---|
| **1** | **纯逻辑拆成可独立发布的包**，UI 只依赖**契约包** | 采纳组织方式：契约独立（对应我们的 `qul-plugin-api` 思路），纯逻辑包零框架依赖 | 34 个 `@xmcl/*` + `xmcl-runtime-api` 独立 |
| **2** | **`InstallIssue` 结构化失败面 + `mergeInstallIssue` 聚合** | **全抄**（含 `isInstallError` 的鸭子类型兜底） | `installer/error.ts:9-74` |
| **3** | **"磁盘高效"= 内容哈希 + SQLite 引用**（不是建链接） | **选它为主路径**，链接只作可选加速 → **避免开发者模式依赖** | `resource/core/hashResource.ts` + `sqlHelper.ts` + 全包无 hardlink |
| **4** | **`inheritsFrom` 成环错误带 `chain` 路径** | 全抄形状 | `core/version.ts:158-166` |
| **5** | **`ConcurrencyDispatcherTelemetry` 的指标集** | 全抄，作为 S5 埋点清单 | `file-transfer/concurrency_dispatcher.ts:13-25` |
| **6** | **占位符未命中时保留 `${...}` 原文** | 全抄（便于诊断） | `core/launch.ts:19-24` |
| **7** | **`ResourceDomain` 的扫描优先级内建于模型** | 全抄（20 行） | `resource/ResourceDomain.ts:13-17` |
| **8** | **`file-type` 魔数嗅探防扩展名伪装** | 全抄（.jar 可能是 .zip，直接影响解析路径选择） | `core/hashResource.ts:44-56` |
| **9** | **目录内容哈希（文件名 + mtime + 排除 `.DS_Store`）** | 全抄算法 | `core/hashResource.ts:22-43` |
| **10** | **`launcher_parser` 三函数契约**（detect / parseLauncherData / parseInstanceFiles） | 抄契约，与 Axolotl 的 `linked_launcher` 合并设计 | `instance/launcher_parser.ts:42/72/223` |
| **11** | **`offline-uuid` 独立成包** | 全抄组织方式 | `packages/user-offline-uuid/` |
| **12** | **`sweepCorruptedRefs`（清理损坏引用）+ `takeSnapshot`** | 采纳入设计：资源库会腐化，需要主动清扫 | `resource/core/sweepCorruptedRefs.ts:65`、`takeSnapshot.ts:105` |

---

## C. 明确不适合的

| 不适合 | 理由 | 证据 |
|---|---|---|
| **Electron 架构本身** | 我们已选 Tauri 2；体积/内存/进程模型完全不同 | `xmcl-electron-app` |
| **`undici` 作为下载底座** | Node 专属；Rust 侧用 reqwest/hyper | `file-transfer/concurrency_dispatcher.ts:1` |
| **`node-html-parser` 解析 Forge 官网** | Node 专属；且**抓官网页面**本身脆弱（Forge 改版即失效）——只作最后兜底 | `packages/forge-site-parser` |
| **34 个包全盘照搬** | 它拆这么细是为了 npm 生态分发；我们是单体应用，**拆到"纯逻辑 / 契约 / 外壳"三层即可**，过度拆包会抬高我们自己的构建成本 | `packages/*` 全清单 |
| **`Resource` 全量目录监视**（498 行 watcher） | 长驻文件监视在 WebView 环境成本高；**我们的"实时刷新"应改为手动/按需扫描 + 轻量失效标记** | `resource/core/watchResourcesDirectory.ts` |
| **对 CurseForge/Modrinth 的直连 API 客户端形态** | 我们要走**用户可配置的 API Key**（CurseForge 2026-07 起强制），不能内置；它的 `curseforge`/`modrinth` 包只是薄封装，无 key 管理设计 | `packages/curseforge`、`packages/modrinth` |
| **`asm`/`bytebuffer` 原样移植** | 它们是 JS 生态的二进制处理选择；**Rust 侧有更成熟的 `zip`/`nbt`/`classfile` crate** —— 但**它们的解析规则（A5/A8）仍要读**，因为规则来自游戏本身 | `packages/asm`、`packages/bytebuffer` |
| **两套 UI（Keystone + DeskGap）并存** | 历史包袱；我们只做一套 | `xmcl-keystone-ui`、`deskgap-app` |

---

## 一句话总结

**XMCL 的价值不在"Electron 架构"，而在它把 Minecraft 生态的脏活全做成了零依赖的纯逻辑包**：
Fabric 官方语义化版本实现（Apache-2.0）、`options.txt` 完整枚举表、Raw JSON 文本格式规格、NBT 双版读写、
五类 mod 的元数据解析规则、`InstallIssue` 错误模型、目录哈希与魔数嗅探算法——
**这些是"抄不到就得自己踩坑"的东西**，且**多数与 Node 无关，可直接移植或按规格重写**。
