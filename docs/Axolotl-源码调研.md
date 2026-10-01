# Axolotl 源码调研（Rust 同栈先例）

> 对象：`repos/Axolotl`（Modrinth 启动器的衍生仓库）。只读代码，未改任何方案文件。
> 本地已克隆，`apps/` 与 `packages/` 双 workspace。**注意：Rust 核心不在 `apps/app`，在 `packages/app-lib`（crate 名 `theseus`，`repos/Axolotl/Cargo.toml:129`）。**

---

## 1. workspace 结构

`Cargo.toml:3-11` 是 **Rust workspace**（7 个成员），`pnpm-workspace.yaml` 另管前端。

| 成员 | 性质 | 说明 |
|---|---|---|
| `apps/app` | **产品（Tauri 壳）** | `Cargo.toml:4`；只有 69 个 rs 文件，纯命令层 + 平台集成 |
| `apps/installer-ui` | 产品（安装器） | `Cargo.toml:5` |
| `packages/app-lib` | **核心库**（crate 名 `theseus`） | `Cargo.toml:6`、`:129`；312 个 rs，**全部业务逻辑在此** |
| `packages/daedalus` | 库 | `Cargo.toml:8`；MC 元数据协议，被 19 个文件引用 |
| `packages/ariadne` | 库 | `Cargo.toml:7`；社交/隧道协议类型 |
| `packages/modrinth-content-management` | 库 | `Cargo.toml:9`；内容安装模型 |
| `packages/path-util` | 库 | `Cargo.toml:10` |

**依赖方向**：`apps/app` → `app-lib`（`theseus`）→ `daedalus` / `ariadne` / `path-util`。
`app-lib` 用 `tauri` **feature 开关**反向依赖 Tauri（`packages/app-lib/Cargo.toml` 的 `[features]`：`cli = ["dep:indicatif"]`、`tauri = ["dep:tauri"]`）——
**同一个核心库可编成 GUI 或 CLI 两种形态**（`src/event/mod.rs:81-83` 用 `#[cfg(feature = "cli")]` 挂 `indicatif` 进度条）。

## 2. 实例模型：**不是文件，是 SQLite 行**

关键结论：**实例元数据不落 JSON 文件，落 SQLite**（`sqlx` 出现在 **81 个文件**）。

- `Instance` 结构：`packages/app-lib/src/state/instances/model/instance.rs:63-107`
  字段含 `id/path/name/icon_path/install_stage/launcher_feature_version/update_channel/game_dir_override/created/modified/last_played/pinned_at/recent_time_played`。
- 持久化：`state/instances/adapters/sqlite/instance_rows.rs`（1900+ 行）
- 迁移：`packages/app-lib/migrations/` **116 个 SQL 迁移**；另有 `backup_migrations/`
- 旧格式转换：`state/legacy_converter.rs`（从 Otherside/旧格式导入，`:340` 用 `LauncherFeatureVersion::None`）

**特性版本机制（值得直接抄）**：`state/instance_types.rs:42-46` 定义**单调递增枚举**

```rust
enum LauncherFeatureVersion { None, MigratedServerLastPlayTime, MigratedLaunchHooks }
const MOST_RECENT: Self = Self::MigratedLaunchHooks;   // :49
```

升级**不是启动时全量迁移，而是懒迁移**：`state/instances/commands/refresh_instances.rs:15-16`
—— 刷新实例列表时，凡 `launcher_feature_version < MOST_RECENT` 的就地升级为最新。**没有独立迁移链，只有"功能版本"标记。**

**外部启动器直连（独有能力）**：`instance.rs:74-100` 有 `linked_launcher`（`hmcl`/`pcl2`/`pcl2_ce`/`generic`）、`linked_dot_minecraft`、`linked_version_json_path` 等字段；
`:109-118` 的 `is_direct_linked()` 判定"实例没有自己的 profile 目录，**文件原地使用，绝不复制或写入**"。

## 3. 部署链：事件驱动的安装运行器

不在单一函数里，而是**分阶段 runner**：`install/runner/{init,lifecycle,pack,content_change,adjunct,upgrade,request}.rs`，共 17 个文件。

