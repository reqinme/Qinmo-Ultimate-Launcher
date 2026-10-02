# 上次收工：**第一次真的把窗口打开** —— 人眼抓到两个布局真 bug，而 `verify.ps1` 长出两条能踩红的检查（13 → **15 项**）

> **这一轮做的不是新功能，是把"界面看起来不对"查成两个可复现的布局缺陷，再给它们各配一条会踩红的检查。** 两个 bug 都不是"样式不好看"：一个是 `.shell` 被两个 CSS 文件定义（整个外壳被 U0 验收页的 720px 居中竖排接管），另一个是网格把**主区放进了 0 宽的那一列**（主页被压成一行一个汉字）。而 **eslint / tsc / vitest(281) / impeccable / tokens / ladder 一个都看不见它们** —— jsdom 没有布局引擎，CSS 检查器只读令牌。只有真窗口里人眼能看见。

## 这一轮（2026-10-02 下半场）：第一次真的打开窗口

### 1. 启动与度量（第一次真跑）

- 跑的是 release：`src-tauri\target\release\qul-desktop.exe`（当时 **3.90 MB**），窗口标题「秦墨」。`DwmGetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` = **2** ⇒ **Mica 真的生效**（`DWMWA_CLOAKED` = 0，不是被隐藏窗口）。
- **内存第一次有了真数字**：主进程 **27.5–28.2 MB** + 属于它的 WebView2 **7 个进程 / 428.1 MB** = **455.6 MB**，对照方案 v3.14 定的预算（主进程 80 / WebView2 400 / 合计 480 MB）**在预算内**。
- ⚠️ **WebView2 的归属不能按进程名算**：本机同时有 **21 个** `msedgewebview2` 合计 **1129.3 MB**，绝大多数属于别的程序。按命令行里的 `--webview-exe-name=qul-desktop.exe` 与 `--user-data-dir=…app.qinmo…` 才筛得出属于自己的那 7 个。
- ⚠️ **截图必须 DPI 感知**：本机显示器 150% 缩放（虚拟 1707×1067 / 物理 2560×1600），而 PowerShell 是 **DPI 未感知**进程 ⇒ `CopyFromScreen` 拿到的是被系统缩放的合成图，尺寸与位置全错（我第一张"界面不像设计稿"的截图就是这么来的）。先调 `user32!SetProcessDPIAware()`，之后窗口物理矩形是 **1942×1213**。可复用脚本留在 `%TEMP%\cap.ps1`。

### 2. 真 bug ①：`.shell` 被两个 CSS 文件定义

- `web/src/styles.css:32`（U0 验收页的骨架，文件头自己写着"本页只是 U0 的验收页，不是最终界面"）定义了 `.shell { max-width: 720px; margin: 0 auto; display: flex; flex-direction: column; … }`；`web/src/routes/Shell.css:12`（真外壳）定义了 `.shell { display: grid; grid-template-columns: var(--shell-rail-w) var(--shell-secondary-w) 1fr; … }`。两条**同权重**，谁赢只由 `web/src/main.tsx:40-41` 的 import 顺序决定 —— 而 `styles.css` 在后面。
- 症状：整个桌面外壳被那一页的"720px 居中 + 竖排"接管 —— 标题栏（连 ― ☰ ✕）不在窗口右上角、侧栏堆在主区**上面**、窗口其余部分是材质底色。**没有任何报错**：React 渲染了，281 项前端测试全过。
- **找到它的方式不是读源码，是读打包后的 CSS**（`web/dist/assets/index-*.css` 里 `.shell{max-width:720px…}` 在位置 35615、`.shell{display:grid…}` 在 5653）。
- 修法：`web/src/styles.css` 里四条 `.shell*` 改名 `.capsPage*`（现在在 `:61` / `:70` / `:76` / `:82`），`web/src/App.tsx:22-25` 跟着改；并写下一条纪律：**`styles.css` 永不定义 `.shell*`** —— 那个前缀属于 `web/src/routes/Shell.css`。

### 3. 真 bug ②：网格把主区放进 0 宽的那一列（① 修好之后才露出来）

- `web/src/routes/Shell.css:61-62` 的 `.shell:not(:has(.shell__secondary)) { grid-template-columns: var(--shell-rail-w) 0 1fr; }` 把第二列**收成 0 宽**（主页本来就没有二级栏），而 `.shell__rail` / `.shell__secondary` / `.shell__main` **都没写 `grid-column`** ⇒ 网格按 DOM 顺序自动落格：标题栏（`:50-52` 有 `grid-column: 1 / -1`）占第一行，侧栏落第 1 列、**主区落进第 2 列（0 宽那一列）** ⇒ 主页正文被压成**一行一个汉字**，窗口右半边空的。
- `:has()` 在 WebView2 里**是支持的**（实测：主区紧贴 196px 侧栏的右缘，而不是 196+176=372 处）⇒ 第二列真的是 0 宽，诊断成立。
- 修法：显式落格 —— `web/src/routes/Shell.css:91`（`.shell__rail` 的 `grid-column: 1`）、`:179`（`.shell__secondary` 的 `grid-column: 2`）、`:219`（`.shell__main` 的 `grid-column: 3`），并在 `.shell__rail` 上方写明这个真 bug + "按 DOM 顺序自动落格只在二级栏总是存在时才碰巧是对的"。

### 4. 两条会踩红的检查（用户拍板"先补能踩红的检查，再补主页 UI"）

- **`tools/check-css-classes.ps1`**（两条规则，无 allow-list）：
  - **R1** 一个类名只能被**一个** `.css` 文件定义（两个所有者 ⇒ 谁生效由打包顺序决定，而没人读打包顺序）；
  - **R2** TSX 里每个 `className` 里的类名都必须有定义（未知类名 = 没有样式的元素，**静默**，与未定义的 `--token` 同一种失败形状）。
  - ⚠️ **它自己刚写出来时漏了一整类**：`className="gallery"` 这种**裸字面量**（引号已经被剥掉，而后面那段扫描器又在找引号 ⇒ 什么都没收到，症状是只报得出 `{...}` 里的名字）。已修，并把行号改成指向规则本身而不是上一条规则的 `}`。
