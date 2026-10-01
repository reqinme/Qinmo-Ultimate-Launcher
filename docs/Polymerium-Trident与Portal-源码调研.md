# Polymerium / Trident.Net + Portal 源码调研（剩余未读部分）

> 真读源码，未改动任何方案文件。所有结论带 `路径:行号`。
> **GPL-3.0 范围内的 Trident 除 Pref 解析外，其余均为 MIT（`repos/Polymerium/LICENSE.txt`），可安全搬运代码。**
> Portal = **AGPL-3.0**，只读设计。

---

## 第一部分：Trident.Net（MIT）——可直接搬的资产最丰富

### 1. `profile.json` 完整模型（`FileModels/Profile.cs`，121 行）

**顶层（`:7-25`）**

| 字段 | 类型 | 说明 |
|---|---|---|
| `Name` | `string` required | 实例名 |
| `Setup` | `Rice` required | 核心声明 |
| `Overrides` | `IDictionary<string,object>` | 任意覆盖项 |

**12 个 Override 键常量（`:9-20`）——可直接搬的命名规范**

```
java.home                    java.max_memory              java.additional_arguments
window.height                window.width                 window.title
behavior.deploy.method       behavior.connect.address     behavior.command.wrapper
modpack.name                 modpack.author               modpack.version
```

**`Profile.Rice`（`:29-40`）**：`Source`(string?)、`SourceOrders`(IList<string>)、`Version`(string required)、`Loader`(string?)、`Packages`、`Rules`

> `:33-34` 注释：*"按叠加强度排序的 Source URI——先入先垫底、后者覆盖前者，末项为最顶层；空 = 依赖层默认档位"*

**`Rice.Entry`（`:44-67`）**：`Pref`(string)、`Enabled`(bool required)、`Source`(string?)、`Tags`(IList<string>)
- `:48-54` 有 `Purl` 遗留兼容属性（`[Obsolete]`），setter 里调 `PackageHelper.SafeMigrate(value)` —— **旧格式迁移手法可直接照抄**
- `:58-64` 注释揭示一个真实坑：*"CSV 清单往返曾把 null 洗成空串落盘，源头已在导入侧修复；待存量 profile 经数个版本的加载-保存自行净化后，移除归一化"* —— **"临时消毒"作为迁移策略的实例**

**`Rice.Rule`（`:73-115`）**

| 维度 | 取值（`:84`） |
|---|---|
| `SelectorType` | `And / Or / Not / Pref / Repository / Tag / Kind` |

```csharp
class RuleSelector {
    SelectorType Type = SelectorType.Pref;
    IList<RuleSelector>? Children;   // And/Or/Not 的组合递归
    string? Pref; string? Repository; string? Tag;
    ResourceKind? Kind;
}
// 动作（:110-112）—— 只有三个，很克制
string? Destination;   // 重定向目标
bool Skipping;         // 跳过部署
bool Normalizing;      // 归一化
```

### 2. 三层目录的实际使用（`FileModels/PathDef.cs`，225 行）

**完整路径表（`PathDef.cs`）——可直接搬**

| 常量/方法 | 路径 | 行号 |
|---|---|---|
| 实例根 | `instances/<key>/` | `:63,65` |
| `profile.json` | 实例根 | `:66` |
| `data.lock.json` | 实例根 | `:71` |
| `data.pack.json` | 实例根 | `:72` |
| `icon.<ext>` | 实例根 | `:68` |
| `_bomb_has_been_planted_` | 实例根 | `:73` |
| **`build/`** | 实例根 | `:74` |
| **`import/`** | 实例根 | `:85` |
| **`persist/`** | 实例根 | `:86` |
| `patches/` | `DirectoryOfHome` 下 | `:87` |
| `snapshots/` | 实例根 | `:88` |
| `snapshots/objects/<hash[:2]>/<hash>` | **内容寻址** | `:90-93` |
| `build/natives/` | | `:80,82` |

**5 个关键文件名常量（`:76-80`）**

```
trident.import.json              ← ImportProjectionManifest
trident.persist.json             ← PersistProjectionManifest
allowed_symlinks.txt             ← 符号链接白名单
.trident-manifest-tmp            ← 清单临时目录
natives                           ← natives 子目录名
```

**缓存布局（`:99-139`）**：`cache/{assets,icons,libraries,packages,runtimes}`
- `FileOfAssetObject(hash)` → `assets/objects/<hash[:2]>/<hash>`（`:137`）—— **与 Mojang 官方一致**
- `FileOfLibrary(ns,name,version,platform,ext)`：namespace 按 `.` 拆成目录（`:120`）
- `FileOfPackageObject(label,ns,pid,vid,ext)` → `packages/<label>[/<ns>]/<pid>/<vid><ext>`（`:130-133`）