- 阶段入口：`install/runner/init.rs:3` `prepare_initial_instance(`
- 状态机字段：`instance_types.rs:4-12` `InstanceInstallStage { Installed, MinecraftInstalling, PackInstalled, PackInstalling, NotInstalled }`
- 事件：`install/events.rs` 把阶段进度发到前端
- 恢复与诊断：`install/recovery.rs`、`install/diagnostics.rs`、`install/missing_content.rs`
- **导入计划（声明式雏形）**：`install/import_plan.rs`；另 `api/pack/import/instance_json.rs`

**判定**：有"计划 + 分阶段执行 + 可恢复"，但**没有 Polymerium 那种 `profile.json` 单一声明式真源**；实例状态主要在 DB，部署产物在磁盘。

## 4. 加载器：**元数据 + 安装器双轨**，覆盖面远超四家

`api/loader_metadata.rs`（91 KB，含大量测试）是集中实现。

| 加载器 | 证据 |
|---|---|
| Fabric / LegacyFabric / **Babric** | `:365` `fetch_fabric_manifest`、`:403` 按游戏版、`:504` legacy、`:531` babric |
| Quilt | `:431` `fetch_quilt_manifest` |
| Forge | `:910` `fetch_forge_manifest`、`:988` `add_forge_loader_version`、`:1007` 拼 `forge-<v>-installer.jar` |
| NeoForge（新+legacy） | `:1015`、`:1046`、`:1068` legacy 回落到 forge maven |
| OptiFine / LiteLoader / Cleanroom | `:596`、`:646`、`:729` |
| **回落到 Prism meta API** | `:295` `fetch_fallback_manifest`、`:328` `validate_scoped_manifest`、`:831` `meta_api_manifest_for_game` |

**安装器流程（我们 M7 的关键）**：`:1701` `resolve_installer_profile` → `:1768` `parse_installer`
—— 读 installer jar 内的 `version.json` / `profile json` / `versionInfo`（`:1794`），
处理 `inheritsFrom`（`:1803`、`:1820`），**合并内嵌产物**（`:1862-1974`，含 `data` 占位符与 `com.axolotl.loader-installer:embedded:` 伪坐标 `:1890`），
产物缓存与按需补拉：`:1723` `ensure_installer_artifacts`、`:2007` `extract_installer_artifacts`、`:2044` `persist_installer_artifacts`。
测试覆盖极全：`:2419` 解析器合并、`:2600` 旧版 profile 路径、`:2615` 旧内联 versionInfo、`:2631` legacy universal artifact。

## 5. 下载与文件层

- 层分布：`util/fetch.rs`（**324 KB**，主力）、`util/downloads.rs`、`util/download\n2_download.rs`（44 KB，HTTP/2 专项）、`util/download_dns.rs`（20 KB）、`util/network.rs`
- **限速感知的并发/测速**：`util/download_manager.rs:6-8` 常量 `INITIAL_SPEED_FLOOR = 256 * 1024`、`SAMPLE_INTERVAL_MS = 100`、`SAMPLE_HISTORY = 30`；
  `:57-107` 滑动窗口**加权平均**（越新权重越大）并维护**自适应速度地板**（`:90-91` 取近 10 样本均值 ×85%，且只增不减）→ 用于"某源是否慢到该切"的判定。
  **注意：它的地板也是 256 KB，与我们既有策略一致。**
- 校验：`sha1_smol` 用于 15 个文件
- **文件去重/链接**：`util/symlink.rs:136` `create_link_blocking` 分级尝试
  → Windows 目录先 `junction::create`（`:154`）、失败退 `symlink_rs::symlink_dir`（`:160`）、文件用 `symlink_file`（`:164`）；
  Unix 走 `symlink_dir`（`:95`）。
  **另有提权路径**：`:177` `create_link_elevated` / `:193` / `:274` `create_link_elevated_helper`
  —— 即在**用户未开开发者模式**时，用一次提权来建链接。
