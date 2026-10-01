# MultiMC vs PrismLauncher 源码调研（上游视角）

> 目的：找出 **MultiMC 有而 Prism 没有、或 MultiMC 做得更好**的东西，并提取**可直接使用的资产**。
> 已读过 Prism 的组件栈（`PackProfile` + `Component`），本文**不重复**该部分。
> **未修改任何方案/计划文件。**

---

## 1. 实例格式：完整可移植规格（**本次最大产出**）

### 1.1 `mmc-pack.json`（组件列表，`PackProfile.cpp:124-154`）

```jsonc
{
  "formatVersion": 1,
  "components": [
    {
      // ---- critical（必须持久化）----
      "uid": "net.minecraft",           // PackProfile.cpp:65
      "version": "1.20.1",              // :66-69，为空则省略
      "dependencyOnly": true,           // :70-73，为满足依赖而自动加入，可被自动移除
      "important": true,                // :74-77，主组件或不可移除
      "disabled": true,                 // :78-81，禁用是一等状态

      // ---- cached（可重建，但落盘以省网络）----
      "cachedVersion": "...",           // :84-87
      "cachedName": "...",              // :88-91
      "cachedRequires":  [ ... ],       // :92
      "cachedConflicts": [ ... ],       // :93
      "cachedVolatile": true            // :94-97
    }
  ]
}
```

**⚠️ 上游一处真实缺陷，抄的时候必须避开**：
写入时字段名是 **`cachedVolatile`**（`:94-97`），但读取时找的是 **`"volatile"`**（`:117` `Json::ensureValueBoolean(obj.value("volatile"), false)`）。
→ **这个字段实际永远读不回来**。我们实现时**读写用同一个键名**。

**另一处值得学的小心机**：写入用 `QSaveFile`（`:134`），`commit()`（`:148`）才原子替换。
→ 与我们"原子写"的纪律一致，且它是**框架自带**的原子写。

### 1.2 `patches/<uid>.json`（单组件覆盖文件）

- 路径模式：`PackProfile.cpp:254-257` `patchesPattern() = <instanceRoot>/patches/%1.json`
- 文件名（**不含扩展名**）**就是 uid**：`:520` `QString uid = info.completeBaseName();`
- 加载时会**回写规范化**：`:536-538` 用文件名纠正 `file->uid` 后 `saveJsonFile(...)` 重新落盘
- **`patches/net.minecraft.json` 是主版本文件**：`:381` `auto mcJson = FS::PathCombine(root, "patches", "net.minecraft.json");`

**这个"文件名即 uid"的约定很实用**——用户可以直接改文件名来改变组件身份，且不依赖文件内容。

### 1.3 `instance.cfg`（INI，`MinecraftInstance.cpp:82-135`）——**双层覆盖模式**

```ini
# 每个可覆盖项 = 一个 OverrideXxx 布尔 + 一个具体值
OverrideJava=false
OverrideJavaLocation=false
OverrideJavaArgs=false
OverrideMemory=false
OverrideWindow=false
OverrideMCLaunchMethod=false
OverrideNativeWorkarounds=false
OverrideGameTime=false

# 启动时直连
JoinWorldOnLaunch=false
JoinServerOnLaunch=false
JoinServerOnLaunchAddress=""
JoinSingleplayerWorldOnLaunch=false
JoinSingleplayerWorldOnLaunchName=""

# 旧配置遗留字段
IntendedVersion=""          # 别名 MinecraftVersion（:132）
LWJGLVersion=""
ForgeVersion=""
LiteloaderVersion=""
```

**这是"全局默认 + 实例覆盖"的标准解法，且比 nullable 值更清晰**：
"是否覆盖"与"覆盖成什么"是两个独立事实，用户重置时只需把 `Override*` 关掉。
→ **我们 §5.5 的"全局与实例配置层级合并"应当采用这个模式，而不是"有值就覆盖"。**

### 1.4 实例目录结构

```
<instances>/<name>/
├── instance.cfg           # INI 设置（上表）
├── mmc-pack.json          # 组件列表（§1.1）
├── patches/<uid>.json     # 单组件覆盖（§1.2）
├── order.json             # 【遗留】旧版组件排序，只读迁移（PackProfile.cpp:562-564）
├── .minecraft/            # 游戏目录
└── ...                    # 其余按组件产出
```

