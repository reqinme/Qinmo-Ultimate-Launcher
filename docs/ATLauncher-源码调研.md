# ATLauncher 源码级调研

> 只读代码，未改动任何方案/计划文件。所有结论带 `路径:行号`。
> 仓库：`repos/ATLauncher`（Java 21 / Swing + FlatLaf / GPL-3.0 / 545 个 .java / 单模块 Gradle + `legacy-launch` 子模块）

---

## 1. 工程与分层

- **单模块 Gradle**（`build.gradle` 一个 + `legacy-launch/build.gradle`），非多模块。
- 包分布（`src/main/java/com/atlauncher`）：**`data` 268 文件**（模型+格式适配，绝对主体）、`gui` 107、`utils` 34、`viewmodel` 21、`managers` 16、`network` 12、`evnt` 13。
- **`data` 包内部按"平台/来源"再分**：`minecraft`(38)、`json`(24)、`ftb`(22)、`curseforge`(19)、`modrinth`(16)、`microsoft`(15)、`minecraft/loaders/{forge(13),legacyfabric(9),quilt(9),fabric(9),neoforge(6)}`、`technic`(8)、`multimc`(6)。
- **依赖方向**：`gui` → `viewmodel` → `managers` → `data`；`data` 是纯模型层。**没有架构约束测试**。
- **`viewmodel` 用了 RxJava3 + rxswing**（`build.gradle`：`rxjava:3.1.10`、`rxswing`）——**Swing 上的 MVVM**，很少见。
- 网络层走 **Apollo GraphQL**（`apollo-runtime:2.5.14`），有独立 `src/main/graphql/`（24 个 `.graphql` + **`schema.json` 274.5 KB**）。

---

## 2. 实例模型

- 核心类 **`data/Instance.java`（156.6 KB！）** —— 单类承载全部实例逻辑，**反面教材：不该学它的规模**。
- **配置版本化/迁移：有，且做法值得记**：`data/json/Version.java` 描述包版本；`Settings.java`(22 KB) 持启动器设置；另有 `PackVersionTypeAdapter.java`(3 KB)、`AccountTypeAdapter.java`(2.1 KB)、`DateTypeAdapter.java`(3.2 KB)、`InstantTypeAdapter.java`(1.8 KB)、`ColorTypeAdapter.java`(2.2 KB)。
  → **"每种需要序列化的复杂类型配一个 TypeAdapter"** 是它处理格式演进的方式（比集中式迁移函数更分散，但每个 adapter 独立演进）。
- **`data/minecraft/JavaVersion.java`、`JavaRuntimeVersion.java`** —— Java 版本信息模型。

---

## 3. 整合包系统（招牌）：**`data/json/Mod.java`（39.2 KB）是核心 schema**

> **这是本次最值钱的"可直接用资产"**：ATLauncher 的包 mod 清单 schema，字段可逐条映射到 Rust `serde` 结构。

**`data/json/Mod.java:44-100` 字段全集**：

| 类别 | 字段（行号） |
|---|---|
| 标识 | `name`(45) `version`(46) `file`(48) `path`(49) `filePrefix`(88) |
| 来源 | `url`(47) `download`(53, `DownloadType`) `website`(54) `donation`(55) `authors`(56) |
| **哈希（三种）** | `md5`(50) `sha1`(57) `sha512`(58) `filesize`(51) `fingerprint`(52, `Long`) |
| 呈现 | `colour`(59) `warning`(60) `description`(89) `group`(85) `linked`(86) |
| 安装行为 | `force`(61) `type`(63, `ModType`) `extractTo`(64) `extractFolder`(65) `decompFile`(66) `decompType`(67, `DecompType`) |
| **文件模式匹配** | `filePattern`(68) `filePreference`(69) `fileCheck`(70) |
| **客户端/服务端分离** | `client`(71) `server`(72) `serverSeparate`(73) `serverUrl`(74) `serverFile`(75) `serverType`(76) `serverDownload`(77) `serverMD5`(78) `serverOptional`(79) |
| 可选性 | `optional`(80) `selected`(81) `recommended`(82) `hidden`(83) `library`(84) |
| **依赖** | `depends`(87, `List<String>`) |
| 平台引用 | `curseForgeProject`(90) `curseForgeFile`(91) `modrinthProject`(92) `modrinthVersion`(93) |
| 兼容字段 | `curseForgeProjectId`(97) / `curseForgeFileId`(99)，**带 `@SerializedName(value=..., alternate={...})` 兼容旧名 `curse_id`/`curse_file_id`** |
| 容错 | `ignoreFailures`(94) |

