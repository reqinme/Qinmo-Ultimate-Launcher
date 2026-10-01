# S9 · 基岩版真隔离（T2）风险评估 · 取证记录

> **本项只做评估，不写实现。** 目的：给"一期不做隔离"留下**书面依据**。
> 相关：`docs/M0-尖刺任务书.md` S9 一节；结论回填任务书结论槽与 `docs/M0-尖刺报告.md`。

## 0. ⚠️ 先纠正任务书的一处**划分错误**

任务书把两条路径按项目分：

| 任务书的划分 | **实际情况** |
|---|---|
| 路径 A ・ UWP 重定向 → `repos/LeviLauncher`（**不碰游戏本体**，风险低） | ✅ 隔离确实是"内容根重定向"，**但它需要开发者模式**（见 §2） |
| 路径 B ・ PE 导入表改写 → `repos/Portal`（改写游戏 exe，风险高） | ❌ **LeviLauncher 自己也有 PE 导入表改写**，只是**用途不是隔离** |

**源码实证**：`repos/LeviLauncher/internal/peeditor/importpatch.go`（11 KB + 7 KB 测试）
实现了完整的 PE32+ 导入表重建，函数 `EnsureImportedDLL`。它的注释写明目的：

> `EnsureImportedDLL makes exePath import funcName from dllName so the Windows
> loader loads dllName (and runs its DllMain) while snapping the executable's imports.`

**即"注入一个代理 DLL 以抢在游戏之前执行"**。而**唯一调用它的是 `internal/leviloader`**
（`leviloader.go:155`，模组加载器），**不是隔离**。

> **这个纠正很重要**，因为它改变了结论的形状：
> **"改二进制"与"做隔离"在同一个项目里是两件独立的事**，
> 而**隔离本身并不需要改二进制**。任务书把两者绑在一条路径上，会让评估得出错误的风险等级。

## 1. 路径 A · UWP 内容根重定向：**机制与代码出处**

| 问 | 答 | 出处 |
|---|---|---|
| **A1 机制** | **把 ApplicationData（内容根）重定向进实例目录**；非隔离实例则**共享包族的 LocalState**。且**UWP 的目录名按通道（channel）命名**，因为**"启动器 owns the redirect target"** | `internal/mcservice/versions.go:148-201`（含原文注释） |
| **A1 补充** | 版本门槛是一个常量：`UWPIsolationMinVersion = "1.19.70.2"`；`SupportsIsolation()` 对 **GDK 恒为 true**，只对 **UWP** 检查版本 | `internal/versions/isolation.go:3,6-11` |
| **A4 版本边界规则** | 比较逻辑有完整用例：`1.19.70.1`→否、`1.19.70.2`→是、`1.19.70.02`→**是**（等价写法）、`1.19.70`→否、`1.19.70.2-preview`→否、`1.19.70.2.0`→否 | `internal/versions/isolation_test.go:11-27` |
| **A4 原因** | **代码里没有解释"为什么是 1.19.70.2"** —— 只有一个常量与用例。**UI 文案里另有该值**，用于提示用户最低版本 | `frontend/src/utils/packageType.ts:9` · `frontend/e2e/uwp-isolation-min-version.test.cjs:42,80` |

**A4 的诚实结论**：**"为什么偏偏是 1.19.70.2"在本机可见的源码里没有答案**。
它只被当作一个既有事实（常量）使用。**要回答"为什么"，需要问上游或查提交历史**，
而这不是本机能取的证。

## 2. 🔴 路径 A 的决定性障碍：**需要 Windows 开发者模式**

**这是文档明写的，不是推断**：

| 出处 | 原文 |
|---|---|
| `README.md:22` | **"UWP: Windows Developer Mode and the UWP framework dependencies required by the package manifest"** |
| `README.zh-CN.md:22` | **"UWP：Windows 开发者模式，以及游戏包清单要求的 UWP 框架依赖"** |
| `docs/guide/requirements-installation.md:5` | "UWP versions require Windows Developer Mode and the UWP framework dependencies…" |
| `docs/zh-CN/guide/requirements-installation.md:5` | "**UWP 版本需启用 Windows 开发者模式**，并安装包清单要求的 UWP 框架依赖（例如 Microsoft.VCLibs）" |

**旁证三条**：