---

## 2. 组件 UID 体系（**可直接用的命名规范**）

**规则：反向 DNS / Maven groupId 风格，全小写点分**。

| UID | 含义 | 证据 |
|---|---|---|
| `net.minecraft` | 主版本（**唯一必需组件**） | `OneSixVersionFormat.cpp:215`、`PackProfile.cpp:508` |
| `net.minecraftforge` | Forge | `PackProfile.cpp:559` |
| `net.fabricmc.fabric-loader` | Fabric | `InstanceImportTask.cpp:457` |
| `com.mumfrey.liteloader` | LiteLoader | `PackProfile.cpp:560` |
| `org.lwjgl` / `org.lwjgl3` | LWJGL 2 / 3（**内建 patch**） | `PackProfile.cpp:505-508` |

**`uid` 与 `version` 的关系**：uid 定位清单，version 定位清单内条目。
`Component.m_uid` 与 `m_version` 是**持久化的两个独立字段**（`Component.h:79-81`），
且注释说明 `m_version` **同时是"自定义 JSON 覆盖时回退到的版本"**（`:80`）。

**发现 `org.lwjgl`/`org.lwjgl3` 是内建组件**这一点很重要：
**LWJGL 也是一等组件**，不是隐藏在版本 JSON 里的库。Prism 保留了这个设计。

---

## 3. 元数据来源：**配置驱动的远程索引**（Prism 保留了它）

**架构**：`Meta::BaseEntity`（`BaseEntity.h:27-66`）是所有元数据实体的基类，提供 `parse()` / `localFilename()` / `url()` / `load()`。

```cpp
// BaseEntity.cpp:76-79
QUrl Meta::BaseEntity::url() const {
    return QUrl(BuildConfig.META_URL).resolved(localFilename());
}
```

**URL 由"基址 + 本地文件名"解析而成**：

| 实体 | `localFilename()` | 证据 |
|---|---|---|
| 索引 | `"index.json"` | `Index.h:49` |
| 版本清单 | `<uid> + "/index.json"` | `VersionList.cpp:122-124` |
| 单版本 | `Version.cpp:97` | — |

**基址**：`CMakeLists.txt:140` → **`https://meta.multimc.org/v1/`**

因此实际 URL：
```
https://meta.multimc.org/v1/index.json                    # 组件索引（有哪些 uid）
https://meta.multimc.org/v1/net.minecraft/index.json      # 原版版本清单
https://meta.multimc.org/v1/net.fabricmc.fabric-loader/index.json
https://meta.multimc.org/v1/net.minecraftforge/index.json
```

**本地缓存**：`MetaCache`（`Application.cpp:915-932`）按 **"URL 基路径 → 磁盘目录"** 映射：

| 缓存 key | 目录 |
|---|---|
| `meta` | `meta/` |
| `versions` | `versions/` |
| `libraries` | `libraries/` |
| `asset_indexes` / `asset_objects` | `assets/indexes` / `assets/objects` |
| `minecraftforge` / `fmllibs` / `liteloader` | `mods/minecraftforge` 等 |
| `ModrinthPacks` / `ATLauncherPacks` / `FTBPacks` / `TechnicPacks` | `cache/*Packs` |

### ⚠️ 这条对我们最重要的判断

**MultiMC 系把"版本与加载器元数据"交给一台中央服务器（`meta.multimc.org`）。**
`META_URL` 只是**编译期常量**（`BuildConfig.cpp.in:49`），用户不能改（除非自己编译）。

→ **这是一个明确的架构取舍，也是一个依赖风险**：官方元数据一旦不可达或改变，启动器就装不了版本。
→ **我们不应照搬**。我们已有 `IVersionSource` 抽象，应当**默认直连 Mojang + 各加载器官方源**，
   把"中央索引服务器"作为**可选镜像**而非唯一来源。
→ **但"基址 + `<uid>/index.json`"这个 URL 约定值得抄**：它让"加一个新加载器"变成**加一个目录**，而不改代码。

---

## 4. 依赖解析：`requires` / `conflicts`（**Prism 没有的机制，最有价值**）