**同包其余 schema 文件**：`Version.java`(11)、`Action.java`(4.2)、`Loader.java`(3.2)、`Messages.java`(2.7)、`Library.java`(2.5)、`Keep.java`/`Deletes.java`/`Keeps.java`、`MainClass.java`、`ExtraArguments.java`、`Configs.java`、枚举 `ActionType`(0.9) `DownloadType`(0.8) `ModType`(1.1) `ExtractToType`(0.8) `DecompType`(0.9) `CaseType`(0.9) `TheAction`(0.9) `ActionAfter`(0.9)。

**四种第三方包格式的适配模型齐全**（可直接对照我们 M8 的格式面）：
`ftb/`(22)、`technic/`(8)、`curseforge/pack/`(4)、`modrinth/pack/`(2)、`multimc/`(6)。

**校验相关**：`data/CheckState.java`(1.5)、`data/DownloadableFile.java`(2.5)。

---

## 4. 下载与文件层

- 网络：**OkHttp**（`okhttp:4.12.0` + `okhttp-tls`），有 `network/DebugLoggingInterceptor.java`。
- **哈希：`com.sangupta:murmur:1.0.0`** —— **MurmurHash3 32-bit，用于 CurseForge mod 指纹**（`Mod.fingerprint`）。**这是可直接用的算法**：CurseForge 用 Murmur3 指纹识别 mod，Rust 侧有成熟 crate。
- 压缩：`org.apache.commons:commons-compress:1.27.1` + `org.zeroturnaround:zt-zip:1.17` + `org.tukaani:xz:1.10`（xz 支持）。
- **CDN 与分域**：`constants/Constants.java:85-92` —— `download.nodecdn.net/containers/atl` 为包 CDN，`DOWNLOAD_SERVER`/`DOWNLOAD_HOST` 分离。
- **镜像/多源：本次未找到"多源切换/测速"机制**（与 HMCL/Axolotl 不同，ATLauncher 走自有 CDN + 直连）。
- 续传：**未在多处见到显式 Range 续传逻辑**（与 LeviLauncher/HMCL 差距明显）。

---

## 5. 加载器

`data/minecraft/loaders/` 下：**forge(13 文件)、neoforge(6)、fabric(9)、quilt(9)、legacyfabric(9)、paper(1)、purpur(1)**。
- 每个加载器有独立 `Version.java`（如 `loaders/forge/Version.java`(1.3)、`loaders/neoforge/Version.java`(1.3)）与 meta 版本模型（`FabricMetaVersion.java`、`QuiltMetaVersion.java`、`LegacyFabricMetaVersion.java`）。
- **加载器元数据走 GraphQL**：`src/main/graphql/com/atlauncher/Get{Forge,Fabric,Quilt,NeoForge,LegacyFabric,Paper,Purpur}LoaderVersion[sForMinecraftVersion].graphql` + `GetLoaderVersionsForMinecraftVersion.graphql`。
  → **用 GraphQL 统一拉多家加载器版本**，是它独有的做法（我们可选 REST，但"统一一个查询入口"的思路值得记）。
- `legacy-launch/` 子模块 = 老版本启动支持（独立 Gradle 工程）。

---

## 6. Java 检测与选择

- 模型：`data/minecraft/JavaVersion.java`、`JavaRuntimeVersion.java`（各 0.9 KB）——**比我预期的薄**。
- **没有看到 HMCL 那种"游戏版本范围 × Java 版本范围"的约束枚举表**；它的 Java 选择偏"实例级设置 + 运行时覆盖"。
- 有 `UpdateBundledJre.java`（顶层）—— 随包 JRE 更新。
- **结论**：Java 选择这块 **HMCL 明显更强**，ATLauncher 不值得参考。

---

## 7. Servers 页 与 Console 页

### ⚠️ 重要纠错：ATLauncher 的 "Servers" 不是"服务器列表"

`data/Server.java`（58.3 KB）的字段是 `minecraftVersion`(115) `version`(116) `hash`(117) `loaderVersion`(120) `javaVersion`(122) `mods: List<DisableableMod>`(125) `isPatchedForLog4Shell`(118) `isDev`(124) `ROOT: Path`(134)……
→ **它是"用启动器开一个 Minecraft 服务端"的实例模型**，不是"服务器列表/MOTD/延迟"。
**文件整理笔记里"ATLauncher 有 Servers 页可参考"的假设不成立**，请勿据此设计服务器列表功能。

### Console 页（**这个值得抄**）

