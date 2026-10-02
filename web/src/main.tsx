/**
 * 前端入口。
 *
 * **它只做两件事**：挂载 React、把路由树交给 `RouterProvider`。
 * 任何"该显示什么"的判断都不在这里 —— 那是后端的活。
 *
 * ## M4 之后的形状
 *
 * U0 的时候它是"把能力表交给 `App`"；现在它是"把路由交给 `RouterProvider`"，
 * 而能力表成了**某一页的数据**（`web/src/api/` 的边界层负责取它，
 * TanStack Query 负责缓存）—— 见门禁④。
 *
 * ⚠️ **挂载时一个网络请求都不发**。方案 §8 的 M4 验收有一条：
 * 「**冷启动期间不等待任何网络请求**（§7 口径）」。
 * 所以这里没有"启动时预热"、没有"提前拉能力表" —— 那些都让冷启动
 * 依赖网络，而"网络慢"会变成"窗口白屏很久"。
 */

// 🔴 **这一个必须排在所有别的业务 import 之前。**
//
// ESM 的求值顺序保证"被 import 的模块先于 import 它的模块求值"，
// 所以这一行让记录器在任何别的模块的顶层代码之前装上 ——
// 而"冷启动零网络"这条验收**只有**那样才成立。
//
// ⚠️ 而"它排在第一"是一个**容易后人破坏**的前提（往上面加一行 import
// 就破坏了它）。`coldStart.ts` 里那段说明了为什么它仍然可验：
// 兜底通道是 `PerformanceResourceTiming`，而它**能看见**记录器之前
// 已经发生的资源加载。
import { assertNoColdStartNetwork, installColdStartRecorder } from "./boot/coldStart.ts";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "@tanstack/react-router";
import { QueryClientProvider } from "@tanstack/react-query";

import { router } from "./routes/router.tsx";
import { queryClient } from "./api/query.ts";
import { useAppearance } from "./appearance/useAppearance.ts";
import { IslandProvider } from "./island/IslandProvider.tsx";
import "./tokens.css";
import "./styles.css";

/*
 * 🔴 **装上记录器 —— 而它必须在 `createRoot` 之前。**
 *
 * `createRoot(...).render(...)` 是**第一件真的会引发副作用**的事
 *（它跑 effect、它挂组件）。所以记录器要排在它前面。
 *
 * ⚠️ 而**这一条顺序依赖是可以被后人破坏的**（把这两行挪到 `render` 之后
 * 就破坏了它，而界面照旧正常）。所以：
 *
 * - `installColdStartRecorder()` 内部有 `PerformanceObserver` 兜底，
 *   它带 `buffered: true` ⇒ **它能看见**记录器装上之前已经发生的资源加载；
 * - 而 `assertNoColdStartNetwork()` 把结果喊到控制台，
 *   于是"破坏了顺序"这件事**会被看到**，而不是静默通过。
 */
installColdStartRecorder();
assertNoColdStartNetwork();

const host = document.getElementById("root");
if (!host) {
  throw new Error("找不到 #root 挂载点");
}

/**
 * 外观接线层。
 *
 * ⚠️ **它必须是一个组件**，因为 `useAppearance` 是钩子（要订阅
 * `matchMedia` 的变化）。一个在 `main.tsx` 顶层调用解析函数的实现
 * 会**只在启动时读一次**系统偏好 —— 而用户切主题之后我们不会跟上。
 */
function AppearanceBoot({ children }: { readonly children: React.ReactNode }): React.ReactElement {
  useAppearance();
  return <>{children}</>;
}

createRoot(host).render(
  <StrictMode>
    {/*
      ⚠️ **`QueryClientProvider` 在 `RouterProvider` 之外。**
      反过来的话，路由自己（将来的 loader）就拿不到缓存 ——
      而那正是"路由是数据的边界"这个结构的意义。
    */}
    <AppearanceBoot>
      <QueryClientProvider client={queryClient}>
        {/*
          🔴 **灵动岛的 Provider 在 `RouterProvider` 之外。**

          它决定了"`HomeRoute` 的按钮能不能推到 `Shell` 里渲染的那个岛" ——
          而答案是能，因为两者都在**这一层之下**。

          ⚠️ 而它**不能反过来**（Provider 在 `RouterProvider` 里面）：
          `RouterProvider` 的子树**不继承**外层的 context，于是
          每一页调 `useIsland()` 都会抛。见 `IslandProvider.tsx` 里那张表。
        */}
        <IslandProvider>
          <RouterProvider router={router} />
        </IslandProvider>
      </QueryClientProvider>
    </AppearanceBoot>
  </StrictMode>,
);