### 4.1 依赖声明 schema（`JsonFormat.h:42-68`，`OneSixVersionFormat.cpp:208-226`）

```jsonc
// 版本文件里可声明
{
  "requires":  [ { "uid": "net.minecraft", "equals": "1.20.1", "suggests": "1.20.2" } ],
  "conflicts": [ { "uid": "org.lwjgl3" } ],
  "mcVersion": "1.20.1",     // 简写：自动转成 requires net.minecraft equals 1.20.1
  "volatile": true
}
```

**`Require` 结构（三字段，`JsonFormat.h:42-61`）**：

| 字段 | JSON 键 | 语义 |
|---|---|---|
| `uid` | `uid` | 依赖的组件 |
| `equalsVersion` | `equals` | **精确版本约束**（不是范围） |
| `suggests` | `suggests` | **建议版本**（用于"要不要升级"的默认答案） |

**⚠️ 一个关键设计陷阱**：`RequireSet = std::set<Require>`（`JsonFormat.h:68`），
而 `operator<` **只比较 `uid`**（`:48-51`）。
→ **同一 uid 的两个不同 `equals` 约束会被 set 去重掉一个！**
→ **我们实现时应用 `HashMap<uid, Constraint>` 或 `Vec`，不要用"按 uid 去重的集合"。**

### 4.2 依赖合成算法（`ComponentUpdateTask.cpp:290-381`）

```
composeRequirement(a, b):            # 同一个 uid 的两条约束合并
  indexOfFirstDependee = min(a, b)   # 记录"最早依赖它的组件序号"→ 决定加载顺序
  equalsVersion:
      一方为空 → 取另一方
      两者相同 → 取该值
      两者不同 → ✗ 冲突（:310 注释 "FIXME: mark error as explicit version conflict"）
  suggests:
      一方为空 → 取另一方
      两者都有 → 取版本号更大的（:324-326）
```

**`gatherRequirementsFromComponents`（`:332-381`）**：遍历所有组件、按 uid 累加合并，
**任一合并失败即整体失败**（`:370` `succeeded &= result.ok`）。

### 4.3 自动清理：`dependencyOnly` + `volatile`

`getTrivialRemovals`（`:384-399`）：一个组件**同时满足**
`m_dependencyOnly == true`（为满足依赖而加）**且** `m_cachedVolatile == true`（元数据声明可丢弃），
**且当前没有任何组件 require 它** → 自动移除。

**这是一套完整的"自动装依赖 + 自动清依赖"，而且是声明式的（元数据说了算，不是代码写死）。**

### 4.4 ⭐ 与 Prism 的关键差异（**结论：MultiMC 更优**）

| | MultiMC | Prism |
|---|---|---|
| 依赖/冲突来源 | **元数据声明**（`requires`/`conflicts`，每个版本文件自己声明） | **代码里硬编码** |
| 证据 | `Component.h:94-95` `m_cachedRequires` / `m_cachedConflicts` | Prism `Component.h:47-50` `ModloaderMapEntry{type, knownConflictingComponents}`、`:62` `KNOWN_MODLOADERS`、`:77` `knownConflictingComponents()` |
| 可扩展性 | **加一个加载器只需它自己的元数据声明冲突** | **加一个加载器要改 C++ 代码里的映射表** |
| 能力 | 依赖 + 冲突 + **建议版本** + 自动清理 | 冲突（限定在已知加载器之间） |

**判断：Prism 在这一点上是"退步"**——它用一张硬编码的已知加载器冲突表，换取了实现简单。
**我们应采用 MultiMC 的元数据声明式方案**（`requires` / `conflicts` / `suggests` / `volatile` 四件套）。

---

## 5. 文件校验：`Validator` 模式（**可直接抄的抽象**）

`net/ChecksumValidator.h`：校验器**挂在下载流上**，边下边算：

```cpp
class ChecksumValidator : public Validator {
    bool init(QNetworkRequest&) override { m_checksum.reset(); return true; }
    bool write(QByteArray& data) override { m_checksum.addData(data); return true; }  // 流式，零额外读盘
    bool abort() override { return true; }
    bool validate(QNetworkReply&) override {
        if (m_expected.size() && m_expected != hash()) { qWarning() << "Checksum mismatch"; return false; }
        return true;
    }
    void setExpected(QByteArray expected);
};
```