组成：`gui/LauncherConsole.java`(6.3)、`gui/components/Console.java`(3.9)、`gui/components/ConsoleBottomBar.java`(7)、`data/ConsoleState.java`(枚举 `OPEN|CLOSED`)、`evnt/LogEvent.java`(3.6)。

**底部操作栏四个按钮**（`ConsoleBottomBar.java:48-51`）：
`Clear`、**`Copy Log`**、**`Upload Log`**、**`Kill Minecraft`**；
`:134` `Kill Minecraft` 有**二次确认弹窗**（`DialogManager.yesNoDialog()`）。

**日志上传目标**（`Constants.java:77-81`）：**ATLauncher 自建 paste 服务** —— `paste.atlauncher.com`，API `PASTE_API_URL = .../api/create-v2`，另有 `PASTE_CHECK_URL` 预检。
→ **它是自有服务**，我们没有；对照项是 mclo.gs 这类公共 paste。

**日志分级**（`evnt/LogEvent.java:94-113`）：

```java
public enum LogType { INFO, WARN, ERROR, DEBUG;
    public Color color() {  // 颜色来自 UIManager 主题令牌
        INFO  -> Console.LogType.info
        WARN  -> Console.LogType.warn
        ERROR -> Console.LogType.error
        DEBUG -> Console.LogType.debug
        default -> Console.LogType.default
    }
}
```

另有**来源标记**（`LogEvent.java:33-37`）：`CONSOLE = 0xA`、`LOG4J = 0xB`，作为 `meta` 字段 —— **"这行日志来自我们自己的输出，还是来自游戏进程的日志"是可区分的**。这是个好设计（诊断时能分清"启动器的错"与"游戏的错"）。

**主题里的日志色令牌**（`themes/ATLauncherLaf.properties:112-117`）：

```properties
Console.fontSize={float}12
Console.LogType.debug=#9f7aea
Console.LogType.error=$red
Console.LogType.info=$primary.500
Console.LogType.warn=$yellow
Console.LogType.default=@foreground
```

→ **五档日志色（含 fontSize）作为设计令牌**，且各主题复用它（`CatppuccinLatte.properties:92-96`、`OneDark.properties:50-54`、`CatppuccinMacchiato.properties:92-96`）。

**日志清理工具**：`gui/tabs/tools/LogClearerToolPanel.java`（1.8 KB）—— 日志清理是一个**独立工具项**，呼应我们百宝箱。

**未见命令输入框**（ConsoleBottomBar 只有四个按钮，无 JTextField 发命令）→ **它不支持往游戏 stdin 发指令**。

---

## 8. 认证

- 模型：`data/MicrosoftAccount.java`(11.6)、`AbstractAccount.java`(10.1)、`Account.java`(3.5)、`AccountTypeAdapter.java`(2.1)；流程 UI `gui/dialogs/LoginWithMicrosoftDialog.java`(19.4)。
- **本次未找到令牌加密存储的明确实现**（`data/microsoft/` 下只有 `LoginResponse.java`），也未见 authlib-injector 外置登录的完整实现痕迹——**这块不如 HMCL/Axolotl 可参考**。
- ⚠️ **反面案例（安全）**：`constants/Constants.java:104` **把 CurseForge API Key 硬编码提交进仓库**：
  ```java
  public static final String CURSEFORGE_CORE_API_KEY = "$2a$10$.7CSxLm/lnj5lCBSM5jGQ.3SICSX4j9r661AgoB1Rc4Nw8jCMKcv2";
  ```
  配合 `:101 CURSEFORGE_CORE_API_URL`、`:105 CURSEFORGE_API_KEY_HEADER = "x-api-key"`。
  → 这正是我们 §5.8 安全基线（不硬编码密钥）要禁止的。**可作反例测试用例。（若当年可发布的 key，也说明这类 key 会被全网提取。）**
- 有用的 ID 常量表（`Constants.java:108-114`）：CurseForge 各加载器 ID —— `FORGE=1`、`FABRIC=4`、`QUILT=5`、`NEOFORGE=6`；mod ID `FABRIC_MOD_ID=306612`、`LEGACY_FABRIC_MOD_ID=400281`；`CURSEFORGE_PAGINATION_SIZE=20`。

---

## 9. 错误与日志

- **崩溃归因做得实在**：`data/MinecraftError.java` —— 五个错误常量（`:27-31`）：
  `OUT_OF_MEMORY=1`、`CONCURRENT_MODIFICATION_ERROR_1_6=2`、`USING_NEWER_JAVA_THAN_8=3`、`NEED_TO_USE_JAVA_16_OR_NEWER=4`、`NEED_TO_USE_JAVA_17_OR_NEWER=5`。