- **降级链实例**：`api/instance/synced_options/files.rs:142-145`
  ```rust
  if tokio::fs::symlink_file(source, target).await.is_ok() { ... }
  if tokio::fs::hard_link(source, target).await.is_ok() { ... }
  ```
  以及 `api/pack/install_mrpack.rs:110`、`state/instances/commands/apply_content_install.rs:1802` 用 `hard_link` 做**包文件去重**。

## 6. 认证

- 实现：`state/minecraft_auth.rs`（1930 行）
- Microsoft：**设备代码流**（`:95` `device_code`、`:192-198` 构造 `grant_type=urn:ietf:params:oauth:grant-type:device_code`）+ 刷新（`:1382` `oauth_refresh`）
- Xbox Live：`:1436-1444`，`"RpsTicket": format!("d={access_token}")`（**与官方教程一致**）
- 授权校验：`:278` `minecraft_entitlements(...)`
- **外置登录（Yggdrasil / authlib-injector）**：`:508-517` 存 `yggdrasil_api_root` / `server_name` / `login` / `client_token`；`:550` 刷新分流到 `refresh_yggdrasil_credentials`
  —— **国内皮肤站生态必需，我们计划里有，它有现成实现可对照**
- **令牌存储（重要）**：`sqlx::query_as::<_, StoredCredentials>`（`:836-840`、`:878-882`）→ **access_token / refresh_token 明文存 SQLite**。
  `keyring` crate **只用于 AI 功能**（`api/ai.rs:11、:1317`），**不用于 MC 令牌**。
- **令牌会序列化给前端**：`impl Serialize for Credentials`（`:1015-1055`）显式 `serialize_field("access_token", ...)`。
- 离线账户哨兵值：`access_token = "0"`（`:488`）；用户名校验**接受中文**（`:1095-1099` 测试 `玩家`、`玩家_123` 通过，空格与 `!` 拒绝）。
- 其它账户类型：`state/mr_auth.rs`（Modrinth 账号）、`:106` `Complete { credentials }`

## 7. 跨语言接口（**对我们最重要**）

三层机制，都值得抄：

1. **Tauri command 作为唯一 IPC**：`apps/app/src/api/*.rs` 按域分文件（`instance.rs`、`auth.rs`、`jre.rs`、`logs.rs`、`worlds.rs`…），
   每文件自注册 `tauri::generate_handler![...]` 并 `.plugin("xxx")`。
   `apps/app/src/main.rs:937` 汇总。
2. **类型自动生成**：`build.rs:182-184`
   ```rust
   tauri_build::try_build(tauri_build::Attributes::new()
       .codegen(tauri_build::CodegenContext::new())
       .plugin("auth", InlinedPlugin::new().commands(&[...])))
   ```
   —— 用 Tauri 官方 **CodegenContext** 生成 TS 绑定（`build.rs:179-181` 注释解释了为何用手写清单而非解析 `#[tauri::command]`：Tauri issue #10075）。
3. **手写清单 + 一致性测试**（**精髓**）：命令名在 `build.rs` 的 `.commands(&[...])` 里再列一遍，然后用测试强制两边一致：
   `apps/app/tests/instance_command_manifest_parity.rs:24-41` 分别解析 `generate_handler![` 与 `.plugin("instance", ...).commands(&[`，
   `:44-53` 断言 `runtime.difference(manifest)` 为空，`:54-66` 还点名断言两个必须存在的命令。
   **这是"防止 Rust 命令与权限清单漂移"的廉价手段，强烈建议照抄。**

**前端侧**：`apps/app-frontend/src` 有 **0 处** `from "@tauri-apps/api/core"`；
调用统一收在 `src/helpers/*.ts`（`invoke(` 出现在 `helpers/instance.ts:51`、`helpers/ai.ts:85`、`helpers/datapacks.ts:25`、`helpers/drop.ts:4` 等）
—— **前端不散落 invoke，全部经 helpers 层**，与我们"前端零逻辑、只走命令层"同构。

## 8. 前端

`apps/app-frontend/package.json`：**Vue 3**（`vue ^3.5.42:52`）、**Pinia**（`:48`）、**vue-router**（`:54`）、
**Tailwind v3**（`:72`）、**Vite**（`:74`，`vite ^8.2.2`）、`dayjs`、`@tauri-apps/api ^2.11.1`。
⚠️ `CLAUDE.md` 自称 "Vue 3 / Nuxt 3"，但 `package.json` 的 scripts 是 `vite`、**没有 Nuxt 依赖** —— **文档与代码不一致，以代码为准**。