**Home 定位（`:184-222`）**：`TRIDENT_HOME` 环境变量 → 从 cwd 向上找 `.trident` → `~/.trident.home` 文件首行 → 兜底 `~/.trident`
- `:11-13` 注释：*"启动瞬间冻结——有显式 override，或当时已存在遗留 ~/.trident，则整个进程统一用 EFFECTIVE_HOME 作单一根目录。不再用每次调用都现查的 Directory.Exists，否则运行中 ~/.trident 一旦被创建会令根目录在多根/单根之间漂移"* —— **根目录漂移的坑，可直接搬这个"启动时冻结"策略**

### 3. DeployEngine：**固定 9 阶段线性管线**

`Engines/DeployEngine.cs:22-33` —— 顺序写死在 `SEQUENCE` 数组：

```
1. LoadLockStage           载入并比对锁定态
2. InstallVanillaStage     安装原版
3. ProcessLoaderStage      处理加载器
4. ApplyLaunchPatchStage   应用启动补丁
5. SyncPackagesStage       同步包
6. SelectRuntimeStage      选择 Java 运行时
7. PersistLockStage        持久化锁定态
8. PlanDeploymentStage     生成部署计划
9. ExecuteDeploymentStage  执行部署
```

- `:10-11` 注释：*"固定线性管线——各阶段按序执行并自行（对照 BaseLock）决定迁移/重建/no-op。**无状态机分支：DecideNext 已移除，改为静态 yield 序列**"*
- `:37-42` 用 `ActivatorUtilities.CreateInstance` 从 DI 构造阶段；`:50-53` **每次 MoveNext 先 Dispose 上一个阶段**（`IDisposableLifetime`）；`:72` 注释 *"WARNING: 中断导致没有 MoveNext"*

**执行阶段（`Stages/ExecuteDeploymentStage.cs`，184 行）的关键细节**

| 机制 | 证据 |
|---|---|
| 并发 = **CPU 核数 - 1** | `:21-24` `MaxDegreeOfParallelism = Math.Max(Environment.ProcessorCount - 1, 1)` |
| 进度流 | `:12` `Subject<(int Current,int Total)> ProgressStream`；`:29` 每完成一个 `OnNext(++completed,total)` |
| **原子写文件** | `:89-99` `target + "." + Guid + ".tmp"` → `File.Copy` → `SetLastWriteTimeUtc(source)` → `File.Move(temporary, target, false)` → finally 删临时 |
| **持久化移动带冲突检测** | `:109-122` `MoveToPersist`：检查 `HasLinkAtOrAbove`、目录占用、目标已存在 → 否则 `File.Move` + `TrimEmptyParents` |
| **回灌带时间戳校验** | `:124-143` `BackportPersistFile`：**比对 `ExpectedTargetLastWriteTimeUtcTicks`**，不一致即抛 `Occupied` —— 乐观并发控制 |
| 符号链接 | `:151-160` `EnsureLink`：`Directory.CreateSymbolicLink` / `File.CreateSymbolicLink` |
| **allowed_symlinks.txt 内容** | `:168` `$"[prefix]{CachePackageDirectory}\n[prefix]{persist}"` |
| natives 提取 | `:70-74` 先 `DeleteLink(nativesDirectory)` 再 `ExtractAsync` |
| 权限位 | `:173-177` 非 Windows 时补 `UserExecute\|GroupExecute\|OtherExecute` |

### 4. 符号链接策略（**与 Axolotl 不同，值得注意**）

Trident **只用符号链接，无 junction/hardlink 降级链**（全项目仅 `ExecuteDeploymentStage.cs:158-159` 两处 `CreateSymbolicLink`）。
它解决"链接安全"的方式是**白名单文件**：`build/allowed_symlinks.txt` 声明允许的链接目标前缀（`:168`）。
→ **两种思路**：Axolotl 用分级降级（junction 免开发者模式），Trident 用符号链接 + 白名单。**Windows 免开发者模式场景下 Axolotl 更实用。**

### 5. `pref://` 解析与格式化（`TridentCore.Pref`，10 文件）

**规范（`Building/Builder.cs:22-64` + `Parsing/Parser.cs:27-55`）**

```
pref://<repository>[/<namespace>]/<identity>[@<version>][?k=v&k2=v2]

Scheme 常量: Building/Builder.cs:8  Scheme = "pref"
```

**解析要点**
- `:17-22` 先试 `Uri.TryCreate` 且 scheme == "pref"，否则回落 legacy
- `:32-39` **`@version` 取路径段最后一个 `@`** —— 注释 *"'@' 是合法 pchar，@version 落在路径段尾部"*
- `:41-52` 第一个 `/` 之前是 namespace，之后是 identity
- `:57-78` query 按 `&` 拆、`=` 拆；`eq <= 0` 的段跳过（**容忍空 key**）
- `:115` **legacy 正则原文**（可直接搬为兼容测试用例）：
  ```
  ^(?<label>[a-zA-Z0-9._-]+):((?<namespace>[a-zA-Z0-9._-]+)/)?(?<identity>[a-zA-Z0-9._-]+)(@(?<version>[a-zA-Z0-9._-]+))?(#(?<filter>[a-zA-Z0-9._-]+)=(?<value>[a-zA-Z0-9._-]+))*$
  ```