| 证据 | 说明 |
|---|---|
| 错误码 `ERR_DEV_MODE_REQUIRED` | `"系统未开启开发者模式，且自动开启失败。请手动开启开发者模式后重试。"` |
| 错误码 `ERR_UWP_DEVELOPER_MODE` | `"Windows 阻止了 UWP 开发包注册，请在系统设置中启用开发者模式后重试。"` |
| 注册机制 | 部署走 `Add-AppxPackage -Register`（PowerShell），卸载走 `Remove-AppxPackage`——**这正是"注册开发包"的做法** |

**还有一个"它自己也知道这是个负担"的证据**：
`internal/registry/devmode.go` 会去读 `AllowDevelopmentWithoutDevLicense`
并尝试用 `reg add HKLM\...` **自动开启**开发者模式——
**即"需要开发者模式"这件事对它是真实痛点，痛到要写代码去自动开。**

### 2.1 为什么这对我们是**硬冲突**而不是"一个可接受的代价"

**我们的定位已经写明不要求用户开开发者模式**（本项目的一条既定边界）。
更关键的是：**开启开发者模式会降低整台机器的应用安装门槛**，
而这不是一个"启动器该替用户做的决定"。

**所以路径 A 的结论不是"风险可控"，而是"与产品边界冲突"。**
——**这也是本节最有价值的产出**：它把一个技术选项变成了一条**边界判断**。

## 3. 路径 B · PE 导入表改写：**它存在，但与我们无关**

| 问 | 结论 | 依据 |
|---|---|---|
| B1 是否真改游戏 exe | **是**。`EnsureImportedDLL` 重建导入表、追加可写节、重定位原始节数据、修 checksum，且**幂等** | `internal/peeditor/importpatch.go:188-199` |
| B1 用途 | **加载 mod 加载器**（`LoaderDLLName` / `LoaderEntryName`），**不是隔离** | `internal/leviloader/leviloader.go:155` |
| B4 可逆性 | 代码注释写明"**已导入则原样返回、不动文件**"（幂等），但**未见到"还原导入表"的路径** | `importpatch.go:196-198` |
| B2/B3 误报与版本跟进 | 需查 issue 与版本节奏（见 §4） | 公开仓库 |

### 3.1 一条**与本项目红线直接相关**的旁证

`repos/LeviLauncher` 曾被质疑**仓库内含有闭源、用途不明的二进制**
（issue #15，标题："Concerns regarding closed source binaries present in the repository"），
涉及：私有仓库取得的 `vcruntime140_1.dll` 代理 DLL、用途不明的 `launcher_core.dll`（解 MSIXVC 包）、
**PE editor（改 `Minecraft.Windows.exe`，用于给游戏加控制台窗口）**、以及 PreLoader。

**这对我们的意义不是"它错了"，而是**：

> **一旦走上"改写游戏二进制 / 注入代理 DLL"这条路，就必然引入
> "用户无法审计的二进制"与"反作弊/杀软误报"这两类风险。**
> 而我们的红线里已经写了**不改游戏二进制**——**这条评估正好说明那条红线在保护什么。**

## 4. 许可（**必须记，因为它约束"能借鉴到什么程度"**）

| 项目 | 许可 | 对我们的约束 |
|---|---|---|
| `repos/LeviLauncher` | **GPL-3.0**（copyleft） | **绝不抄代码**；只可读机制并**用我们自己的实现表达** |
| `repos/Portal` | 见下（待补） | 同上 |

> 与 `docs/来源记录.md` §5「零代码引用」一致：**借机制，不借代码**。
> **GPL 的项目尤其要小心**：读它的**设计**与**抄它的实现**在版权上是两件事，
> 而在工程上很容易滑过去（"这段逻辑我照着重写一遍"）。**本项目的做法是：
> 只提取"问题 → 机制 → 边界"三层，实现独立写，并在 `来源记录.md` 登记出处。**

## 5. 待补（子智能体取证回来后回填）

- `repos/Portal` 的许可
- 路径 B 的 B2（杀软误报率）/ B3（游戏更新后失效概率）：需查 issue 与发版节奏
- A3（是否影响商店授权与自动更新）：本机源码里未见结论，需查文档/issue
- A5（失败如何回退）：需查备份/还原路径