结构：`src/{announcements,components,composables,data,helpers,lab,locales,pages,plugins,providers,routes,store}`；
Pinia store 很薄（`store/` 仅 7 个文件：`state.js`、`error.js`、`theme.ts`、`breadcrumbs.js`…）——
**重状态不在 Pinia，而在 Rust 与 `providers/`**。
样式纪律见其 CLAUDE.md：**强制 Tailwind utility，禁止只包一两声明的自定义类**。

## 9. 内容管理

- Modrinth 为一级公民：`packages/api-client/src/modules/labrinth/`（Labrinth 是 Modrinth 后端）与 `state/labrinth/`
- **CurseForge 支持量惊人**：`api/curseforge.rs` **436 KB**，另有 `curseforge_download/mapping/metrics/validation`
- 内容安装模型：`packages/modrinth-content-management`
- 依赖与冲突：`state/instances/model/instance_upgrade_plan.rs:23-38`
  `InstanceUpgradeItemStatus` 枚举把冲突**分类**为
  `DependencyConflict / MissingRequiredDependency / IncompatibleDependency / UnsupportedContentType / NoCompatibleShaderRuntime / PrereleaseOnly / Unidentified`
  —— **不是"有冲突/无冲突"二元，而是可解释的分类**；`:42-46` `InstanceUpgradeAction { Upgrade, Keep, Disable }`
- 包引用：**没有 `pref://` 之类自定义 URI**，用项目 id + 版本 id + provider 三元组
- 世界/存档：`api/instance/backup.rs`（含 `InstanceBackupProgressStage { Scanning, Hashing, Saving, Validating, Copying, Restoring, Deleting }`，`event/mod.rs:282-295`）

## 10. 事件目录（**灵动岛的直接素材**）

`event/mod.rs`（485 行）是一份完整的进度/状态事件表：

- `LoadingBarType`（`:138-183`）**12 种**：`JavaDownload`、`PackFileDownload`、`PackDownload`、`MinecraftDownload`、`InstanceUpdate`、`ZipExtract`、`LegacyDataMigration`、`DirectoryMove`、`CopyInstance`、`LauncherUpdate`…
- `LoadingPayload`（`:187-192`）：`fraction: Option<f64>` + **`None` 约定表示完成**
- `ProcessPayload`（`:330-343`）+ `ProcessPayloadType { Launched, Finished }` + `crashed: Option<bool>`
- `InstancePayloadType`（`:407-434`）：`Created/Synced/ContentChanged{revision}/ServersUpdated/ScreenshotsUpdated/WorldUpdated/ServerJoined/Edited/ContentInstallFinished/Removed`
- `ServerPayloadType`（`:372-395`）含 **`ExitReason::Eula`**（`:364-368`）：**从控制台输出尾部推断退出原因**，让 UI 主动弹 EULA 对话框而不是只报"进程死了"
- 进度条 id 用 `Drop` 清理（`:90-133`）—— RAII 式生命周期，避免泄漏

## 11. 可借鉴 vs 不适合

### 值得抄（机制）
1. **命令清单一致性测试**（`apps/app/tests/instance_command_manifest_parity.rs`）
2. **Tauri 官方 CodegenContext 生成 TS 类型**（`apps/app/build.rs:182-184`）
3. **feature 开关让同一核心编出 GUI/CLI**（`app-lib/Cargo.toml` `[features]`；`event/mod.rs:81`）
4. **`LauncherFeatureVersion` 懒迁移**（`instance_types.rs:42-49` + `refresh_instances.rs:15-16`）
5. **安装器 jar 解析全流程**（`loader_metadata.rs:1701/1768/2007/2044`）—— 含 `inheritsFrom`、内嵌产物合并、按需补拉
6. **速度滑动窗口 + 自适应地板**（`download_manager.rs:57-107`）—— 比固定窗口更适合我们 0.28 MB/s 的波动链路
7. **链接分级降级 + 提权兜底**（`symlink.rs:136/154/160/164`、`:177` 提权）
8. **冲突分类枚举而非布尔**（`instance_upgrade_plan.rs:23-38`）
9. **`fraction: None` 表示完成** 的单一进度模型（`event/mod.rs:190`）
10. **退出原因推断**（`event/mod.rs:364-368` 的 `ExitReason::Eula`）
11. **经验证的元数据回落到 Prism meta API**（`loader_metadata.rs:295/831`）
12. **前端 invoke 全收在 helpers 层**（`apps/app-frontend/src/helpers/`）