- `:112` 解析失败抛 `FormatException`（无消息）

**类型（`PackageIdentifier.cs:5`）**：`readonly record struct PackageIdentifier(string Repository, string? Namespace, string Identity, string? Version)`

### 6. 部署规则：selector + action

见 §1 的 `Rice.Rule`。**选择器 7 维（And/Or/Not/Pref/Repository/Tag/Kind），动作 3 个（Destination/Skipping/Normalizing）**。
锁定态里规则评估结果**按包冻结**（`LockData.cs:87-89`）：
> *"WARNING: 锁定时刻冻结的规则评估结果。按包存储，**规则微调只重算受影响包、绝不重解析**（重解析会漂移 floating pref）"*

→ **这条是设计要点**：规则改动不应触发全部包重解析。

### 7. 快照与 diff：**内容寻址 + 引用计数 GC**

- 存储（`PathDef.cs:90-93`）：`snapshots/objects/<hash[:2]>/<hash>` —— **与 git 同构**
- `SnapshotInfo`（`Snapshots/SnapshotInfo.cs:5-12`）：
  ```csharp
  record SnapshotInfo(object Id, string Label, string Remark, Profile.Rice Metadata,
                      int PackageCount, int FileCount, long TotalSize, DateTime CreatedAt);
  ```
  → **`Metadata` 存完整 `Profile.Rice`**，快照 = 元数据快照 + 内容寻址文件集
- `ISnapshotStore`（`Snapshots/ISnapshotStore.cs`，18 行）：
  ```
  InsertSnapshot / GetSnapshots / GetSnapshot / GetReferences
  DeleteSnapshot / GetAllReferencedHashes / DeleteOrphanReferences
  ```
  → **`GetAllReferencedHashes` + `DeleteOrphanReferences` = 引用计数式垃圾回收**，可直接搬

### 8. 导入导出：**5 格式，且 Modrinth 导出已避开 Prism 的缺陷**

| 导入器（`Importers/`） | 导出器（`Exporters/`） |
|---|---|
| `TridentImporter`(5.4KB) `ModrinthImporter`(5.8) `CurseForgeImporter`(4.1) `MultiMcImporter`(4.9) `PackwizImporter`(3.2) | `TridentExporter`(7.8) `ModrinthExporter`(6.0) `CurseForgeExporter`(4.3) `MultiMcExporter`(4.0) |

**ModrinthExporter 的校验逻辑（`Exporters/ModrinthExporter.cs`）——正是我们关心的那点**

- `:48-68` **`OfflineMode` 分支**：离线时把包**实体化进 `overrides/`**（`container.OverrideDirectoryName = "overrides"`，`:39`），并用 `PackageMaterializer.MaterializeAsync` 收集 `(RelativeTargetPath, absPath)`
- `:69-103` **在线分支**：把已解析包写进 **`index` 的 `files[]`**（含 `sha1`/`sha512`/`downloads[]`/`fileSize`），**不进 overrides**
- `:86-102` 明确 `PackIndex.IndexFile(path, hashes, new("required","unsupported"), [download], size)`
- `:105-109` `dependencies` 字典：`minecraft` + 按 `LOADER_MAPPINGS`(`:19-25`) 映射的加载器键

→ **结论：它正确区分了"在线可下载（进 index）"与"离线实体文件（进 overrides）"**，
Prism #1165 那个"把所有 mod 都塞进 Overrides"的缺陷**在 Trident 里不存在**。
**Loader 映射表（`:19-25`）可直接搬**：
```
forge→forge   neoforge→neoforge   fabric→fabric-loader   quilt→quilt-loader
（内部 id 常量：LoaderHelper.LOADERID_FORGE / NEOFORGE / FABRIC / QUILT）
```

### 9. 账户与认证（`Core/Accounts/` + `Core/Clients/`）

- **4 种账户实现**：`MicrosoftAccount` / `OfflineAccount` / `TrialAccount` / `AuthlibAccount`，各配 `*Configurer`
- **13 个客户端接口**（`Core/Clients/`）：
  `IAuthlibInjectorClient` `ICurseForgeClient` `IGitHubClient` `IMclogsClient` `IMicrosoftClient`
  `IMinecraftClient` `IModrinthClient` `IMojangLauncherClient` `IMojangPistonClient`
  `IPrismLauncherClient` `IXboxLiveClient` `IXboxServiceClient` `IYggdrasilClient`
