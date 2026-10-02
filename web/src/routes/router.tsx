/**
 * 路由树（**M4 门禁第 ③ 项**）
 * ============================================================================
 *
 * 单一路由定义处。TanStack Router 的选型理由（`docs/方案-重新立意版.md` §4.5）：
 *
 * > 两段式导航 + 百宝箱二级分组 + 实例详情，路由是"**能力描述符如何被消费**"
 * > 的落点；类型安全路由与 TS 严格模式搭配能**在编译期挡住"跳转到未注册的
 * > 能力页"**。
 *
 * ## 🔴 这里用的不是"文件式路由"
 *
 * TanStack Router 支持文件式（配 `@tanstack/router-plugin` 做代码生成），
 * 而**本项目用代码式** —— 理由有一条，而它是硬的：
 *
 * **门禁⑤ 的 lint 规则要求"界面不许用产品名做判断"**，而文件式路由会把
 * **路径**变成文件名。于是"某个产品专属的页面"会自然长成
 * `routes/java/settings.tsx` —— **而那个文件名本身就是界面认识那个产品的证据**，
 * 且它绕过了 lint（lint 看的是代码内容，不是文件名）。
 *
 * 代码式路由让**路由表是数据**：它是从能力描述符/固定清单来的，
 * 而不是从目录结构隐式长出来的。
 *
 * ## 骨架的边界
 *
 * 七个一级页面（主页 / 实例 / 下载 / 账号 / 百宝箱 / 设置 / 关于）现在是**真组件**
 *（`web/src/routes/*Page.tsx`）—— 它们各自的**内容**还没接内核，所以它们大多
 * 用一个真实的空态把"这里缺什么"写出来，而不是画灰条假装在加载。
 *
 * 而**二级页面**（六个下载分类、两个账号页、四件工具、四个设置分类）仍是
 * 下面那个 `page()` 工厂生成的占位 —— 它们的存在证明了"两段式导航 +
 * 百宝箱二级分组的可跑通空壳"（门禁③ 的交付物本身）。
 */

import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router";
import type { ReactElement } from "react";
import { Shell } from "./Shell.tsx";
import { PagePlaceholder } from "./PagePlaceholder.tsx";
import { AboutPage } from "./AboutPage.tsx";
import { AccountsPage } from "./AccountsPage.tsx";
import { CapabilitiesPage } from "./CapabilitiesPage.tsx";
import { CapabilitiesQueryPage } from "./CapabilitiesQueryPage.tsx";
import { ComponentsPage } from "./ComponentsPage.tsx";
import { DownloadsPage } from "./DownloadsPage.tsx";
import { HomeRoute } from "./HomeRoute.tsx";
import { InstanceDetailPage } from "./InstanceDetailPage.tsx";
import { InstancesPage } from "./InstancesPage.tsx";
import { LogsPage } from "./LogsPage.tsx";
import { SettingsPage } from "./SettingsPage.tsx";
import { ToolboxPage } from "./ToolboxPage.tsx";

/** 根路由：外壳（侧栏 + 顶栏 + 内容区）。 */
const rootRoute = createRootRoute({
  component: Shell,
});

/** 一个占位页的工厂 —— 骨架里 20 多个路由都由它生成。
 *
 * ⚠️ **不要给它写返回类型注解。**
 *
 * 第一版写的是 `: ReturnType<typeof createRoute>`，而 `createRoute` 的返回
 * 类型**依赖它的泛型参数**（父路由、路径、参数……）。用一个"擦掉泛型"的
 * `ReturnType<>` 会让下游 `addChildren` 的联合类型对不上，而错误信息会
 * 变成**几百行的泛型不匹配**——完全看不出真因。
 *
 * 让 TS 推断，于是每个路由带着自己的精确类型进 `addChildren`。
 *
 * ## 🔴 而 `<const P extends string>` 是这一段的关键
 *
 * 参数写成 `path: string` 时，`createRoute` 收到的是**被拓宽的 `string`**，
 * 于是这个路由的路径**进不了 `to=` 的合法值联合** —— 后果是
 * `Link to="/downloads/loaders"` **能编译过而运行时 404**
 *（正是选这个路由库要挡的那件事，见文件末尾的 `declare module`）。
 *
 * 而这里有个反直觉的观测：**这个洞存在了很久而没人发现**，因为所有页面
 * 在此之前都是占位、一个 `Link` 都没有。`web/src/routes/DownloadsPage.tsx`
 * 第一次真的链到二级页，`tsc` 立刻报
 * `Type '"/downloads/loaders"' is not assignable to type '"…"'`，
 * 合法值联合里**一个二级路径都没有**。
 *
 * `const P` 保留字面量类型，于是每一条二级路径都被真的登记进联合。
 */
