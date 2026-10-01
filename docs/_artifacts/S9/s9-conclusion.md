# S9 · 基岩版真隔离（T2）风险评估 · 结论

> **本项只做评估，不写实现。** 目的：给"一期不做隔离"留下**书面依据**。
> 回填位置：`docs/M0-尖刺任务书.md` S9 结论槽 · `docs/M0-尖刺报告.md` 附录。
> **取证过程与逐条出处**见 `s9-evidence.md`（同目录）。

## 0. 结论摘要（三条，各自独立成立）

| # | 结论 |
|---|---|
| **1** | **路径 A（UWP）与路径 B（PE 改写）都不能按原样做**，但**原因完全不同**：A 是**红线**，B 是**只对非商店副本成立且与 UWP 互斥** |
| **2** | **"隔离"这个词用在这里偏强**：它不是沙箱，而是**进程内 API 返回值的替换**（见 §3）——应改称"**数据目录重定向**" |
| **3** | **任务书对两条路径的划分有误**：它把"改二进制"归给 Portal，而 **LeviLauncher 自己就改二进制**，且**改的正是把红线那一块送进游戏的手段**（见 §2.2） |

## 1. 路径 A · UWP 数据目录重定向 → **不做（红线）**

### 1.1 机制（源码实证）

**不是清单技巧、也不是系统级沙箱**，而是：

> **给游戏进程注入一个原生 DLL，劫持 WinRT `ApplicationData.LocalFolder` 的 COM vtable 槽**，
> 让 `get_LocalFolder` 返回实例目录。

| 环节 | 出处 |
|---|---|
| 劫持 vtable 槽 | `native/levilauncher/src/hook/folder_redirect.cpp:43-57`（`replace_vtable_slot`）、`:110-122`、`:182-213` |
| **DLL 怎么进进程** | **靠改写游戏的 PE 导入表**（`internal/leviloader/leviloader.go:155` → `peeditor.EnsureImportedDLL`） |
| 版本门槛 | 常量 `UWPIsolationMinVersion = "1.19.70.2"`（`internal/versions/isolation.go:3`），GDK 恒为 true |

### 1.2 🔴 为什么是"不做"而不是"风险可控"

**两个独立理由，任一个都足够**：

**理由一：需要 Windows 开发者模式（文档明写）**

| 出处 | 原文 |
|---|---|
| `README.zh-CN.md:22` | **"UWP：Windows 开发者模式，以及游戏包清单要求的 UWP 框架依赖"** |
| `docs/zh-CN/guide/requirements-installation.md:5` | **"UWP 版本需启用 Windows 开发者模式"** |
| 错误码 | `ERR_DEV_MODE_REQUIRED` · `ERR_UWP_DEVELOPER_MODE` |
| 部署方式 | `Add-AppxPackage -Register`（注册开发包） |

**它自己也知道这是负担**：`internal/registry/devmode.go` 会读
`AllowDevelopmentWithoutDevLicense` 并尝试 `reg add HKLM\...` **自动开启**开发者模式。

**我们的定位已写明不要求用户开开发者模式**——更关键的是，
**开启开发者模式会降低整台机器的应用安装门槛，这不是启动器该替用户做的决定。**

**理由二：🔴 它的宿主机制是一个真实的 DRM 绕过（本项最重发现）**

`internal/uwp/auth_key.go:16-22` **硬编码并替换游戏的 Xbox 认证公钥**，
注释**自己写出出处**（第三方项目 KeyPatcher）：

```go
// Public key bytes and replacement behavior verified against KeyPatcher:
// https://github.com/ambiennt/KeyPatcher/...
const (
    legacyAuthPublicKey  = "MHYwEAYHKoZIzj0CAQYFK4EEACIDYgAE8ELkixyLcwlZryUQ..."
    currentAuthPublicKey = "MHYwEAYHKoZIzj0CAQYFK4EEACIDYgAECRXueJeTDqNRRgJi..."
    authKeyMarkerName    = ".levilauncher-xbox-auth-key-v1"
)
```