- **完整 API DTO 集（`Core/Models/`，MIT，可直接搬）**：
  - `MicrosoftApi/`：`DeviceCodeResponse` `TokenResponse` `AuthenticateRequest` `RefreshUserRequest` `AcquireUserCodeRequest`
  - `XboxLiveApi/`：`XboxLiveAuthenticateRequest` `XboxLiveResponse` `XstsAuthorizeRequest` `XboxLiveTokenProperties` `MinecraftTokenProperties`
  - `MinecraftApi/`：`AcquireAccessTokenByXboxServiceTokenRequest` `MinecraftLoginResponse` `MinecraftProfileResponse` `MinecraftStoreResponse`
  - `YggdrasilApi/`：7 个（含 `YggdrasilAuthenticateRequest/Response` `YggdrasilRefreshRequest` `YggdrasilValidateRequest` `YggdrasilGameProfile` `YggdrasilProfileResponse` `YggdrasilAgent`）
  - `AuthlibInjectorApi/`：`AuthlibInjectorArtifactListResponse` `AuthlibInjectorArtifactResponse`
  - `CurseForgeApi/`：**15 个**（`ModInfo` `FileInfo` `FingerprintMatches` `SearchResponse` `GetFingerprintMatchesRequest` `SortableGameVersionModel` `ModLoaderTypeModel` …）
  - `ModrinthApi/`：`ProjectInfo` `VersionInfo` `SearchHit` `SearchResponse` `GameVersion` `ModLoader` `MemberInfo` `UserInfo` `VersionFilesRequest`
  - 打包格式：`ModrinthPack/PackIndex` `CurseForgePack/Manifest` `MultiMcPack/MmcPack`
  - 其他：`PrismLauncherApi/{Component,ComponentIndex,RuntimeManifest}` `MojangLauncherApi/{MinecraftNewsResponse,RuntimeEntry}` `MclogsApi/*` `GitHubApi/*`
  - 各启动器实例嗅探：`AtLauncher/AtLauncherInstance` `CurseForgeLauncher/CurseForgeInstance`

→ **这一整批 DTO 是我们最省设计开销的资产：字段名、JSON 属性名、必填性、枚举值全部现成。**

### 10. 包仓库与依赖解析

- `Repositories/ModrinthRepository.cs` / `CurseForgeRepository.cs` / `PackwizRepository.cs`（第三个仓库！）
- `:36-39` **`PackageResolver.ResolveAsync` 的关键设计**：
  > *"同项目同版本现可来自不同源（SyncPackages 以 (project, source) 为键）；把一次解析扇出给共享该键的每个条目"*
  - 用 `ToLookup(x => x.Key, x => x.Origin)` 做扇出，`ResolveBatchAsync` 批量解析后 `ThrowIfFailures()`
  - 解析键 = `PackageIdentifier(Repository, Namespace, Identity, Version)`（`:24`）

### 11. 迁移（`Services/MigratorAgent.cs`，294 行）——**可直接搬的完整迁移框架**

- `:19-24` 按 `LauncherKind` 建 adapter 索引；`SupportedKinds` 暴露支持列表
- **`:26-37` `ScanAsync` + `:23-24` `DefaultDataDirectory(kind)`** —— 每个启动器一个 adapter
- **`:39-153` `MigrateAsync` 的三个关键设计**：
  1. **`:51-77` 先批量指纹识别**：`GatherIdentifiableFiles` 只收 `.jar`/`.zip`（`:190-198`），
     `repository.IdentifyBatchAsync` 用 CurseForge 指纹批量识别 → **把本地文件还原成 pref 引用**（`:213-217` `PackageHelper.ToPref(p)`）
  2. **`:81-87` 取消以实例为边界**：注释 *"cancellation honours the instance boundary — the in-flight instance finishes its file copy and migration stops before the next one, so a started instance always lands whole or not at all"*
  3. **`:107-143` 注册放在最后 + 失败整体清理**：注释 *"Register only after both runtime files and patch sources have landed; a failed transfer must remove the entire reserved instance"*，用 `finally { if(!registered) { reservedKey.Dispose(); BestEffortDelete(...) } }`
- `:115-120` MultiMC/Prism 特殊路径：`MultiMcImporter.ExtractSourceAsync` + `importers.ExtractPatchesAsync`
- `:205-220` `BuildProfile` 生成标准 profile

→ **`LauncherKind` 枚举 + `ILauncherAdapter` 接口 + 指纹还原 + 实例边界取消 + 全或无注册**，这套骨架 MIT 可搬。

---

## 第二部分：Portal（AGPL，只读设计）

### 1. Java 侧实例模型（`Portal.Core/Minecraft/Classes/MinecraftInstance.cs`，893 行）

**`JavaInstanceConfig : MinecraftInstanceConfig`（`:867-887`），19 个字段：**

| 字段 | 类型 | 默认 |
|---|---|---|
| `EnableIndependentInstance` | bool | **true** |
| `EnableSpecificJava` | bool | — |
| `EnableOverrideMaxMemory` | bool | — |
| `MinecraftMaxMemory` | int | — |
| `JvmArgs` | string? | — |
| `SpecificJavaEntry` | JavaRuntimeEntry? | — |
| `GraphicsApi GraphicsBackend` | enum | `Default` |
| `OpenGlRenderer` / `VulkanRenderer` | string? | — |
| `EnableFullscreen` | bool | — |
| **`AutoSetChineseLanguage`** | bool | **true** ← 中文用户默认开 |
| `EnableGameOverlay` | bool | true |
| `OverrideMinecraftWindowTitle` | string? | — |
| `MinecraftWindowWidth/Height` | int | **854 / 480** |
| `BeforeLaunchCommand` / `AfterLaunchCommand` / `PackagedCommand` | string? | — |

