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
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

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

/* ============================================================================
 * 🔴 **显式清理 DOM —— 因为 `globals: false`**
 * ============================================================================
 *
 * ## 症状（而它极难从失败信息里看出来）
 *
 * 「对话框：它没打开时内容不在 DOM 里」这条测试**单跑通过、全量失败**，
 * 而错误是 `expected <span></span> to be null`。
 *
 * ## 真因
 *
 * `@testing-library/react` 的自动清理**注册在一个全局 `afterEach` 上**，
 * 而只有当 `globals: true` 时它才会自己注册。我们把 `globals` 设成了 `false`
 *（那是刻意的：显式 import 让每个测试文件的依赖自洽）——
 * **于是 DOM 从不清空**，前面测试渲染的对话框**留在了文档里**。
 *
 * ## 为什么必须显式清理，而不是"把 globals 打开"
 *
 * 打开 `globals` 会修好这一条，而它同时会把 `describe` / `it` / `expect`
 * 注入全局 —— 那正是我们刻意不要的东西。
 * **一个显式的 `afterEach(cleanup)` 只解决它，不动别的东西。**
 *
 * ## 而这一类 bug 的教训值得单独说
 *
 * **"单跑通过、全量失败"几乎总是共享状态** —— 而这里共享的是 `document`。
 * 一个只在全量下出现的失败如果被当成 flake 重试掉，它就永远不会被找到
 *（见 `SESSION.md` 里那次"瞬时失败"的记录）。
 */
afterEach(() => {
  cleanup();
});