而 `ensureLegacyAuthKey` **在正常部署路径上被调用**（不是可选项）：

| 调用点 | 场景 |
|---|---|
| `internal/uwp/deploy_windows.go:266` | **注册/部署流程内** |
| `internal/uwp/extract.go:240` | **解包流程内** |

**这落在我们方案 §1.4 的红线里**（破解登录 / 篡改游戏二进制 / 绕过商店授权）。

> **所以结论比"不抄代码"更强**：
> **不是"我们可以读它但自己写"，而是"这个机制我们不该做"。**

**并且路径 A 无法与它解耦**：重定向 DLL 进游戏的方式，
**与替换认证公钥的方式是同一个**（都是"改 PE 导入表 + 注入代理 DLL"）。
**想只取重定向那一半，就必须另找注入手段**——而那正是路径 B 的激进路线（见 §2）。

### 1.3 其余四问

| 问 | 结论 | 置信度 |
|---|---|---|
| **A3 影响商店授权/自动更新？** | **未找到**任何说明或代码判断（搜 `Store`/`License`/`Update`/`auto` 全仓，命中只有 UI 文案与更新器，**无一处提到隔离） | **未找到** |
| **A4 版本边界原因？** | 常量真实存在，但**全仓库没有一处注释或文档解释"为什么是 1.19.70.2"**（`1.19.70.2` 共 25 处命中，全是常量/比较/测试/i18n） | 常量=源码实证；**原因=未找到** |
| **A5 失败如何回退？** | 有：注册失败用另一个 context 把旧包重新注册回去（`ERR_UWP_REGISTER_ROLLBACK`），数据靠 `-PreserveApplicationData` | 源码实证 |

**A4 的等价写法规则**（值得记，我们的版本比较器要用同一条）：
`1.19.70.2` 视为 `1.19.70.02`；而 `1.19.70`、`1.19.70.2-preview`、`1.19.70.2.0` 均**不**成立。

## 2. 路径 B · PE 导入表改写 → **有条件考虑，且只对"解包副本"成立**

### 2.1 它比任务书描述的**窄**

任务书写的是"改写游戏 exe"。**实际手法要窄得多**：

> **原地重命名已有的导入 DLL 名**（所以**长度受限**，名字过长直接抛错），
> **只有在缺失时才 `AddImport`**。

出处：`src/Portal.Bedrock.Windows/BedrockDataIsolation.cs:301-333`、`:325-327`。

**可逆**：备份在 `config/Portal/Minecraft.Windows.portal-original.exe`，
且**每次启动都先从 `.orig` 还原、再重新打补丁**（`:32-34`、`:53-73`）。

**"每次启动都先还原再重打"这件事本身是一个结论**：作者**默认补丁会失效**（游戏更新后）。
（"默认会失效"是我的推断；"每次先还原"是源码事实。）

### 2.2 🔴 但 Portal 自己就**对 UWP 禁用了**它

```csharp
SupportsBedrockDataIsolation => IsBedrock && !IsUwpBedrock
```

出处：`src/Portal/Views/Pages/InstancePages/Properties.axaml.cs:35`（另见 `:33`）。

**这是 B5 的答案**：**路径 A 不覆盖路径 B，两者互补**——
而且**做 PE 改写的那一方，自己认为它在 UWP/商店形态下不可行**。

**对我们的直接意义**：**B 只可能用在"解包后的副本"上**，
而"解包副本"意味着**我们已经不再与商店安装的那一份打交道**——
那本来就是另一种产品形态（相当于自建一份可执行文件），**代价与法律风险都极大**。

## 3. 一条**措辞**上的纠正：「隔离」不是沙箱

路径 A 的机制是**进程内 API 返回值的替换**：
游戏以为自己写在 `LocalState`，实际写在实例目录。