- `:889-893` `enum MinecraftInstanceType { Java, Bedrock }` —— **与我们 provider 抽象同构**
- `EnableIndependentInstance` 在 Java 侧**默认 true**（基岩侧默认 false），且 `RequiresIndependentInstance` 会强制打开（见父 agent 先前记录 `:408-409`）

**目录布局识别（`Classes/MinecraftFolderLayout.cs`，360 行）—— 可直接借鉴的"多启动器共存"设计**

`:5-17` 10 种布局：
```
Auto / Standard / Modrinth / ModrinthInstance / MultiMc / MultiMcInstance
/ CurseForge / CurseForgeInstance / PortalMc / Unknown
```
- `:80-95` `Detect(path)` 按特征识别：含 `instances`+`libraries`+`assets`+`meta/net.minecraft` → MultiMC
- `:29-36` `GetMultiMcBrand` 从目录名反推品牌：`.BakaXL`→BakaXL、`PrismLauncher`→Prism Launcher、`MultiMC`→MultiMC
- `:52-78` `IsPortalMcRoot` / `TryFindPortalMcRoot`：**从任意子目录向上找实例根**
- `:25-27` `SupportsTraditionalInstallation`（仅 Standard）/ `SupportsInstallation`（仅 PortalMc）—— **能力布尔值写在布局类型上，与我们的能力描述符同思路**

### 2. 内容管理

- `Portal.Core/Minecraft/Classes/MinecraftResourceRoots.cs`、`MinecraftFolderEntry.cs`、`WorldSaveInfo.cs`
- `ModDependencyFilter.cs`、`ModpackSniffer.cs`（嗅探整合包类型）
- 世界数据结构齐全：`WorldLevelData` / `WorldPlayerData` / `WorldGameRules` / `WorldEnvironmentSettings` / `WorldScoreboard` —— **它把世界元数据拆成 5 个专门模型**

### 3. 启动服务（`MinecraftLaunchService.cs`，**54.4 KB，最大单文件**）

配合 `MinecraftInstallationTasks.cs`(29.7KB)、`MinecraftResourceCompleter.cs`(6.2KB)、`LaunchCustomization.cs`(9.4KB)、`MinecraftTextParser.cs`(4.7KB)。
→ 体量说明启动链路复杂度高（含文本解析与资源补全）。**只读设计，不抄代码。**

### 4. Preload DLL 的真实机制（**本次最重要的 Portal 发现**）

`Portal.Bedrock.Preload/` 是 **C# 写的原生 DLL**（`unsafe` + `[ModuleInitializer]`），不是 C++。

**`ModuleEntry.cs`**
- `:18-35` `[ModuleInitializer]` 等价 `DllMain(DLL_PROCESS_ATTACH)`
- `:37-69` **`WriteBootMarker`**：*"极简启动标记：**仅用原生句柄写入，不经过托管文件层**（降低 DllMain 期间 loader 锁死锁风险）。用于区分'DLL 未被加载'与'加载后初始化中途失败'"* → **纯原生 `CreateFileW`/`WriteFile`/`CloseHandle` 写 `config/Portal/logs/boot.log`**
- `:71-112` Run 顺序：**切工作目录** → 可选 `AllocConsole` → `FileRedirectHooks.Install`（仅当 `isVersionIsolated || launchInfoEnabled`）→ `NativeExports.LogInjection()` → **另开 `CreateThread` 跑 WorkerThread**（`:114-136`）
- `:138-143` `UseExeDirectoryAsWorkingDirectory` → `SetCurrentDirectoryW(进程目录)`

**`PathRedirector.cs`（141 行）—— 重定向的判定逻辑**

- `:14-22` **6 个关键词**（路径子串匹配）：
  ```
  AppData\Roaming\Minecraft Bedrock
  AppData\Local\Packages\Microsoft.MinecraftUWP_8wekyb3d8bbwe
  AppData\Local\Packages\Microsoft.MinecraftWindowsBeta_8wekyb3d8bbwe
  AppData\Local\Packages\...UWP...\LocalState
  AppData\Local\Packages\...WindowsBeta...\LocalState
  AppData\Roaming\Minecraft Bedrock Preview
  ```
- `:24-27` **排除的顶层目录**（这些不重定向）：`AC / LocalCache / SystemAppData / Settings / TempState / RoamingState`
- `:45-69` `GetRedirectedRelativePath`：找关键词 → 取其后相对路径 → 取第一段判排除 → `EnsureParentDirectory` → 返回相对路径
- `:89-105` `InitializeBaseDirectory` 按 **`folderPolicyString`** 决定根：
  ```
  "shares"                    → <exeDir>/Minecraft Bedrock[ Preview]   （按 versionType 0/2=Preview, 1=Release）
  "independence" 或 ""        → <exeDir>/config/Portal/isolation
  "portal"                    → %APPDATA%/cc.tiouo.Portal/Bedrock
  其他                        → 不重定向
  ```