- 它一次报出 **18 个真问题**：
  - **2 个类名冲突**：`.panel` 与 `.panel__title` 同时被 `web/src/styles.css` 与 `web/src/components/components.css` 定义，而两处**规则不同** ⇒ **组件库的面板标题被 U0 验收页的样式覆盖**（多了一份 flex、字重从 semibold 变 500）。
  - **16 处"用了但没有任何定义"**：`btn__label` / `select__viewport` / `choice__mark`（`web/src/components/index.tsx:82` / `:240` / `:281`）、`fieldset--${state}`（`:307`，而 `components.css` 里一个 `.fieldset--*` 都没有）、以及 `gallery` / `gallery__row` / `gallery__grid` / `gallery__col` / `gallery__sidebar` / `gallery__sidebarDemo` 九个 —— 也就是说 **M4 门禁第②项的自验页（`/__components`）整页没有任何样式**。
  - 修法：`web/src/styles.css` 删掉 `.panel` / `.panel__title`（唯一所有者回到组件库，并把 `display:flex;align-items:center;gap` 三行**搬**过去，面板标题里确实要放徽标）；`web/src/components/index.tsx` 删掉三个死钩子 + Radio 的 `fieldset--${state}`（禁用视觉本来就由内层 `:disabled` 表达，外层没有可表达的东西）；新增 `web/src/routes/ComponentsPage.css`（只给版面，颜色/描边/间距全走令牌）。
- **`tools/check-css-grid.ps1`**：**含 0 宽轨道的 `grid-template-columns`，同一个样式表里必须有显式落格**（`grid-column` / `grid-column-start` / `grid-area`，且条数不少于折叠声明的条数）。它就是钉 bug ② 的。逃生口**写在 CSS 里**（声明上方的注释包含 `qul-grid-auto-ok`）而不是一张 allow-list。
- **两条都做过人为踩红**（演示完逐字节还原）：
  - 把 `Shell.css` 里四处 `grid-column` 注释掉 ⇒ `FAIL` 并**指名 `routes\Shell.css:62`**（`grid-template-columns: var(--shell-rail-w) 0 1fr`），exit 1；
  - 在 `styles.css` 里重新加一条 `.panel` ⇒ R1 报出**两个所有者**（`components\components.css:440` / `styles.css:178`）；
  - 在 TSX 里写一个 `zzz-not-defined` ⇒ R2 指名 `routes\ComponentsPage.tsx:50`。
- 两条已注册进 `tools/verify.ps1`（`ladder` 与 `clean` 之间）：**13 → 15 项**。

### 5. 两条如实记下的话

- 我先前说过"这两个 bug 这条检查都能拦"，**那是过头话**：类名检查拦得住 bug ①，**拦不住 bug ②**（网格自动落格）—— 所以才有第二条检查。
- **"检查全绿"与"界面是对的"是两件事。** 这两个 bug 恰好穿过我们全部的自动化：jsdom 没有布局引擎，`tsc`/`eslint` 只解析不排版，`tokens`/`ladder` 只读令牌数字，`impeccable` 只认反模式。**只有人眼看真窗口能发现它们** —— 这正是用户说"我觉得应该先确立界面"的实证。

### 6. 本轮的收尾数字（逐项单独跑，绕开 `pnpm run`）

`tsc --noEmit` **0** · `eslint .` **0** · **vitest 281 项 / 18 个文件全过** · `vite build` **1.41 s** · `impeccable` **零命中** · `tokens` OK（9 文件 / 128 个定义）· `ladder` OK · `docs -Strict` OK（22 文件 / 634 条标题）· `audit` **P0 / P1 / P2 全 0**（27 条 promise 候选留给人看）· 两条新检查 OK。

⚠️ **没有跑整个 `verify.ps1`**：在本机它会让 `web` 项失败**并把 `node_modules` 拆成半个**（下节的环境事实），所以这一轮按项单跑。

⚠️ 又踩了两次项目早就记过的纪律：写 JSX 注释时在 `return (` 之后用 `{/* … */}` ⇒ 两个孩子 ⇒ `TS1005: ')' expected`（`web/src/routes/Shell.tsx:79-81` 早就记着这条）；而写这段说明的注释时又踩了第二次 —— **注释里原样写出块注释的结束符号，提前关掉了整段注释**。

### 7. 这一轮还没做的

- **主页按设计稿补纯 UI 缺口**（`docs/_artifacts/home-final.png` / `docs/UI设计规格.md:730` §4.6.8）：插件头部的 **`＋添加插件` / `⚙插件设置`** 两个按钮（`web/src/home/HomePage.tsx` 自己的 ASCII 图与"三条不动"表都写着它们，JSX 里没有）、横幅缺"画面"层、插件清单只有 5 件（规格是七件，缺**我的分组**与**产品状态**）。
- **`Win + ←/→/↑` 贴靠仍待人验**（M4 第四条验收，规格 `docs/UI设计规格.md:850` 的降级线）。
- `docs/UI设计规格.md:373` 的 §4.5 正文（空标题）与 L842 右键系统菜单的处置，**待用户拍板**。

---

# 上一轮（2026-10-02 上半场）：把 `SESSION.md` 补到 HEAD

> **那一轮做的不是功能，是把一个文件补到 HEAD。** 补的过程中发现它 3697 行里有 **33 份逐字节相同的副本**（见文末「副产物」一节）；**唯一内容一段没丢** —— 四段逐字保留在文末「历史存档」，其余重复件已删除。
>
> 另外两件如实记下的事：① 本机 pnpm 12 会让 `verify.ps1` 的 `web` 项失败，**并且把 `node_modules` 拆成半个**（原因不在代码，见「环境事实」）；② 顺着 `audit` 的一条 P1 查出 `docs/UI设计规格.md` §5.4 的子节**编号撞了真正的 §5.5**（已修，见「那一轮顺手修掉的一个真缺陷」）。**在修 ② 之前，`audit` 的 P1 是 2；修完是 0。**

- 那一轮结束时的 HEAD = `a7232ca`（`SESSION.md` 重写 + §5.4 编号修正，143 增 / 3435 删，**未推送**）；它下面那个提交是 `faa7fa1`「标题栏的两条规格：**双击最大化**（而它抓出一个真 bug）+ **排除交互元素**」。
- 本文件上次更新曾停在 `20a5098`（"M4 主干五块完成 + Tauri 安全基线已强制（verify 11 项）"）。它当时说的三件事——"`src-tauri/` 还不存在""灵动岛未接线""verify 11 项"——**现在全部已被推翻**；落后期间落地 **16 个提交**，而它们**只动了代码与工具**，没有动这个文件。