- **每个错误都有"人话 + 可执行动作"**：
  - `:53` 内存不足 → *"go to the settings tab and increase the maximum memory option"*（**指到具体设置页**）；
  - `:69` Java 太新 → 弹窗带 **"Download Java 8" 按钮**（`:74`）→ `:77 OS.openWebBrowser("https://atl.pw/java8download")`（**按钮直连下载**）；
  - `:81`/`:90` Java 版本过低 → 文案里**逐条给排查路径**（检查 Java 选择、检查 "Use Java Provided By Minecraft" 开关、Runtime Override 在哪）。
  → **这正是我们 §5.7"错误严重级别 + 可执行建议"的产品级范例**，文案可直接翻译复用（GPL-3.0，须遵守）。
- i18n：**`org.mini2Dx.gettext` + `.po` 文件**，30 个 locale 在 `Language.java:44-89` 静态注册；
  `Language.java:75-88` 另维护 **`localesWithoutFont` / `localesWithoutTabFont`**（阿拉伯语、中文、希伯来语、日语、韩语、希腊语）——**字体回退按语言白名单处理**，是个细节好设计。
  ⚠️ 但 **`src/main/resources/assets/lang/*.po` 在本仓库不存在**（Crowdin 拉取，`Constants.java:61 CROWDIN_URL`）→ **`.po` 文件不在克隆里，拿不到译文**。
- 异常：`exceptions/` 5 个类 + 顶层 `ExceptionStrainer.java`；日志走 log4j2（`log4j-api/core:2.24.3`），`managers/LogManager.java`(10.2)。

---

# A. 可直接用的资产

| # | 资产 | 位置 | 怎么用 |
|---|---|---|---|
| **A1** | **FlatLaf 设计令牌表（含 3 条色彩阶）** | `themes/ATLauncherLaf.properties:27-62` | `primary.100–900`、`secondary.100–900`、`gray.100–900` + `red`/`yellow`/`green`/`black`/`white`/`transparent`。**直接映射成我们的 CSS 变量阶**（我们的 §4.5 令牌系统正缺这种成阶的中间色） |
| **A2** | **组件级令牌别名写法** | 同上 `:67-91` | `Button.background=$buttonBackground`、`BottomBar.dividerColor=$Separator.foreground`、`CheckBox.icon.borderColor=$border`——**"语义令牌引用调色板"的两层结构**，可直接照搬命名风格 |
| **A3** | **五档日志色令牌 + fontSize** | 同上 `:112-117`；复用见 `CatppuccinLatte.properties:92-96`、`OneDark.properties:50-54` | `Console.LogType.{debug,error,info,warn,default}` + `Console.fontSize`。**我们日志抽屉/Console 页的令牌直接采用这五个语义名** |
| **A4** | **`Mod.java` 包 mod schema（含 20+ 字段与兼容别名）** | `data/json/Mod.java:44-100` | **逐字段转 Rust `serde` 结构**，用于解析 ATLauncher 格式整合包（M8 格式面 +1）。`@SerializedName(alternate=...)` 对应 `#[serde(alias="...")]` |
| **A5** | **CurseForge 平台常量表** | `Constants.java:101-114` | API URL、`x-api-key` 头名、CDN host、**加载器 ID（1/4/5/6）**、mod ID（306612/400281）、分页 20。全部是稳定事实，可直接抄 |
| **A6** | **版本比较算法三件套** | `utils/Utils.java:1396/1407/1428/1441` | `compareVersions`（纯数字段比较）、`matchVersion(version, matches, lessThan, equal)`、`matchWholeVersion`、`getNumericVersionParts`（按 `.` 切分，**含 `_` 的版本直接返回 false**——旧快照命名）。可直接移植为 Rust 函数 + 单测 |
| **A7** | **MurmurHash3 32-bit 指纹** | `build.gradle` 依赖 `com.sangupta:murmur`；字段 `Mod.java:52 fingerprint` | CurseForge mod 指纹识别算法，Rust 侧有 crate 可直接实现 |
| **A8** | **崩溃错误文案表（5 条，含可执行动作）** | `data/MinecraftError.java:53-97` | 中文改写后可直接用作我们 M11 的崩溃提示文案；尤其 `:69-79` 的"按钮 → 直接打开 Java 8 下载"模式 |
| **A9** | **GraphQL schema（274.5 KB）** | `src/main/graphql/com/atlauncher/schema.json` + 24 个 `.graphql` | 完整后端 API 契约。**含"按 MC 版本查各加载器版本"的查询形状**（`Get{Forge,Fabric,Quilt,NeoForge}LoaderVersionsForMinecraftVersion.graphql`）——**可直接照它的查询字段设计我们的加载器元数据接口** |
| **A10** | **三方包格式模型（5 种）** | `data/{ftb,technic,curseforge/pack,modrinth/pack,multimc}/` | FTB / Technic / CurseForge / Modrinth / MultiMC 的清单字段模型，M8 格式面可逐一对照 |
| **A11** | **30 个 locale 的注册表 + 字体回退清单** | `Language.java:44-89` | locale 列表与 `localesWithoutFont`/`localesWithoutTabFont` 白名单，可作我们多语言覆盖与字体回退的起点 |