- `:72-84` 根目录句柄缓存（`NtCreateFile` 的 `RootDirectory` 用）
- `:42` `IsolationFolder = @"config/Portal/isolation"`

**`FileRedirectHooks.cs`（355 行）—— 8 个 ntdll 内联 Detour**

`:40-47` 挂钩的 8 个函数：
```
NtCreateFile / NtOpenFile / NtQueryAttributesFile / NtQueryFullAttributesFile
/ NtSetInformationFile / NtDeleteFile / NtQueryDirectoryFile / NtCreateSection
```
- `:78-80` `Attach` 用 `InlineHook.TryCreate(original, detour, out trunk)`
- `:61-63` 打印 `"Attached: 8"` 或 `"Detour attach incomplete. Attached: n/8"` —— **逐个统计挂接成功率**
- `:90-97` 用 `delegate* unmanaged<...>` 函数指针定义 8 个 detour，签名精确到 `ObjectAttributes*` / `IoStatusBlock*` / `FileInformationClass`
- `:12` `BufferChars = 2048`；`:193` 启动信息语言文件重定向；`:210/241` `TryRedirect(attributes, &patched, &name, buffer, "NtCreateFile", out relative)`
- 另有 `InlineHook.cs`(10.1KB) 自研内联钩子、`NtTypes.cs`(3.4KB)、`NativeMethods.cs`(3.6KB)

→ **机制确认**：不是"启动器传参数让游戏换目录"，而是**在游戏进程内用 ntdll 层 Detour 把 AppData 路径改写到隔离根**。
必须在游戏 exe 导入表里注入这个 DLL（`BedrockDataIsolation.cs` 的 PE 改写），两者配合才成立。

**`Portal.Bedrock.Hook`（另一个 DLL，38 文件）**：Xbox 用户桥接（`XUserBridge`/`XUserToken`/`XUserObject`）+ `WinSock2Hook` + `ModLoader`（`BlHost`/`BlModApiV1` 模组宿主）+ `CrashReporter` + `X64Decoder`。
→ **Hook 是"Xbox 身份 + 网络 + 模组加载"层，Preload 是"数据路径重定向"层，职责分离。**

### 5. 前端（`web/`）—— **它不是桌面 UI**

`web/package.json` 依赖仅 `vue` + `vue-router`；`web/src/components/` 是 `HeroSection` `FeatureSection` `DownloadSection` `OpenSourceSection` `ProtocolSection` `SiteFooter` `SiteHeader` `AurInstallPanel` `QqGroupSection`，
`views/` 是 `HomeView` `InstallView` `LegalView` `MacOsInstallView` + `prerender.mjs`（SSG）。
→ **这是官网/落地页，不是应用界面。**

**桌面 UI 在 `src/Portal/`**：`Assets Classes Module Platform Services Styles ViewModels Views`（Avalonia + XAML）。
- 状态管理：`Portal.Core` 的 `[ObservableProperty]`（CommunityToolkit.Mvvm 源生成器）
- 无前端框架层的"状态管理库"概念

---

## A. 可直接搬的资产（Polymerium / Trident，**MIT**）

> 全部路径以 `repos/Polymerium/submodules/Trident.Net/` 为根。