## 上一轮做到哪：M4 那 16 个提交

这 16 个提交（旧 → 新）分四组：**M1 收尾 → 编排层搬家 → M4 主链路 → M4 四条验收**。

| # | 提交 | 它做了什么 | 值得记住的一条 |
|---|---|---|---|
| 1 | `0316d00` | M1 收尾：`src-tauri` 从"只有配置"变成**真的能构建**（`cargo build --offline` exit 0，8.41 s），§5.8 安全基线在真外壳上逐条通过 | Tauri 那棵树的代价**量出来了**：新增 crate ≈ **210** 个（工作区 18 → 218）、`src-tauri/Cargo.lock` **417** 包、`target` 1521 MB（已 gitignore）、首次 debug 构建 **36.7 s**、**不需要 C 工具链**；许可证里出现 **5 个 MPL-2.0**（`cssparser` / `cssparser-macros` / `selectors` / `dtoa-short` / `option-ext` ← `dom_query` ← `tauri-utils` ← `tauri-build` ← `tauri`）。**产品的 `Cargo.lock` 仍是 18 个包**——隔离成立，台账进 `docs/来源记录.md` §5 |
| 2 | `009dc6c` | 前端 `web/src/api/` 从"边界层 + 桩"换成**真的 `invoke`**；§5.8 的"组件里连拿到 `invoke` 的机会都不该有"现在有对象可测、也能被踩红 | 拆四层单向依赖 `api/contract.ts` → `api/backend.ts` → `api/tauriBackend.ts` → `api/index.ts`：**TypeScript 对循环 import 不报错**，症状是某个绑定 `undefined`，而错误**指不到循环本身**。另：`invoke` 的构造参数是 camelCase 而 Rust 侧是 snake_case，用错名字只得到 "missing required key instanceId"（**只说了一半**的错误） |
| 3 | `b831d17` | 灵动岛接线**撞到墙**：`qul-app` 是空壳 | 这一轮**最重要的产出是没有做的事**——没把 `install_cmd.rs` 的编排复制进命令层（那会立刻产生两份策略、两份进度文案，**而它们会漂移**）。墙的位置写成 `docs/灵动岛接线-边界分析.md`（8 节）：`Channel<TSend>` 而不是全局 `app.emit`（后者作用域是**所有窗口**，"两个安装的进度混在一条全局事件流里"会在**消息层**破坏 §7.1，而内核 22 条队列测试**管不到消息层**）；`Stage::key()` 与 `LaunchStage::key()` **逐字相同** ⇒ 映射恒等 |
| 4 | `91ca58f` | 编排层接住**安装策略**：`crates/qul-app/src/install_plan.rs` 的 `descriptor_for`，CLI 与界面**共用同一份** | `crates/qul-app/tests/layering.rs` 里**有意放松**一条禁令（允许 `qul-app` 依赖 `qul-infra`，附 **19 行理由**）：禁令原文的理由是"依赖 `tauri` 之后命令行想用同一份逻辑就得把 Tauri 也拖进去"，而**实测 `qul-cli` 本来就依赖 `qul-infra`** ⇒ 那句话对它**没有内容**；`tauri`/`wry`/`tao`/`tokio`/`reqwest` **仍在**禁列。四条策略：先本机缓存再联网 / transport 由调用方给（让"离线模式"成为**一个参数**而不是一份分支）/ 要不要装资产 / **失败也要走同一个 sink**（"失败就 `return Err`"会让界面**停在最后那个进度上**，而 §7.2 的 `Error` 是灵动岛的一个状态） |
| 5 | `eae2092` | 编排层接住**资产索引**：`ensure_asset_index`（四个分支 + 两种"跳过"） | "离线时没有资产"是一条**产品判断**：游戏能起，只是没声音没语言 ⇒ `Ok(None)` **跳过而不是失败**。⚠️ 顺序不能反——**索引自己也是要下载的文件，先下下来再解析它**。⚠️ 三个数（逻辑名数 / 不同哈希数 / 去重后字节数）**必须一起返回**：`26.3` 上那两个数**恰好相等（5147 = 5147）**，只返回第一个数会**悄悄丢掉去重** |
| 6 | `9e0a47b` | 编排层接住**主体**：`install_to_instance` + 布局（`instance_dir` / `default_data_root`） | 判据是**"零策略"而不是"零 `qul_infra::`"**。`install_cmd.rs`（450 → 419 → 358 → **361** 行）里只剩 **3 处** `qul_infra::`，全是 transport 选择（`NoNetwork` / `WinHttpTransport` / `PlainHttpTransport`）；把它们也搬走会让"离线模式"从**一个参数**变回**一份隐藏的分支**。⚠️ `Path::new(&env::var(..).unwrap_or_default())` 会拼出**相对路径**，数据落在**进程的 cwd**（**实测发生过一次**：`qul install` 之后 `logs/` 出现在仓库根目录） |
| 7 | `db54fcc` | **灵动岛通了**：内核 `StageSink` → `Channel` → 前端八态。`src-tauri` 有了 `ChannelSink`（**纯翻译**）+ `install` / `cancel_install`；前端有了 `web/src/island/bridge.ts` | `send` 失败**静默**——那只意味着**前端已经不在了**，安装不该中断（§7.3 约束 4：可关闭但任务不丢失），**真正的取消只走 `cancel_install`**；`speed_bps` / `eta_secs` 传 `None`——编一个 `0` 会让 **"0 B/s" 与"下载卡住了"在视觉上无法区分**；`resident_bytes` 传 `None`（**基岩版没有这个概念**） |
| 8 | `0ece286` | `Channel` → `useIslandQueue` hook；**抓到一个真 bug** | `toIslandContent` 的 `switch` 没有 `default` ⇒ 认不出的 `kind` **静默返回 `undefined`**，而 React 的 `useState` 接受它 ⇒ 界面而后炸在一个**指不到这里**的地方。修法是 `default: { const never: never = s; throw new TypeError(...) }`——`never` 那个赋值**只保证编译期**，而这是一个**边界** |
| 9 | `d41129c` | **首次真的构建出可运行的桌面应用**：`qul-desktop.exe` **3.84 MB**、窗口「**秦墨**」、NSIS `秦墨_0.0.0_x64-setup.exe` **1.80 MB**、release 构建 **1 分 32 秒**（`--no-bundle`） | 之前缺两样（没装 `@tauri-apps/cli`、`bundle.icon` 是 `[]`）⇒ **"我配好了" ≠ "它能启动"**。⚠️ `git check-ignore -q` 的退出码是**假信号**（`-q` 与"路径以斜杠结尾"的组合）——**要结论，就用会打印结论的那个命令** |
| 10 | `03f2cd7` | **全流程一次点通**：主页「启动 26.3」→ `startInstall` → 内核 → `Channel` → `bridge` → `useIslandQueue` → `Shell` 里的岛（`web/src/island/IslandProvider.tsx`） | `impeccable` **拦了一次而它是对的**（`border-inline-start: 3px solid` 是 AI 生成界面最明显的特征之一），但"直接删掉"会违反 §5.4.3"信息性的用法不能只靠颜色分辨"⇒ **找到同时满足两者的第三种形态**（一圈细边框 + 一个 `::before` 实心点）。另：引用了**不存在的类名** `page__error`——**"我写了一个类名 ≠ 它真的存在"**。另：`IslandProvider` 必须包住 `RouterProvider`（**`RouterProvider` 的子树不继承外层 context**） |
| 11 | `5a902cc` | 自绘标题栏（§4.5：**36 px** 高、控制按钮 **46 px** 宽；`decorations: false` 的后果）；顺带挖出一个**静默的既存缺陷** | `--font-weight-medium` / `--font-weight-semibold` **13 处使用 / 0 处定义**（`--line-height-tight` 同病）：自定义属性未定义时**整条声明失效**，字重回落到 `normal`，而 `normal` 与 `medium` 在 **13 px 的中文上几乎看不出来** ⇒ `tools/check-css-tokens.ps1`（含两条反证），verify **11 → 12 项**。⚠️ **"我手工核对了一次"不是方法，是运气**。⚠️ 窗口控制走**自定义命令**（`window_toggle_maximize` 是 toggle 并返回切换后的状态——`maximize()`+`unmaximize()` 会让前端决定做哪一个 ⇒ 两次 IPC 之间有**竞态**），**不是** `core:window` 权限 |
| 12 | `24f8185` | M4 验收 ①：**冷启动期间零网络请求**（四通道记录器，含 `PerformanceResourceTiming` 的 `buffered: true`，能看见装记录器**之前**的加载） | `web/src/main.tsx` **第一行**装记录器、`render` 前 `assertNoColdStartNetwork()`，而它**不抛**只 `console.error`——"因性能问题让应用崩掉是**更坏**的取舍" |
| 13 | `f3fd6a2` | 用户推荐的两个仓库：`guillermolg00/morphicons` ✅ **采用** / `zhangjw-THU/Emoji` ❌ **不采用** | 采用理由**只有一条**：它**插值 SVG 的 `d` 属性**，而 `framer-motion` **不插值路径**（MIT、0 运行时依赖、+18.29 KB / gzip +8.37 KB、零 CSS 副作用）；测试断言渲染出的 `d` 里**含有我们自己写的坐标**（`M4 7` / `M4 12` / `M4 17`）⇒ "顺便用了它自带图标集"的实现会红。不采用的三条独立理由：**无许可 ⇒ 不可查**、内容是 GitHub 的素材、徽章指向另一个仓库（衍生）。**"MIT" 不是"可以直接用"**，后续推荐照样要过许可 / 依赖数 / 体积三条 |
| 14 | `61a259b` | M4 验收 ③：**材质阶梯可指认**（`tools/check-material-ladder.ps1`：面板 8–12% / 卡片高于面板 / 浮层不透明 / `--hairline` 正好 1 px（2x DPI 下 0.5 px）/ 每档 `--space-N` 是 4 的倍数 / 间距单调递增） | 四个反证**全部踩红**（卡片降到 5% / 描边 1.5 px / 间距 6 px / 浮层半透明）；它只读 `tokens.css` 的**第一个 `:root` 块**（浅色主题的 `--surface-card: rgb(255 255 255 / 72%)` 不在区间内）；verify 12 → 13 项 |
| 15 | `ec183e8` | M4 验收 ②：**键盘与读屏**（`web/src/routes/Shell.a11y.test.tsx` 6 项 + `scrollTo` 桩） | ⚠️ **`Win + ←/→/↑` 贴靠要人验**（测试只能钉住前提：`resizable === true` / `decorations === false` / `maximizable !== false`）；教训：**探测桩不能靠名字或字符串**——jsdom 确实装了 `scrollTo` 且它是**真函数**，只在运行时报 `Not implemented` |
| 16 | `faa7fa1` | 标题栏两条规格：**双击最大化/还原**（而它抓出一个真 bug）+ 排除交互元素 | 三个窗口控制只排除了 `pointerDown`、**没排除 `doubleClick`** ⇒ 双击「关闭」冒泡到标题栏 ⇒ **先最大化再关闭**（**排除了一种事件、漏了另一种**）；`disabled` 的搜索框**仍会收到 `pointerdown`**（disabled 拦的是 click 与 focus） |