function page<const P extends string>(path: P, title: string, note: string) {
  return createRoute({
    getParentRoute: () => rootRoute,
    path,
    component: (): ReactElement => <PagePlaceholder title={title} note={note} />,
  });
}

/* ── 产品相关区 ─────────────────────────────────────────────────────── */

// ⚠️ 主页**不是占位** —— 它是 §4.6.8 的真实落点。
const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: HomeRoute,
});

// 实例：列表 + 详情（详情是动态段，**它证明"跳转到未注册的路由"会编译报错**）
const instancesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/instances",
  component: InstancesPage,
});

const instanceDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/instances/$instanceId",
  component: InstanceDetailPage,
});

/**
 * 🔴 **分组那三项是二级栏真的会链到的路径**，所以「最近使用」「收藏」必须存在。
 *
 * `Shell.tsx` 的 `secondaryHref()` 是机械的：`一级路径 + "/" + 项 key`
 * —— 而「全部实例」不在这条规则里：**它就是 `/instances` 这一页本身**
 * （见 `nav.ts` 的 `SecondarySection.rootItem`），所以这里没有 `/instances/all`。
 *
 * ⚠️ 而 `secondaryHref()` 的返回类型是 `string`（不是路由字面量联合），
 * 所以**类型系统抓不到"指向一个不存在的路由"** —— 这三条路径此前一条都不存在，
 * 点下去是 **Not Found**（真窗口里验过）。今天补上，并由
 * `tools/check-shell-contract.ps1` 的检查 G 钉住：
 * **每个静态二级项的路径都必须有一条同路径的路由，除非它就是一级页**。
 *
 * 等实例列表那条命令接上之后，这两项很可能变成同一页的筛选
 * （`/instances?group=recent`），届时删掉这两条占位路由。
 */
const instancesRecent = page("/instances/recent", "实例 · 最近使用", "");
const instancesFavorites = page("/instances/favorites", "实例 · 收藏", "");

// 下载：二级分组的六个页（一级页是目录，六个分类各自仍是占位）
const downloadsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/downloads",
  component: DownloadsPage,
});
// ⚠️ 「游戏版本」也必须有一条自己的路由 —— 它是二级栏「类型」里的第一项，
// 而一级页（`/downloads`）是**六类目录**，不是版本列表。见检查 G。
const downloadsVersions = page("/downloads/versions", "下载 · 游戏版本", "正式版 / 快照 / 旧版 —— 官方源清单里的版本");
const downloadsLoaders = page("/downloads/loaders", "下载 · 加载器", "Fabric / NeoForge / Forge / Quilt");
const downloadsMods = page("/downloads/mods", "下载 · 模组", "Modrinth → CurseForge");
const downloadsResourcepacks = page("/downloads/resourcepacks", "下载 · 资源包", "");
const downloadsShaders = page("/downloads/shaders", "下载 · 光影", "");
const downloadsWorlds = page("/downloads/worlds", "下载 · 存档", "");

/* ── 下段工具区 ─────────────────────────────────────────────────────── */

const accountsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/accounts",
  component: AccountsPage,
});
const accountsAuth = page("/accounts/auth", "授权状态", "**账户层与授权层拆开**（M5）");

const toolboxRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/toolbox",
  component: ToolboxPage,
});
// ⚠️ 日志页**不是占位** —— 它是 §5.5 的真实落点（组件已存在）。
const toolboxLogs = createRoute({
  getParentRoute: () => rootRoute,
  path: "/toolbox/logs",
  component: LogsPage,
});
const toolboxSpeedtest = page("/toolbox/speedtest", "百宝箱 · 测速", "官方源 vs 镜像");
const toolboxComponents = page("/toolbox/components", "百宝箱 · 依赖与组件检查", "");
const toolboxCleanup = page("/toolbox/cleanup", "百宝箱 · 磁盘清理", "");
const toolboxSchematic = page("/toolbox/schematic", "百宝箱 · 投影材质查看", ".litematic / .nbt 结构预览 + 材料清单");

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
});
const settingsRuntime = page("/settings/runtime", "设置 · 运行时", "探测 / 选择 / 内存 / 启动参数");
const settingsNetwork = page("/settings/network", "设置 · 网络与镜像", "默认官方源优先，镜像作回退");
const settingsStorage = page("/settings/storage", "设置 · 存储与目录", "");
const settingsAdvanced = page("/settings/advanced", "设置 · 高级", "");

const aboutRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/about",
  component: AboutPage,
});

/**
 * ⚠️ **自检页：U0 那一页变成了一条路由。**
 *
 * 它**不在侧栏里** —— 因为它不是产品功能，而是一个"契约对得上吗"的自检口。
 * 一个把它放进侧栏的实现会让用户以为"能力表"是一个功能。
 */
const capabilitiesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/__capabilities",
  component: CapabilitiesPage,
});

/**
 * **门禁② 的自验页**：20 个基础组件 × 六状态。
 *
 * ⚠️ 它也**不在侧栏里** —— 它是给人眼验伪类状态用的自检口。
 */
const componentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/__components",
  component: ComponentsPage,
});

/**
 * **门禁④ 的贯通示例。**
 *
 * 它走完整条链：`api/index.ts` 的边界层 → `api/query.ts` 的键与缓存 →
 * TanStack Query 的三态 → 组件只渲染。
 *
 * ⚠️ 同样**不在侧栏里** —— 它是自检口，不是产品功能。
 * 而"把自检口放进侧栏"是那种看起来很整齐、实际让用户困惑的做法。
 */
const capabilitiesQueryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/__data-layer",
  component: CapabilitiesQueryPage,
});

/* ── 路由树 ─────────────────────────────────────────────────────────── */

const routeTree = rootRoute.addChildren([
  indexRoute,
  instancesRoute,
  instanceDetailRoute,
  instancesRecent,
  instancesFavorites,
  downloadsRoute,
  downloadsVersions,
  downloadsLoaders,
  downloadsMods,
  downloadsResourcepacks,
  downloadsShaders,
  downloadsWorlds,
  accountsRoute,
  accountsAuth,
  toolboxRoute,
  toolboxLogs,
  toolboxSpeedtest,
  toolboxComponents,
  toolboxCleanup,
  toolboxSchematic,
  settingsRoute,
  settingsRuntime,
  settingsNetwork,
  settingsStorage,
  settingsAdvanced,
  aboutRoute,
  capabilitiesRoute,
  capabilitiesQueryRoute,
  componentsRoute,
]);

export const router = createRouter({
  routeTree,
  /**
   * ⚠️ **默认预加载关掉是刻意的。**
   *
   * 方案 §8 的 M4 验收里有一条："**冷启动期间不等待任何网络请求**（§7 口径）"。
   * 而预加载会在鼠标掠过侧栏时就发请求 —— 那正是"冷启动期间有网络"的一种形态。
   *
   * 数据由 TanStack Query 按页取，而**取数据的时机是"这一页真的被打开"**。
   */
  defaultPreload: false,
  /**
   * 滚动复位到顶部 —— 骨架期唯一需要的导航行为。
   */
  scrollRestoration: false,
});

/**
 * ⚠️ **`declare module` 是 TanStack Router 的类型安全前提。**
 *
 * 少了它，`router.navigate({ to: "/拼错的路由" })` **不会报错** ——
 * 而那正是选这个路由库的**唯一理由**（§4.5："能在编译期挡住跳转到未注册的
 * 能力页"）。所以下面这一段不是样板，它是那条理由本身。
 */
declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