---

# B. 值得抄的机制（按价值排序）

| # | 机制 | 证据 |
|---|---|---|
| **1** | **日志来源标记**：`meta` 字段区分 `CONSOLE(0xA)` / `LOG4J(0xB)`——**能分清"启动器输出"与"游戏输出"** | `evnt/LogEvent.java:33-37` |
| **2** | **Console 底部四操作**：Clear / Copy Log / Upload Log / **Kill Minecraft（带二次确认）** | `ConsoleBottomBar.java:48-51, 134` |
| **3** | **日志色是主题令牌**（不是硬编码），各主题复用 | `LogEvent.java:97-113` + `ATLauncherLaf.properties:112-117` |
| **4** | **崩溃错误 = 人话 + 指到具体设置页 + 一键动作** | `MinecraftError.java:53-97` |
| **5** | **每种复杂序列化类型配独立 TypeAdapter** | `data/{Color,Date,Instant,Account,PackVersion}TypeAdapter.java` |
| **6** | **兼容旧字段名**（`@SerializedName(alternate=...)`） | `Mod.java:96-100` |
| **7** | **GraphQL 统一拉多家加载器版本** | `src/main/graphql/*LoaderVersion*.graphql` |
| **8** | **字体回退按语言白名单**（而非全局换字体） | `Language.java:75-88` |
| **9** | **日志清理是百宝箱里的独立工具项** | `gui/tabs/tools/LogClearerToolPanel.java` |
| **10** | **客户端/服务端分离的包字段**（`client`/`server`/`serverSeparate`/`serverUrl`/`serverMD5`） | `Mod.java:71-79` |
| **11** | **可选性三元组**：`optional` / `recommended` / `selected` / `hidden`——安装器能表达"推荐但可选" | `Mod.java:80-83` |
| **12** | **包内保留/删除规则表**（`Keep`/`Keeps`/`Delete`/`Deletes`）——声明式控制更新时什么该留 | `data/json/{Keep,Keeps,Delete,Deletes}.java` |

---

# C. 明确不适合的

| 不适合 | 理由 |
|---|---|
| **`Instance.java` 156.6 KB 单类** | 上帝类，反向教材 |
| **硬编码 CurseForge API Key 进仓库** | `Constants.java:104`，与我们 §5.8 直接冲突（**建议做成反例测试用例**） |
| **走自有后端**（API / analytics / paste / CDN 全自建） | 我们零后端定位，不适用 |
| **"Servers 页"作服务器列表参考** | ⚠️ **名称误导**：`Server.java` 是"用启动器开服务端"的模型，**不是服务器列表**。我此前的假设不成立 |
| Java 检测与约束表 | 明显弱于 HMCL（无版本×Java 约束枚举表） |
| 令牌存储 / 外置登录 | 本次未找到可参考实现，弱于 HMCL/Axolotl |
| 续传 / 多源测速 | 未见实现，弱于 LeviLauncher/HMCL/Axolotl |
| Swing + FlatLaf + RxJava 的 MVVM | 技术栈无关，界面层对我们无迁移价值（**令牌表除外**） |
| `.po` 语言文件 | **不在仓库**（Crowdin 托管），拿不到译文 |

---

## 一句话结论

**ATLauncher 对我们的价值集中在三处**：① **FlatLaf 设计令牌表**（我们令牌系统缺的成阶色彩，可 A1/A2/A3 直接用）；
② **`Mod.java` 包 schema + 5 种三方格式模型 + GraphQL 契约**（M8 整合包格式面与加载器元数据接口的现成设计）；
③ **Console 页与崩溃文案**（日志来源标记、四操作、五档令牌、可执行错误提示）。
**它的下载层、Java 选择、令牌存储都明显弱于 HMCL/Axolotl/LeviLauncher，不必参考。**