**测试与构建的当前位置**（上一轮在本机逐项重跑过）：前端 **281 项 / 18 个文件** 全过（5.60 s）· `tsc --noEmit` 0 · `eslint .` 0 · `vite build` 1.56 s（CSS **37.61 kB**、JS **526.11 kB** / gzip 172.44 kB）· Rust **717** 项 · `clippy -D warnings` 0 · `impeccable` **零命中** · `docs` / `audit` / `tokens` / `ladder` / `tauri` / `vocab` / `vocab-probe` / `fmt` 全 OK · 产品 `Cargo.lock` **18** 包 · `src-tauri/Cargo.lock` **420** 包 · 应用 `qul-desktop.exe` **3.90 MB**（这一轮重建后是 **4,088,320 字节**）/ 安装包 `秦墨_0.0.0_x64-setup.exe` **1.85 MB**。`verify.ps1` 现在是 **15 项**（这一轮加了 `css-classes` 与 `css-grid`）。

⚠️ **`tools/verify.ps1` 的整跑在本机有两个独立麻烦**：上一轮（13 项时）实测 `11 ok, 1 failed, 1 skipped`（exit 1），**失败的是 `web` 而原因不在代码**（见下面「环境事实」里的 pnpm 版本那条），而且**跑它会把 `node_modules` 拆成半个**（要靠 `pnpm dlx pnpm@10 install --frozen-lockfile` 修回来）——所以这一轮**没有整跑**，两条新检查是按项单跑 + 人为踩红的。另外 `test` 一项是**第二次才过**（脚本自己标 `OK*` 并警告 *"A retry that succeeds is a transient failure, not a passing test"*）——**本机 Rust 套件那次瞬时失败的原因还没查**，这条不该被忘掉。