| # | 文件 | 内容形态 | 搬运方式 |
|---|---|---|---|
| 1 | `src/TridentCore.Abstractions/FileModels/Profile.cs` | `profile.json` 完整 schema + **12 个 override 键常量** + Rule/Selector 结构 | **照抄 schema 与常量值**，转 Rust `serde` 结构体 |
| 2 | `.../FileModels/LockData.cs` | 锁定态 schema：`FORMAT=9`、`PlatformData`、`ArtifactData`、`ArtifactRegion`、`LockedPackage`、`PackageRule`、`AssetData`、`Library(+Identity)` | **照抄**；`IsNative`/`IsPresent` 分离的设计直接采用 |
| 3 | `.../FileModels/PackData.cs` | `data.pack.json`：`OfflineMode`、`ExcludedTags`、`IncludedOverrides` 默认三项、`IncludingSource/Tags` | **照抄**（含 3 个默认 override 键） |
| 4 | `.../FileModels/PatchDocument.cs` | patch 文档 `Format=2`：`Operation{Target,Action,Value,Match(Selector),Before,After,IfMissing,OnlyIfNewer,Rules}` + `PlatformRule(Action,Os,Arch,Version)` | **照抄 schema** |
| 5 | `.../FileModels/PatchArtifact.cs` | `MainClass`、`CompatibleJavaMajors`、`GameArguments`、`JvmArguments`、`DefaultJvmArguments=true`、`Libraries`、`Agents`、`MainJar`、`AssetIndex` | **照抄**——这是"启动清单"的规范形态 |
| 6 | `.../FileModels/PackPatchIndex.cs` | `Format=1` + **只列 import 层**（用户层不打包） | **照抄语义** |
| 7 | `.../FileModels/PatchLibrary.cs` / `PatchAgent.cs` / `PatchIndex.cs` | patch 索引与库/agent 模型 | **照抄** |
| 8 | `src/TridentCore.Abstractions/PathDef.cs` | **完整路径布局 + 5 个文件名常量 + Home 定位策略** | **照抄常量与布局**；"启动时冻结根目录"策略照搬 |
| 9 | `src/TridentCore.Pref/Building/Builder.cs` | `pref://` 构造 + `Scheme="pref"` | **照抄格式规范** |
| 10 | `src/TridentCore.Pref/Parsing/Parser.cs` | pref 解析 + **legacy 正则原文**（`:115`） | **照抄正则**作为兼容测试用例 |
| 11 | `src/TridentCore.Pref/PackageIdentifier.cs` 等 7 个模型 | `PackageIdentifier` / `ProjectIdentifier` / `ScopedPackageIdentifier` / `ScopedProjectIdentifier` / `PackageDescriptor` | **照抄形状**（含 Scoped 变体） |
| 12 | `src/TridentCore.Core/Engines/Deploying/DeploymentPlan.cs` | **7 种 Operation 枚举** + `Download` 记录 | **照抄操作集** |
| 13 | `src/TridentCore.Core/Engines/DeployEngine.cs` | **9 阶段固定管线顺序** | **照抄阶段划分与顺序** |
| 14 | `src/TridentCore.Core/Engines/Deploying/Stages/ExecuteDeploymentStage.cs` | 原子写文件、MoveToPersist、Backport 时间戳校验、allowed_symlinks 写入 | **照抄算法** |
| 15 | `src/TridentCore.Abstractions/Snapshots/{ISnapshotStore,SnapshotInfo,ReferenceInfo}.cs` | 快照接口 + 引用计数 GC | **照抄接口形状与 GC 策略** |
| 16 | `src/TridentCore.Core/Exporters/ModrinthExporter.cs` | **`LOADER_MAPPINGS` 表**（`:19-25`）+ index/overrides 正确分流 | **照抄映射表与分流逻辑** |
| 17 | `src/TridentCore.Core/Services/MigratorAgent.cs` | 迁移框架：adapter 索引 + 指纹还原 + 实例边界取消 + 全或无注册 | **照抄骨架** |
| 18 | `src/TridentCore.Core/Models/**`（**约 90 个 DTO**） | Microsoft / XboxLive / Minecraft / Yggdrasil / AuthlibInjector / CurseForge(15) / Modrinth(9) / ModrinthPack / CurseForgePack / MultiMcPack / PrismLauncherApi / MojangLauncherApi / Mclogs / GitHub | **照抄字段名与 `[JsonPropertyName]`**，转 Rust |
| 19 | `src/TridentCore.Core/Clients/I*.cs`（13 个接口） | 各平台 API 客户端接口形状 | **照抄接口划分** |
| 20 | `src/TridentCore.Core/Repositories/{Modrinth,CurseForge,Packwiz}Repository.cs` | **三个仓库实现**（Packwiz 是额外收获） | 参考实现 |
| 21 | `src/TridentCore.Core/Services/PackageResolver`（在 `Deploying/PackageResolver.cs`） | "一次解析扇出给同键多个条目" | **照抄** |

---

## B. 只能借鉴设计（Portal，**AGPL-3.0**）

| # | 设计 | 路径 |
|---|---|---|
| 1 | **Java/Bedrock 双实例类型 + 共享基类** | `Classes/MinecraftInstance.cs:867,889-893` |
| 2 | **10 种目录布局识别 + 从子目录向上找根** | `Classes/MinecraftFolderLayout.cs:5-17,52-78,80-95` |
| 3 | **多启动器品牌反推**（.BakaXL/PrismLauncher/MultiMC） | `:29-36` |
| 4 | **能力布尔值挂在布局类型上** | `:25-27` |
| 5 | **`folderPolicyString` 四策略**（native/portal/shares/independence） | `Portal.Bedrock.Preload/PathRedirector.cs:89-105` |
| 6 | **6 个路径关键词 + 6 个排除顶层目录** | `:14-27` |
| 7 | **ntdll 8 函数内联 Detour + 逐个统计挂接成功率** | `FileRedirectHooks.cs:40-63` |
| 8 | **DllMain 期引导标记绕过托管文件层** | `ModuleEntry.cs:37-69` |
| 9 | **Hook/Preload 双 DLL 职责分离**（身份网络/数据重定向） | `Portal.Bedrock.Hook/` vs `Portal.Bedrock.Preload/` |
| 10 | **世界元数据拆 5 个专门模型** | `Classes/World*.cs` |
| 11 | `AutoSetChineseLanguage` 默认 true | `MinecraftInstance.cs:879` |
| 12 | 默认窗口 854×480（Minecraft 经典默认） | `:882-883` |

---

## C. 值得抄的机制（15 条）