**它被用在这些地方**（可直接对照我们的下载引擎）：
- 库：`Library.cpp:91-95`（SHA-1，从 `QByteArray::fromHex(sha1)` 转二进制）
- 资源：`AssetsUtils.cpp:298`（SHA-1）
- 资源索引：`AssetUpdateTask.cpp:35-39`
- 整合包文件：`InstanceImportTask.cpp:477`（**算法由文件声明，`file.hashAlgorithm`**）
- ATLauncher / Technic / 官方更新：MD5

**两个值得抄的细节**：
1. **算法是参数**（`QCryptographicHash::Algorithm`），不是写死 SHA-1——整合包可能用 MD5。
2. **`expected` 可后设**（`setExpected`）——不必在构造时就确定。
3. **哈希复用做缓存失效**：`MetaCacheSink` 自己带一个 MD5（`MetaCacheSink.h:10`、`Download.cpp:41`），
   **缓存条目与内容校验用同一套哈希**。这是"缓存正确性"的干净解法。

**⚠️ 一处应当警惕的缺口**：`BaseEntity.cpp:88` 有一行
`// TODO: check if the file has the expected checksum`
→ **元数据文件本身不校验**。我们应补上（元数据是我们一切校验的信任根，它自己不校验是逻辑漏洞）。

---

## 6. 旧配置迁移：两条真实迁移路径（**印证我们的迁移链设计**）

`migratePreComponentConfig()` / `load()`（`PackProfile.cpp:272-322`）：

| 迁移 | 触发 | 证据 |
|---|---|---|
| **无 `mmc-pack.json`** → 从旧配置重建 | `:278-286` 文件不存在时调 `migratePreComponentConfig()`，**失败只 `qCritical` 不阻断** | `:280-285`，注释 `// FIXME: the user should be notified...` |
| **旧 `order.json` 排序** → 合并进组件顺序 | `:562-580` 读 `ProfileUtils::readOverrideOrders(...)`，按用户排序取组件；`:583-603` 剩余的按 `getOrder()` 数值插入（`QMultiMap<int,...>` 排序并**检测重复**） | 见左 |

**另外注意**：`Component.h:44-46` 与 `:100-102` 都标着
`// DEPRECATED: explicit numeric order values, used for loading old non-component config. TODO: refactor and move to migration code`

→ **它自己也把"数值排序"当成待清偿的迁移债，且明确说"应移到迁移代码里去"。**
→ **两条印证**：(1) 迁移逻辑应当**隔离在专门的迁移代码里**，不留在运行时模型上；
   (2) 我们 §4.7 的"迁移链 + 迁移回执"方向正确，且**它缺的正是"回执"与"失败告知用户"**。

---

## 7. 与 Prism 的关键差异清单（**本次最重要清单**）

| # | 维度 | MultiMC | Prism | 谁更好 |
|---|---|---|---|---|
| 1 | **冲突表达** | **元数据声明** `requires`/`conflicts` | **代码硬编码** `KNOWN_MODLOADERS` 映射表 | **MultiMC** ✅ |
| 2 | 建议版本 | 有 `suggests`（合并时取更大者） | 无 | **MultiMC** ✅ |
| 3 | 自动清理依赖 | `dependencyOnly` + `volatile` + 无人依赖 → 自动移除 | 未见等价机制 | **MultiMC** ✅ |
| 4 | 依赖加载顺序 | `indexOfFirstDependee`（**最早依赖者优先**） | 未见 | **MultiMC** ✅ |
| 5 | 已知加载器识别 | 无（不需要，靠元数据） | 有 `isKnownModloader()` | Prism（但属补丁，非机制） |
| 6 | 内建组件 | `net.minecraft` + `org.lwjgl`/`org.lwjgl3` 内建 patch | 保留 | 平 |
| 7 | 元数据来源 | `meta.multimc.org`（编译期常量） | Prism 自有 meta 服务 | 平（都有中央依赖） |
| 8 | 组件 `ProblemProvider` | 有（`Component.h:62-63`） | 有（`Component.h:96-99`） | 平 |
| 9 | 遗留债 | 数值排序、`order.json`、`volatile` 键名不一致 | 继承了部分并清理 | 平 |