### 不适合我们
| 项 | 理由 |
|---|---|
| **令牌明文存 SQLite** | `minecraft_auth.rs:836-840` 明文存 access/refresh token。我们已定 DPAPI/系统凭据保护，**不可退让** |
| **令牌序列化给前端** | `:1015-1055` 把 `access_token`/`refresh_token` 发进 WebView。**我们应只发"是否已登录 + 账号显示名"** |
| **实例状态全在 SQLite** | 116 个迁移 + 81 文件的 sqlx 耦合。与我们会话级"声明式 profile 文件"取向不同；SQLite 只适合做**缓存/索引**，不宜做真源 |
| **`loading_bar_uuid` 语义靠约定** | `event/mod.rs:72-73` 注释自陈"不可直接用，可能不是最新状态"—— 弱类型约定；我们应用 `reason` 式显式字段 |
| **Tailwind 重度使用** | 与其自身 CLAUDE.md 一致，但 utility 类会掩盖"材质层次"语义（我们已决意用令牌 + CSS Modules） |
| **无 `pref://` 式稳定包引用** | 仅 id 三元组，跨实例复用与离线复原弱于 Polymerium |

---

## 最值得抄的 12 条（按对我们 M1/M6/M7/M8 的价值排序）

| # | 抄什么 | 程度 | 价值点 |
|---|---|---|---|
| 1 | **命令清单一致性测试**：解析 `generate_handler!` 与权限清单，断言集合相等 | 照抄机制，改成我们的命令命名 | **M1** |
| 2 | **`LauncherFeatureVersion` 懒迁移**：单调枚举 + `MOST_RECENT` + 刷新时就地升级 | 照抄，补上我们的"迁移前备份 + 失败不阻断" | **M1** |
| 3 | **安装器 jar 解析**：`version.json`/`profile`/`versionInfo` 三形态 + `inheritsFrom` + 内嵌产物合并 | 照抄流程与测试用例集 | **M7** |
| 4 | **速度滑动窗口 + 自适应地板** | 照抄，用于镜像源选择与 ETA 分段 | **M1 / M2** |
| 5 | **事件目录集中定义**（进度 12 类 + 进程 Launched/Finished + crash 标志） | 照抄结构，作为灵动岛事件表的底稿 | **M1 / M4** |
| 6 | **`fraction: None` = 完成**的单一进度模型 | 照抄 | **M1** |
| 7 | **feature 开关让核心同时编出 GUI 与 CLI** | 照抄，直接支撑我们的 CLI 目标 | **M1** |
| 8 | **冲突分类枚举**（7 类，含 shader runtime 类） | 照抄分类，作依赖图的状态标签 | **M8** |
| 9 | **链接分级降级 + 提权兜底** | 采纳降级链与 junction 首选；**提权路径改为"引导用户开开发者模式"** | **M6** |
| 10 | **Tauri CodegenContext 生成 TS 类型** | 照抄 | **M1** |
| 11 | **元数据回落到 Prism meta API** | 作为我们的第二元数据源 | **M2 / M7** |
| 12 | **退出原因推断**（如从日志尾部判 EULA） | 抄思路，用于崩溃归因 | **M11** |

---

**一句话总评**：Axolotl 是**"工程完备度"的最高参照**（116 迁移、436 KB CurseForge 客户端、安装器全兼容、命令清单测试），
但**架构取向与我们有本质分歧**——它把状态放 DB、令牌放明文并下发前端；我们把状态放声明式文件、令牌留在内核。
**抄它的机制与测试密度，不抄它的信任边界。**