| # | 机制 | 来源 | 抄到什么程度 |
|---|---|---|---|
| 1 | **9 阶段固定线性部署管线**（无状态机，阶段自决 no-op） | Trident `DeployEngine.cs:22-33` | 全抄阶段划分与顺序 |
| 2 | **7 种部署 Operation + 计划/求差/应用三段分离** | Trident `DeploymentPlan.cs`、`ExecuteDeploymentStage.cs:66-69` 注释 | 全抄 |
| 3 | **原子写：`tmp + Guid + .tmp` → Copy → 保留 mtime → Move** | Trident `ExecuteDeploymentStage.cs:89-99` | 全抄 |
| 4 | **Backport 时间戳乐观校验**（不一致即判被占用） | Trident `:139-140` | 全抄 |
| 5 | **`allowed_symlinks.txt` 白名单**（`[prefix]<dir>` 逐行） | Trident `:162-171` | 全抄格式 |
| 6 | **快照内容寻址 + 引用计数 GC** | Trident `PathDef.cs:90-93`、`ISnapshotStore.cs` | 全抄 |
| 7 | **规则评估结果按包冻结于锁定态**（微调只重算受影响包） | Trident `LockData.cs:87-89` 注释 | 全抄策略 |
| 8 | **`pref://` 完整规范 + legacy 正则** | Trident `Parser.cs:115`、`Builder.cs:22-64` | 全抄 |
| 9 | **迁移框架：指纹还原 + 实例边界取消 + 全或无注册** | Trident `MigratorAgent.cs:51-77,81-87,107-143` | 全抄骨架 |
| 10 | **导出时 index/overrides 正确分流**（避开 Prism #1165） | Trident `ModrinthExporter.cs:48-103` | 全抄 |
| 11 | **90 个 API DTO 的字段与 JSON 名** | Trident `Core/Models/**` | 全抄字段 |
| 12 | **启动时冻结根目录，避免多根/单根漂移** | Trident `PathDef.cs:11-13` | 全抄策略 |
| 13 | **`[ModuleInitializer]` + 原生句柄引导日志**（区分"未加载"与"加载后失败"） | Portal `ModuleEntry.cs:18-69` | 借鉴思路（Rust 侧对应 `ctor`/早日志） |
| 14 | **逐个统计挂钩/附加成功率并上报** | Portal `FileRedirectHooks.cs:61-63` | 借鉴（用于我们的任何注入类操作，包括"权限/能力探测"） |
| 15 | **能力布尔值挂在类型上而非散落判断** | Portal `MinecraftFolderLayout.cs:25-27` | 已与我们的能力描述符一致，确认方向正确 |

---

## D. 明确不适合的

| 不抄 | 来源 | 理由 |
|---|---|---|
| **ntdll 内联 Detour 改 AppData 路径** | Portal `FileRedirectHooks.cs` | ① AGPL，代码不可用；② **触碰"不修改游戏二进制"红线**（须先在游戏 exe 导入表注入 DLL）；③ 杀软/反作弊暴露面极大 |
| **依赖 `CreateSymbolicLink` 而无降级链** | Trident `ExecuteDeploymentStage.cs:158-159` | Windows 需开发者模式；**Axolotl 的 junction 优先策略更实用**（见父 agent 笔记 §5.6） |
| **`patches/` 作为"不走 profile 的外部声明"** | Trident `PackPatchIndex.cs` 等 | 灵活性高但**心智负担重**（两套真源：profile + patches）；我们一期只保留 profile 单真源，patches 概念可作为二期扩展位 |
| **`SourceOrders` 多层叠加** | Trident `Profile.cs:33-35` | 为"整合包套整合包"设计，**我们的用户场景不需要多层**，单 source 即可 |
| **`_bomb_has_been_planted_` 这类调试哨兵文件** | Trident `PathDef.cs:73` | 开发期痕迹，不上线 |
| **Purl 遗留兼容层** | Trident `Profile.cs:48-54` | 我们从零开始，**没有存量数据要兼容**，直接只做 pref |
| **把状态放 SQLite**（对照 Axolotl） | — | Trident 用文件，Axolotl 用 DB；**文件更适合我们**（可版本控制、可手工修） |
| **52 KB 单文件的启动服务** | Portal `MinecraftLaunchService.cs` | 体量说明耦合；我们应拆分为 Provider 内的多个窄模块 |

---

## 三处对父 agent 决策有影响的更正

1. **Portal 的前端不是 Vue 应用**：`web/` 是官网落地页（只有 `vue` + `vue-router`，含 `prerender.mjs` SSG）。**桌面 UI 是 Avalonia（`src/Portal/Views` + `ViewModels`）**。任何"Portal 有 Vue 桌面前端"的说法是错的。
2. **Trident 没有 junction/hardlink 降级链**，只有 `Directory/File.CreateSymbolicLink`（`ExecuteDeploymentStage.cs:158-159`）+ `allowed_symlinks.txt` 白名单。**Windows 免开发者模式场景应采 Axolotl 的 junction 优先策略，而非 Trident 的符号链接方案。**
3. **Trident 的 Modrinth 导出已正确处理 index/overrides 分流**（`ModrinthExporter.cs:48-103`），确认 Prism #1165 那个缺陷**不是普遍问题，是 Prism 的实现缺陷**——我们按 Trident 的分支逻辑写即可。