## M4 的四条验收条件（方案 §8 逐条）

| 条件 | 状态 | 证据 |
|---|---|---|
| 全流程一次点通 | ✅ | `03f2cd7`：主页按钮 → `startInstall` → 内核 → `Channel` → `bridge` → `useIslandQueue` → 岛。⚠️ `HomeRoute.test.tsx` 钉的是**接线**；那条链本身由 `bridge` / `useIslandQueue` 的测试负责 |
| 冷启动期间不等待任何网络请求（§7 口径） | ✅ | `24f8185`：`web/src/boot/coldStart.ts` 四通道记录器 + `main.tsx` 启动前断言；`coldStart.test.ts` 第 ③ 组是**验收**（import 应用模块图断言求值不发请求，并反证 `console.error` 会喊） |
| 质感验收五类**零命中** | ✅ | `impeccable`（`repos/impeccable`，61 条确定性规则）已接进 `verify.ps1`，每次跑；`03f2cd7` 那次它**真的拦下了一个**形状（而修法不是加豁免） |
| 键盘与读屏通过 | ⏳ **差一条人验** | `ec183e8` 的四件可验性；第 ④ 件（`Win + ←/→/↑` 贴靠）**jsdom 测不了**——没有窗口管理器 |
| 三层材质的不透明度阶梯 / 1 px 描边 / 留白节奏**可指认** | ✅ | `61a259b` 的 `ladder` 检查：**它把三个数字打出来并核对**，不是"感觉还行" |

**另外两件 M4 正文的事还没做**（都不是编码问题）：

1. **`docs/UI设计规格.md` §4.5 是一个没有正文的标题**（L373 → L375 直接跳 §4.6），实际内容散在 L270 图 / L288 表格行 / L834–L857 三小节（拖动与命中区 / Snap Layouts / 高 DPI 与多显示器）。**补哪一节是需要用户确认的设计决定。**
2. **§4.5 L842「空白处右键 → 系统窗口菜单（移动 / 大小 / 最小化 / 关闭）」没做。** 代价要 `TrackPopupMenu` / `SendMessage(WM_NCRBUTTONUP)`，即碰 `WM_NCHITTEST` / 非客户区那一层，需要 `SetWindowSubclass` 子类化（一处**能弄坏输入**的改动）——**下一轮先量代价再决定，不先写**。

## 下一步第一件事

**M5（微软身份）之前，先把 `capabilities` 与 `instance_summary` 从骨架接上真内核调用**——它们现在返回硬编码的一份，而 `qul-core` 已经有 23 个模块可以支撑真的结论。（这是 `03f2cd7` 自己留下的待办。）

顺带清掉 M4 的三件尾巴：**人验 `Win + ←/→/↑`**（三步约 20 秒）· **§4.5 补正文**（等用户拍板）· **L842 右键系统菜单**（先量代价）。

## 卡在哪

| 卡点 | 性质 |
|---|---|
| **`Win + ←/→/↑` 贴靠** | **等人验**：jsdom 没有窗口管理器，这条只能人跑（打开秦墨 → 点标题栏空白处 → 按 `Win + ←` / `Win + →` 应贴到屏幕左右半边）。不动则 §4.5 的降级线未达到，要查 `WM_NCHITTEST` 那条路 |
| **§4.5 的正文** | **等用户拍板**：规格里 §4.5 是空标题，补哪一节是设计决定 |
| **L842 右键系统菜单** | **未做（不是"不用做"）**：代价未量，且要碰非客户区（`SetWindowSubclass`） |
| **M4.5 百宝箱** | 未开工：工具集合骨架 + 首批 6 件（`docs/方案-重新立意版.md:1192`） |
| **natives 解压没有架构感知** | 承 M3 的已知偏差：官方部署 **12 个文件（只有 x64）**，我们解出 21 个（x64 **与** arm64）。功能上是对的（`-Djava.library.path` 指向架构对应子目录，arm64 那些**永远不会被加载**），但**与官方逐文件比对还没做** |
| **S7 第 5–8 步 / S10 / Mojang 审批** | 承 M0：分别被"离线身份未实现"（**现已实现，这一条可以重开**）、"只有这一台电脑"（环境降级，已如实标注）、"已提交但无编号、无进度面板"卡住 |
| **`verify.ps1` 在本机有一条副作用** | **有解，但需要一次决定**：它的 `web` 项会因 pnpm 12 的 24 小时策略失败，并把 `node_modules` 拆掉（修回来是 `pnpm dlx pnpm@10 install --frozen-lockfile`，几秒）。根治是给 `package.json` 补 `packageManager` + `engines`（或把 pnpm 降到 10）—— **那是改仓库约束，留给用户拍板** |

## 这一轮顺手修掉的一个真缺陷：§5.4 的子节编号与真正的 §5.5 **撞号**

