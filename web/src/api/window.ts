/**
 * 窗口控制（**自绘标题栏的那一半**）
 * ============================================================================
 *
 * ## 🔴 为什么它在 `api/` 里
 *
 * 因为它是**唯一允许碰 IPC 的那一层** —— `boundary.test.ts` 里那条
 * "`@tauri-apps/api/core` 只允许在 `api/` 下被 import"的检查会拦下别的位置。
 * 而窗口控制**不是**"业务逻辑绕过命令层"，它是**边界层的另一条命令**。
 *
 * ## ⚠️ 为什么是 `invoke` 而不是 `getCurrentWindow().minimize()`
 *
 * `@tauri-apps/api/window` 的 `getCurrentWindow().minimize()` 需要
 * `core:window:allow-minimize` 那一条**插件权限** —— 而
 * `capabilities/main-window.json` 的 `permissions` 是**空**的（§5.8 的要求）。
 *
 * 所以窗口控制走**自定义命令**（见 `src-tauri/src/main.rs` 的
 * `window_minimize` 那一段的理由）：三个按钮只需要三个动作，
 * 不需要一整族权限。
 *
 * ## ⚠️ 而**浏览器里**它们静默返回 —— 而不是抛
 *
 * 与 `startInstall` 那条**相反**，而这不对称是**有意的**：
 *
 * | | 没有后端时 | 为什么 |
 * |---|---|---|
 * | `startInstall` | **抛** | 一次装几十 GB 的操作**假装成功**是最坏的一类错 |
 * | 窗口控制 | **静默返回** | 在浏览器里预览界面时，"最小化"**本来就无事可做** —— 抛出去只会让预览里的控制台变脏 |
 *
 * 而"静默"在这里是安全的：窗口控制的**返回值和副作用都不是数据** ——
 * 没有任何下游会因为"它没真的最小化"而算错。
 */

import { invoke, isTauri } from "@tauri-apps/api/core";

/** 最小化窗口。 */
export async function windowMinimize(): Promise<void> {
  if (!isTauri()) return;
  await invoke("window_minimize");
}

/**
 * 最大化 ⇄ 还原。
 *
 * @returns 切换**之后**是否处于最大化 —— 而那是**窗口那边**的真相，
 *          不是前端猜的（见 Rust 侧那一段：连点两下不会不一致）。
 */
export async function windowToggleMaximize(): Promise<boolean> {
  if (!isTauri()) return false;
  const raw: unknown = await invoke("window_toggle_maximize");
  return raw === true;
}

/**
 * 窗口**现在**是不是最大化。
 *
 * ## ⚠️ 为什么"切换"那条命令的返回值还不够
 *
 * `windowToggleMaximize` 返回的是**切换之后**的状态，于是"我们自己点"
 * 那一路不需要再问一次。而**最大化不只由我们改变**：
 *
 * | 谁改的 | 经过 `onClick` 吗 |
 * |---|---|
 * | 标题栏那个按钮 / 标题栏空白处双击 | ✅ |
 * | `Win + ↑` / 把窗口拖到屏幕顶端 | ❌ |
 *
 * 后者会让按钮上那个图形变成一个**谎**（窗口已经最大化，而它还画着
 * "点了能最大化"那个方框）。所以 `TitleBar` 在挂载时、以及每次 `resize`
 * 之后问一次 —— 见 `TitleBar.tsx` 里那段 `useEffect`。
 *
 * ⚠️ 它**不需要**新的插件权限：窗口控制走的是自定义命令（与那四条同一个
 * 理由，见本文件顶部那段与 §5.8）。
 */
export async function windowIsMaximized(): Promise<boolean> {
  if (!isTauri()) return false;
  const raw: unknown = await invoke("window_is_maximized");
  return raw === true;
}

/** 关闭窗口。 */
export async function windowClose(): Promise<void> {
  if (!isTauri()) return;
  await invoke("window_close");
}

/**
 * 开始拖动窗口。
 *
 * ⚠️ **它必需**：`decorations: false` 意味着**没有系统标题栏**，
 * 而标题栏是系统提供的拖动区。没有它窗口**移不动**。
 */
export async function windowStartDragging(): Promise<void> {
  if (!isTauri()) return;
  await invoke("window_start_dragging");
}
