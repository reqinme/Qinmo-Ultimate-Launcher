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

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "@tanstack/react-router";
import { QueryClientProvider } from "@tanstack/react-query";

import { router } from "./routes/router.tsx";
import { queryClient } from "./api/query.ts";
import { useAppearance } from "./appearance/useAppearance.ts";
import "./tokens.css";
import "./styles.css";

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
        <RouterProvider router={router} />
      </QueryClientProvider>
    </AppearanceBoot>
  </StrictMode>,
);