- **症状是"写下的那一刻就指不到"**：`docs/UI设计规格.md` 的 §5.4（视觉强度阶梯）下面那五个子节，此前被编成 `5.5.1`–`5.5.5`，而**紧接着就是真正的 `### 5.5 日志分级色（令牌化）`**（现 `docs/UI设计规格.md:1069`）。父节是 5.4、子节却叫 5.5.x ⇒ 任何按节号写的引用都落空；`tools/audit-milestone.ps1` 一直把它记成 **P1 dangling**（原文：`section 5.4.1 has no matching heading`）。
- **它是怎么被发现的**：新版 SESSION.md 里引了 `§5.4.3`（承 `03f2cd7` 的原文），于是 `audit` 的 P1 从 **1 变 2** —— 顺着这条 P1 去查，才看出这不是"我引错了号"，而是**规格自己编号撞号**。它与 `5a902cc` 那个"13 处用了、0 处定义"同属一类：**出错的地方不报错，报错的地方在别处**。
- **修法与安全边界**：五条子节标题改成 `5.4.1`–`5.4.5`，**标题文字一字未动**。改之前先 grep 全仓（排除 `target/` `node_modules/` `repos/` `_archive/`）确认 `5.5.x` **只出现在这五行标题里**，别处无人引用 ⇒ 改编号不会连带打断别的引用。`docs/.heading-baseline.json` 记的是**每个文件的标题条数**（不是标题文字），条数没变 ⇒ 不需要 `-UpdateBaseline`。§5.4 标题下留了一条说明它为什么被改。
- **结果**：`audit` 的 **P1 从 2 → 0**（P0 / P2 也都是 0）、`check-docs.ps1 -Strict` 仍 OK、`docs` 总账不变（22 个文件 / 634 条标题）。

## 本轮发现的环境事实（会影响开工命令）

- **本机没有 PowerShell 7**：`$PSVersionTable` = **5.1.26100.9444 Desktop**，`pwsh` 不存在（`C:\Program Files\PowerShell\7\pwsh.exe`、`WindowsApps\pwsh.exe` 都没有）。README 与各里程碑里写的 `pwsh -File tools/verify.ps1` **在本机跑不了**，要用：
  `powershell -NoProfile -ExecutionPolicy Bypass -File tools\verify.ps1 -AllowDirty`
  （`-List` 已在 5.1 下验过：正常解析、exit 0、每项都带 "protects:" 说明；现在列 **15 项**。）
- **`clean` 项现在必然失败**：工作区有一个未跟踪的 `docs/_artifacts/qinmo-layout.png` ⇒ 要么先 `-AllowDirty`，要么把它提交 / 加进 `.gitignore`。（`diag.pdb` 在仓库根但**已被忽略**，`git status --porcelain` 看不到它。）
- 工具链坑：本机 .NET Framework 的 `[System.Security.Cryptography.SHA256]` **没有 `HashData` 方法**，要用 `::Create().ComputeHash(bytes)`；`Get-FileHash` 也不接受从管道传进来的字符串。
- 探索坑：在仓库根 `glob *`（不带路径分隔符）会返回 **72634 条**（`target/`、`repos/`、`_archive/` 的构建产物），没有信息量——要看目录就用目录列举。
- 🔴 **pnpm 版本漂移：本机是 `12.8.1`，而 README 要求 `pnpm 10`** —— 而 `package.json` 里**既没有 `packageManager` 也没有 `engines`**，所以这次漂移**没有任何东西拦**。pnpm 12 的内置默认 `minimumReleaseAge = 24h`（**不是仓库的设置**：`pnpm config get minimum-release-age` 返回 `undefined`，`pnpm config list` 里也没有这一项）会拒收 lockfile 里近 24 小时发布的条目；实测被拒 **7 个**：`@csstools/css-calc@3.4.2` / `@csstools/css-color-parser@4.2.5` / `@csstools/css-parser-algorithms@4.0.2` / `@csstools/css-syntax-patches-for-csstree@1.1.15` / `framer-motion@13.5.0` / `motion-dom@13.5.0` / `motion-utils@13.5.0`，报错原文 `Error: ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION`。
- 🔴 **它的后果比"一条检查变红"严重**：pnpm 12 在 `pnpm run` 之前会做一次依赖校验（`verifyDepsBeforeRun` 默认 `install`）⇒ `pnpm run verify:web` **在跑 typecheck 之前就退出**，而那次失败的 install **把 `node_modules` 拆成了半个**（顶层只剩 `.pnpm/` 等 4 项，**`.bin` 与 `.modules.yaml` 都没了** ⇒ 前端在修回来之前根本跑不起来）。修法（本轮实测两条）：`pnpm dlx pnpm@10 install --frozen-lockfile`（**3.6–5.8 秒**，走本机 store）恢复，或等那 7 个版本各自满 24 小时（最晚 **2026-10-02T11:34Z** 之后自愈）。**不要在没问过用户的情况下把 `minimum-release-age=0` 写进仓库 `.npmrc`** —— 那是把一条供应链护栏拆掉。
- pnpm 12 还**整体忽略** `package.json` 里的 `pnpm.onlyBuiltDependencies`（WARN 原文：`The "pnpm" field in package.json is no longer read by pnpm. The following keys were ignored: "pnpm.onlyBuiltDependencies".`）⇒ "只允许 `esbuild` 跑安装脚本"这条护栏在 pnpm 12 下**静默失效**（要恢复得写进 `pnpm-workspace.yaml` / 新的 settings 位置，而那个文件现在**不存在**）。
- 绕开 pnpm 的依赖校验、直接跑前端的办法：`.\node_modules\.bin\{tsc,eslint,vitest,vite}.cmd`（实测四项全 0）—— ⚠️ PowerShell 里**必须带 `.\` 前缀**，写 `node_modules\.bin\tsc.cmd` 会被当成模块名（`无法加载模块"node_modules"`）。
- Node 是 **v24.21.0**（README 要求 ≥ 22 ✓）；`.npmrc` 把 registry 指向 `https://registry.npmmirror.com/`（仓库注释记录：直连 npmjs 时**单个元数据请求要 240 秒**）。
- ⚠️ **PowerShell 5.1 的 `Get-Content` 默认按系统代码页（936）解码** ⇒ 直接读 UTF-8 的 `.npmrc` / `package.json` / 本文件会得到**乱码**，要显式带 `-Encoding UTF8`。反方向的坑同样存在：**工具写出的 `.ps1` 没有 BOM，而 5.1 也按 936 解析它** ⇒ 脚本里的中文串会乱码**并直接造成 ParserError**（本轮写装配脚本时踩到；最后改成"脚本纯 ASCII，中文标题从 UTF-8 文本文件读进来"）。被处理的文件本身仍是 UTF-8 无 BOM。

## 副产物：这个文件曾经 **96% 是重复的**（已去重）