**结论**：**Prism 是 MultiMC 的功能超集，但在"依赖与冲突的表达方式"上是一次回退。**
**凡是"用元数据声明优于用代码硬编码"的地方，MultiMC 更值得学。**

---

## 8. 未覆盖项（如实记录）

| 项 | 状态 |
|---|---|
| `GroupView` / 实例分组 | **未读**（本轮预算优先给了格式与依赖解析） |
| Java 检测与选择 | **仅见** `VerifyJavaInstall.cpp:35`（取 `net.minecraft` 组件）、`JavaSettingsWidget.cpp:136`（`loadList`），未展开 |
| 启动链路（参数组装 / natives / classpath） | **未读**（Prism 的同类实现已在上一轮覆盖） |
| 实例创建/复制/导入导出 | **仅见** 文件级线索（`InstanceCopyTask` / `InstanceImportTask` / `ModrinthInstanceExportTask`），未展开 |
| 版本比较算法 | **未定位**（依赖解析里用了 `Version aVer(a.suggests)` 做大小比较，`ComponentUpdateTask.cpp:324-326`，但比较器实现未读） |

---

## A. 可直接用的资产

| 资产 | 位置 | 用法 |
|---|---|---|
| **`mmc-pack.json` 规格** | `PackProfile.cpp:61-121` | 照抄字段设计，**但修正 `cachedVolatile` 键名不一致的 bug** |
| **`instance.cfg` 双层覆盖模式** | `MinecraftInstance.cpp:82-135` | `Override*` 布尔 + 值，用于全局/实例层级合并 |
| **组件 UID 命名规范** | 见 §2 表 | 反向 DNS 点分小写；`<uid>/index.json` URL 约定 |
| **`requires`/`conflicts` schema** | `JsonFormat.h:42-68`、`OneSixVersionFormat.cpp:208-226` | `{uid, equals, suggests}`；**用 map 不要用 set** |
| **依赖合成算法** | `ComponentUpdateTask.cpp:290-381` | 4 条合并规则可直接实现 |
| **`Volatile` 自动清理条件** | `:384-399` | `dependencyOnly && volatile && 无人依赖` |
| **`Validator` 流式校验模式** | `net/ChecksumValidator.h` | 边下边算，算法为参数，`expected` 可后设 |
| **元数据 URL 约定** | `BaseEntity.cpp:76-79` | 基址 + `localFilename()`；`<uid>/index.json` |
| **meta 缓存目录映射表** | `Application.cpp:915-932` | 13 个缓存 key → 目录的完整对照 |

## B. MultiMC 优于 Prism / Prism 没有的机制（**最重要清单**）

1. **元数据声明式的 `requires` / `conflicts`**（Prism 退化为硬编码映射表）——**最高价值**
2. **`suggests` 建议版本**（合并取更大者）——用于"要不要升级"的默认答案
3. **`dependencyOnly` + `volatile` 自动清理依赖**
4. **`indexOfFirstDependee` 依赖顺序推导**
5. **`Validator` 流式校验抽象**（算法参数化 + `expected` 可后设 + 与缓存哈希复用）
6. **`instance.cfg` 的 `Override*` 双层覆盖模式**
7. **`patches/<uid>.json` "文件名即 uid"** 的约定

## C. 明确不适合的

| 不适合 | 理由 |
|---|---|
| **中央元数据服务器作为唯一来源** | `META_URL` 是编译期常量、用户不可改；官方站不可达即装不了版本。我们应默认直连官方源 + 可选镜像 |
| **元数据文件不做校验** | `BaseEntity.cpp:88` TODO；元数据是信任根，不校验是逻辑漏洞 |
| **`RequireSet = std::set<Require>` 按 uid 去重** | 会丢弃同一 uid 的第二个版本约束。改用 map/vec |
| **数值型 `m_order` 排序残留** | 上游自己标了 DEPRECATED 与 TODO；我们用显式组件数组顺序即可 |
| **`order.json` / `IntendedVersion` 等遗留字段** | 上游为兼容旧实例保留；我们新项目不必背 |
| **Qt 的 `QAbstractListModel` 双重身份** | 内核不应依赖 UI 框架的模型接口（Prism 同样问题） |