> **任何绕过被 hook 的 API 的路径（直接 `\\?\` 路径、自带文件 API）都会漏。**

**所以 S9 的结论里，凡涉及此机制处一律称「数据目录重定向」，不称「真隔离」。**
（任务书原文用"真隔离（T2）"，本结论不沿用这个措辞。）

## 4. 任务书的一处划分错误（**需回填任务书**）

| 任务书的划分 | **实际情况** |
|---|---|
| 路径 A → LeviLauncher（**不碰游戏本体**，风险低） | ❌ **它碰**：DLL 注入靠改 PE 导入表；且部署路径里替换认证公钥 |
| 路径 B → Portal（改写游戏 exe，风险高） | ⚠️ 存在，但**窄**（原地重命名，长度受限），**可逆**，且**它对 UWP 主动禁用** |

**"改二进制"与"做隔离"在 LeviLauncher 里是绑在一起的**，
而**任务书把它们当成两条独立路径**，会让评估得出错误的风险等级。

## 5. 建议（回填结论槽的两行）

```
【路径 A · UWP 重定向】
  A1 机制：进程内劫持 WinRT ApplicationData 的 COM vtable 槽
          （DLL 靠改写游戏 PE 导入表注入）
  A2 需开发者模式：☑是（README 与文档明写）
  A3 影响授权/更新：未找到证据（搜 Store / License / Update / auto，无一处提及）
  A4 版本边界原因：未找到（常量 1.19.70.2 真实存在，但无任何注释解释为何是它）
  A5 可回退：☑是（失败时重新注册旧包 + -PreserveApplicationData）
  建议：☑放弃
    理由一：需要开发者模式，与"不要求用户开开发者模式"的定位冲突
    理由二：🔴 其宿主机制（改 PE 导入表注入 DLL）同时承载"替换游戏 Xbox 认证公钥"，
           后者落在方案 §1.4 红线内（破解登录 / 篡改游戏二进制 / 绕过商店授权）
    补充：机制无法与红线解耦 —— 想只取重定向那一半必须另找注入手段

【路径 B · PE 导入表改写】
  B1 完整性校验：未找到任何"二进制签名校验"的讨论（"校验"命中全是下载包 MD5/SHA）
  B2 误报：未评估（不适用，见建议）
  B3 更新后失效：作者默认会失效 —— 每次启动都先从 .orig 还原再重新打补丁
  B4 可逆性：☑可逆（备份 .orig + 每次启动先还原）
  B5 等效手段：否 —— 路径 A 不覆盖它，两者互补；且 Portal 对 UWP 主动禁用 B
  建议：☑有条件考虑
    条件：仅对"解包后的副本"成立，且必须保留 .orig 备份与每次启动还原
    但不作为一期方案：解包副本意味着不再与商店安装的那一份打交道，
    那已是另一种产品形态，代价与风险都远超"做一个实例隔离"
```

## 6. 许可（约束"能借鉴到什么程度"）

| 项目 | 许可 | 约束 |
|---|---|---|
| `repos/LeviLauncher` | **GPL-3.0** | 只读机制，**绝不抄代码** |
| `repos/Portal`（`github.com/tiouoo/Portal`） | **AGPL-3.0** | 同上；**AGPL 更强**，且 `来源记录.md` 已登记为"只读设计" |

> 两者在 `docs/来源记录.md` §1 里**都已登记**，本节不新增来源，只补"用途与边界"的结论。

## 7. 顺带的产品结论（影响 M9）

**一期不做基岩版实例隔离**，并且**如实告知**（方案 §6.2 已定的口吻：
"做不到就如实声明 `isolation:false` 并给原因"）。**现在这个"原因"有了书面依据**：

> 隔离需要把 DLL 注入游戏进程，而**在当前已知的两种做法里，
> 一种（UWP）需要开发者模式且宿主机制承载 DRM 绕过，另一种（PE 改写）
> 只对解包副本成立且被其作者自己对 UWP 禁用**。
> **我们的选择是不做，而不是做得不完整。**