- 3697 行按 `^# ` 切出 **37 块，而只有 9 块唯一**：同一个"收工状态"块被逐字节复制了 **33 份**（`x16` / `x8` / `x4` / `x2` / 单例…），SHA-256 完全相同。**96% 的行是重复的**——很可能是"每一轮把上一轮顶部的块再抄一遍"这个写法造成的。
- **唯一、不可丢的内容只有四段**，已逐字保留在文末「历史存档」：M4 五块 + 门禁六项 + 质感验收（08 轮）· M3 达成（证据是游戏自己写的 `.minecraft/logs/latest.log`）· `qul launch` 的两个 bug · M3 安装取证 + M0/M1 状态 + M1 交付物清单 + 三层守卫 + S7 事实附录。
- ⚠️ 顺手发现两处**格式缺陷**，补的时候都没有沿用：① M3 达成那条标题在原文里被拆成**两个相邻的 `#` 行**（`# 🎉 **M3 达成** …` / `# 「CLI 能完成…」**它闭合了**。`）——已合成一行；② 另有一份 M4 块的**早期变体**被粘在 `qul launch` 那一段前面，那 48 行与保留的 M4 块**逐字节相同**，已删。
- 复现这条检测：按 `^# ` 把文件切块 → 对每块 `[System.Security.Cryptography.SHA256]::Create().ComputeHash(...)` → `Group-Object`。"重复度"就是"块数 / 唯一块数"。

---

# 历史存档 · 使用说明

> 下面四段是**逐字保留**的唯一内容（各轮的收工记录）。**段内的"下一步第一件事"与"卡在哪"都是那一轮的状态**，可能已被后来的提交推翻——要看当前状态，请读本文件最上面那一节。

# M4 主干：**五块完成**，而 `verify.ps1` 从 8 项长到 **11 项**

## 一、M4 主干已完成的部分

| # | 落点 | 关键约束（规格原文） |
|---|---|---|
| 1 | **灵动岛的状态机**（内核 `crates/qul-core/src/island.rs`） | §7.1「**只有一个岛、一个主状态**」· §7.3「`Error` 可抢占，**其余按到达顺序**」 |
| 2 | **灵动岛的视图**（`web/src/island/`） | §7.2 八态 · §7.3 两态切换**只用 `transform`+`opacity`** · 进度节流「**≥100 ms 或 ≥1%**」 |
| 3 | **主题/材质/强度四态**（`web/src/appearance/`） | §5.4.1「**向下只累加收紧，不叠加放开**」· 「`Enhanced` 不生效，**且不报错**」 |
| 4 | **日志抽屉**（`web/src/logs/`） | §5.5「用 `meta` 区分来源」· 「**默认不上传**」· `Kill Minecraft` **带二次确认** |
| 5 | **主页插件网格**（`web/src/home/`） | §4.6.8 三条硬规则，尤其「**关掉 = 主页不显示，但设置里仍然列着**」 |

**测试：前端 98 → 219 项 · Rust 684 → 706 项。全部绿。**

## 二、Tauri 安全基线：§5.8 现在是**强制的**

`src-tauri/`（**纯配置，零依赖**）+ `tools/check-tauri-baseline.ps1`。

⚠️ **而四个反证全部踩红**（CSP 加 `unsafe-inline` / capabilities 给 `fs:default` /
html 加远端脚本 / Cargo.toml 加 `tauri-plugin-fs`）——
**一个从没被踩红的检查器与一个永远返回 OK 的检查器无法区分。**

## 三、M4 的验收条件现状

| 条件 | 状态 |
|---|---|
| 全流程一次点通 | ⏳ 等灵动岛的队列接线（进度上报还没有来源） |
| **冷启动期间不等待任何网络请求** | ✅ **静态可证** —— `web/src` 里 `fetch(`/`invoke(` **只在注释里出现**；`main.tsx` 零取数 |
| **质感验收五类零命中** | ✅ **已接进 verify**，每次跑 |
| 三层材质的不透明度阶梯可指认 | ✅ 令牌已就位（`--surface-panel/card/raised` + 四态） |

## 四、⏭ 还差什么（按依赖顺序）

1. **灵动岛的队列接线** —— 需要"进度上报"的来源（`install.rs` 的 `StageSink`
   已经在，而"把它推到前端"要 `src-tauri` 的命令层）
2. **`src-tauri/src/main.rs` + `Cargo.toml`** —— 要 Tauri CLI 与**一棵依赖树**，
   而本仓库现在只有 **18 个 Rust 包**。**这是一个需要你拍板的决定**（见下）
3. **M4 的"全流程一次点通"** —— 它硬依赖 1 与 2

## 五、⚠️ 下一轮会遇到一个**需要你拍板**的事

**"加 Tauri"意味着给一个现在只有 18 个包的工作区加一棵依赖树。**

这与之前那次 TLS 决定**同一类问题**：它改变的是"我们欠多少维护与许可证台账"，
而不是"代码写不写得出来"。

**所以我会先做的是**：把 Tauri 那棵树**实测量出来**（包数、许可证、构建时间、
体积），再让你选 —— 与上次 TLS 那样，**用数据而不是偏好**。

### ① M4 门禁**六项齐备** ⇒ M4 可以开工

| # | 门禁 | 落点 |
|---|---|---|
| 1 | 令牌文件（四态 + 表现层） | `tokens.css` **116 个自定义属性** |
| 2 | 约 20 个基础组件骨架（6 状态） | `web/src/components/`（20 个组件） |
| 3 | 路由骨架 | `routes/` **23 处 createRoute** + 22 条二级 |
| 4 | 数据层贯通示例 | `api/{index,query,contract}.ts` |
| 5 | 三条 lint 规则（+ S4 边界层守卫） | `eslint.config.js` |
| 6 | 表现层令牌 + `intensity` 两档 | `intensity-*` 令牌 |

### ② 🔴 质感验收**真的跑了**，而它抓到两处

用 `repos/impeccable` 的 CLI（**61 条确定性规则**，Rust 内核）：

| # | 命中 | 我的处理 |
|---|---|---|
| 1 | `[side-tab]` `border-inline-start/end: 4px solid` | **判据在这里是误判**（那两条是透明的），**而我没有加 ignore 规则** —— 有另一个不需要任何 hack 的做法（旋转 45° 的两条边），那就用它 |
| 2 | `[marquee]` `.progress__track--indeterminate` 的**无限横向循环** | **这是真冲突** —— §2 约束 7 **明确禁止无限循环动画**。改成"整条填充 + 低不透明度"，而"它在动"由**文案**回答 |

