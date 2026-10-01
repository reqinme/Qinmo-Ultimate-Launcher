/**
 * 测试环境的前置（`vitest.setupFiles`）
 * ============================================================================
 *
 * ## 🔴 它只做一件事，而那一件事是**跨 realm 的 `instanceof`**
 *
 * jsdom 环境里，`window` 有它**自己的 realm**，于是 `window.Uint8Array`
 * 与外层 Node 的 `Uint8Array` **不是同一个构造函数**。而某些库
 *（`react-dom` 与 `@testing-library/react` 里的编码探测）会断言：
 *
 * ```text
 *   new TextEncoder().encode("") instanceof Uint8Array
 * ```
 *
 * 当 `TextEncoder` 来自一个 realm 而 `Uint8Array` 来自另一个时，
 * 那个断言是 `false` —— 而它会以 `Invariant violation` 的形式**在模块加载期**
 * 让整个测试文件失败（不是某个用例失败）。
 *
 * ## 修法是"把两边对齐到一个 realm"，而不是绕过那个断言
 *
 * 把 Node 的 `TextEncoder` / `TextDecoder` / `Uint8Array` 显式装到
 * 全局上 —— 于是编码器与类型判断来自**同一个** realm。
 *
 * ⚠️ **不要去 polyfill 一个假的 `TextEncoder`**：那会让"真实编码行为"
 * 与测试里看到的不一致，而本项目要测的正是真实的渲染与序列化行为。
 */

// ⚠️ 这些是**真实**的实现，只是从 Node 的 realm 取。
import { TextDecoder, TextEncoder } from "node:util";

const g = globalThis as unknown as Record<string, unknown>;

if (typeof g.TextEncoder === "undefined") {
  g.TextEncoder = TextEncoder;
}
if (typeof g.TextDecoder === "undefined") {
  g.TextDecoder = TextDecoder;
}

/**
 * ⚠️ **`Request` / `Response` / `fetch` 也要对齐。**
 *
 * jsdom 不提供它们（Node 18+ 提供），而 TanStack Query 的某些路径会碰到。
 * 若外层有而 jsdom 的 window 上没有，就会出现"这个函数在测试里不存在"
 * 的偶发失败 —— 而那类失败最容易被误判成"框架有 bug"。
 */
for (const name of ["Request", "Response", "Headers", "fetch"] as const) {
  if (g[name] === undefined && (globalThis as never as Record<string, unknown>)[name] === undefined) {
    // 从 Node 的全局取（若 Node 也没提供就跳过 —— 那时测试自己会说不缺什么）
    const fromNode = (globalThis as unknown as Record<string, unknown>)[name];
    if (fromNode !== undefined) {
      g[name] = fromNode;
    }
  }
}
