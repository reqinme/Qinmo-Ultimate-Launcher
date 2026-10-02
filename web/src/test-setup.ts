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

/* ============================================================================
 * **`window.scrollTo` —— jsdom 没实现它，而 TanStack Router 会调它**
 * ============================================================================
 *
 * ## 症状
 *
 * 每一条渲染了路由的测试都会往 stderr 写一行：
 *
 * ```text
 *   Not implemented: Window's scrollTo() method
 * ```
 *
 * ## ⚠️ 而它**不是**我们的 bug —— 而它仍然必须被修
 *
 * 断言**照旧全过**（jsdom 对未实现的 DOM 方法只记一条警告），
 * 而"照旧全过"正是它该被修的唯一理由：**它会训练人忽略 stderr**。
 *
 * 而不久之后一个真问题也会写进那同一条流里，而那时没人看。
 *
 * ## 为什么是"什么都不做的桩"而不是一个真的滚动
 *
 * 测试里没有布局引擎，所以**没有「滚到哪里」这件事**。
 * 一个试图真的滚动的实现只能自己编一个位置 —— 那会引入一个
 * **假的真相**（"它滚了"），而它骗的是下一个读测试的人。
 *
 * 所以这里是**显式的空实现**，并说明它为什么是空的。
 *
 * ## ⚠️ 而它是**无条件覆盖**的 —— 而第一版不是，那一版没生效
 *
 * 第一版的条件是：
 *
 * ```ts
 * if (typeof g.scrollTo !== "function" || /not implemented/i.test(String(g.scrollTo)))
 * ```
 *
 * 而那个条件**永远为假**：jsdom 确实**装了**一个 `scrollTo`（它是一个真函数），
 * 只是它内部 `console.error("Not implemented")` 然后就返回。
 * 也就是说 `String(g.scrollTo)` 里**没有**"not implemented"那几个字 ——
 * 那句话在**函数体运行时**才产生。
 *
 * > 而这是一条可复用的教训：**"探测一个桩存不存在"不能靠它的名字或字符串。**
 * > jsdom 的未实现方法**是存在的函数**，而它们的证据在**运行时输出**里。
 */
g.scrollTo = (): void => {
  /* 测试环境没有布局，所以没有可滚动的目标 —— 见上面那段。 */
};