**两处都不是靠豁免解决的，而理由写在了代码注释里**：

> `repos/impeccable` 是 M4 质感验收的**指定工具**（§8 的 M4 行），
> 而"遇到命中就加豁免"会让那条验收
> **从「零命中」退化成「零命中减我加的那些豁免」**。

**现在 `detect web/src` 与 `detect web/dist` 都是 exit 0（零命中）。**

### ③ 而它已接进 `verify.ps1` —— 所以下一个人不会漏掉它

`tools/verify.ps1` 从 **8 项变成 10 项**：

```
fmt · clippy · test · vocab · vocab-probe · docs · audit
  · web（前端 typecheck/lint/测试/构建，一条命令）
  · impeccable（UI 反模式检测器）· clean
```

⚠️ **每一项都带"它保护什么"**，而 `impeccable` 那一行写的就是 §8 的 M4 验收原文
（通用 AI 味配色 / 紫色渐变 / 发光粒子 / 玻璃拟态堆砌 / SaaS 落地页套路
**五类必须零命中**）。

### ④ 验证

- **`tools/verify.ps1`：9 ok / 0 failed / 1 skipped（clean）** —— 10 项
- 前端 **98 项测试全绿** · typecheck 0 · lint 0 · build 成功
- Rust **684 项全绿** · clippy 0 · fmt 通过
- **`pnpm.onlyBuiltDependencies` 仍是 `["esbuild"]`**

### ⚠️ ⑤ 自动轮次已用尽（40/40），而目标未达成

`get_goal` 报 `roundsStarted: 40` = `maxGoalRounds: 40`。

**已达成**：M0 · M1（可自验部分）· M2 · **M3** · **M4 的前置门禁六项**
**未达成**：**M4 界面主干**本身（岛式布局 / 灵动岛 8 态 / 导航内容 / 主题四态 / 日志抽屉），
以及 M1 的剩余两项（**Tauri 安全基线 + 前端命令层门禁** —— 它们硬依赖 `src-tauri`，
而 `src-tauri` 还不存在）。

### ⏭ 下一轮（需要你重启目标或新开会话）

**M4 界面主干**。而它的验收条件里有两条现在就能说清：

1. 「**冷启动期间不等待任何网络请求**（§7 口径）」—— 结构上已成立
   （挂载时零请求；数据按页取），而**要有一次实测**。
2. 「**质感验收**：五类必须零命中」—— **已接进 verify**，所以它会在每次验证里跑。

**而 M1 的 Tauri 安全基线要建 `src-tauri`** —— 那是 M4 开工后第一件该做的事
（它是 M1 的最后两项，而它们挡着"M1 完全闭合"）。

# 🎉 **M3 达成** —— 方案 §8 的 M3 出口条件是「CLI 能完成一次真实安装 → 部署 → 启动全链路」，**它闭合了**。

## 而证据是游戏自己写的日志

```
[02:01:40] [Render thread/INFO]: Environment: Environment[… name=PROD]
[02:01:40] [Render thread/INFO]: Setting user: qinme          ← **离线身份生效**
[02:01:40] [Render thread/INFO]: Backend library: LWJGL version 3.4.3+4
[02:01:41] [Render thread/INFO]: Reloading ResourceManager: vanilla, vanilla
[02:01:43] [Render thread/INFO]: OpenAL initialized on device OpenAL Soft on 扬声器
[02:01:43] [Render thread/INFO]: Sound engine started
[02:01:43] [Render thread/INFO]: Created: 2048x2048x4 minecraft:textures/atlas/blocks.png-atlas
```

**进程活着 10 分钟以上**（内存 1028 MB / CPU 95.7 s），而不是前两次的 1.3 秒崩溃。

### 全链路的三个数字

| 环节 | 实测 |
|---|---|
| 真实安装（联网） | **76 文件 / 129.8 MB / 145.7 s / 缺口 0** |
| 资产迁移（**离线，零联网**） | **5147 个对象 / 461.4 MB / 62.9 s / 每个都过 SHA-1** |
| 启动 | **到主菜单**（`Setting user` + 音频 + 贴图集） |

## 本轮修掉的四个真 bug

| # | bug | 症状 |
|---|---|---|
| ① | `inventory()` 内部**写死了不带资产的 `required_files`** | 索引解析出 **5147 个对象**，而流水线说**需要 76 个** —— 于是"0 缺口"那句话是**错的** |
| ② | 迁移被放在 `offline_only` **之后** | `--offline --from <官方目录>` **一个文件都没搬** |
| ③ | `cwd` 用的是 `resolved.cwd`（**从没被设过**） | 游戏在**仓库根**跑，`logs/` 写到那里，而 `git add -A` 把它提交了 |
| ④ | `--from` 被插进了 **`launch` 子命令** | 编译错误（而它是我的替换打偏了） |

## ⚠️ 而 ③ 的验证**还没做**

我把它改成 `cwd: Some(game_dir.display().to_string())` 了，而**没有再跑一次游戏确认
`logs/` 落在实例里**。所以它有**编译保证**，没有**实测保证** —— 下一轮要补。

## 一条**纠正**：我之前那个"5147 个逻辑名里有重复哈希"是**错的**

实测：**5147 个逻辑名 = 5147 个不同哈希，去重掉 0 个**。
去重逻辑仍然正确且必要（协议允许共享，代价一次 `BTreeMap` 插入），
**但它在这个版本上是空转的** —— 而我上一轮把它写成了实测结论。
代码注释与文档都已改正。

## 下一步：M3 ✅ → **M4 的门禁五项**

M3 已达成，所以按 §8 的关键路径，下一站是 **M4（界面主干）**，
而它开工前需要**门禁五项齐备**。要做的第一件事是**核对那五项的门禁现状**，
以及 **M1 剩余的两项**（Tauri 安全基线 + 前端命令层门禁 —— 它们硬依赖 M4 的前端）。

# `qul launch` 真的把游戏拉起来了 —— 而两个真 bug 是"真跑一次"才发现的

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

# M3 安装与启动取证 · M0/M1 收工状态 · 三层守卫 · S7 事实附录

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
