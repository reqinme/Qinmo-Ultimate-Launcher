/**
 * 灵动岛的**全局接线层**（Context）
 * ============================================================================
 *
 * ## 🔴 为什么需要这一层：两件事**不在同一棵子树里**
 *
 * ```text
 *   main.tsx
 *     └─ RouterProvider
 *          └─ Shell              ← 岛渲染在这里（它要浮在整个应用之上）
 *               └─ Outlet
 *                    └─ HomeRoute  ← 而"启动"按钮在这里
 * ```
 *
 * `HomeRoute` 是 `Shell` 的**子**，所以它拿不到 `Shell` 里的状态 ——
 * 而"点一下启动"这件事**必须由主页发起**。
 *
 * ## 而三条路里为什么选"提到路由之外"
 *
 * | 做法 | 为什么不 |
 * |---|---|
 * | 状态放在 `Shell` 里 | 主页拿不到（见上） |
 * | 状态放在 `main.tsx` 里渲染 | `RouterProvider` 的子树**不继承**外层的 context —— 那会让 `useIsland()` 在每一页里都抛 |
 * | **一个 Provider 包住 `RouterProvider`** | ✅ 而它正好对上 §7.1 的定位 |
 *
 * §7.1 那句是：*"把此刻唯一值得看的事**提到顶层**"* ——
 * 而"提到顶层"在这里是**字面**的：Provider 在路由之外，
 * 于是**每一页**都能看它、都能推它。
 *
 * ## ⚠️ 而 `useIsland()` 在 Provider 之外**会抛**
 *
 * 而不是返回一个"空的岛"。一个返回空壳的实现会让"我把 Provider 忘了"
 * 表现为**岛永远空闲** —— 而那与"没事发生"在屏幕上一模一样。
 * 抛出去至少指得到这里。
 */

import { createContext, useContext, type ReactElement, type ReactNode } from "react";
import { useIslandQueue, type IslandQueue } from "./useIslandQueue.ts";

const Ctx = createContext<IslandQueue | null>(null);

/**
 * 包住整个路由树。
 *
 * ⚠️ **它必须在 `RouterProvider` 之外** —— 见模块文档里那张表。
 */
export function IslandProvider({
  children,
}: {
  readonly children: ReactNode;
}): ReactElement {
  const queue = useIslandQueue();
  return <Ctx.Provider value={queue}>{children}</Ctx.Provider>;
}

/** 取当前那个队列。**必须在 `IslandProvider` 之内调用。** */
export function useIsland(): IslandQueue {
  const v = useContext(Ctx);
  if (v === null) {
    throw new Error(
      "useIsland() 用在了 IslandProvider 之外 —— 而灵动岛的状态是全局的" +
        "（§7.1：把此刻唯一值得看的事提到顶层）。检查 main.tsx 里那一层。",
    );
  }
  return v;
}
