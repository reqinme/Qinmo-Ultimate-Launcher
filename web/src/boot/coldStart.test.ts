/**
 * 冷启动那条承诺的测试（**"零网络"必须能被证，而也能被弄红**）
 * ============================================================================
 *
 * ## 🔴 这个文件的价值在**反证**那两条
 *
 * "我们没有发请求"这条断言，如果**永远只看一个空数组**，那它是**同义反复** ——
 * 一个 `coldStartAttempts()` 恒返回 `[]` 的实现会让它永远绿。
 *
 * 所以下面的结构是：
 *
 * | 组 | 它证 |
 * |---|---|
 * | ① | 记录器**真的会记**（`fetch` / `xhr` / `beacon` 各一条） |
 * | ② | 记的**内容**对（via 与 url） |
 * | ③ | 🔴 **而 M4 那条承诺本身**：这个应用的启动路径上零条 |
 *
 * 第 ③ 组才是验收，而它**只有在前两组成立时才有意义**。
 */

import { afterEach, describe, expect, it, vi } from "vitest";

// ⚠️ **没有静态 import 那个模块本身。**
//
// 而那是**有意的**：每个测试要一份**干净**的模块实例（它的
// `attempts` / `installed` 是模块级状态），所以一律走下面的 `fresh()`。
// 一个 `import { … } from "./coldStart.ts"` 会让所有测试**共用**那份状态 ——
// 于是"第二条测试看到第一条留下的记录"。
//
// （而 TypeScript 的 `noUnusedLocals` 会指出这一点 —— 它是对的。）

// ⚠️ **每个测试之间要能重来。**
//
// 而模块级状态在一个 vitest 文件里只求值一次 ——
// 所以这里用 `vi.resetModules()` + 动态 `import()` 拿一份干净实例。
async function fresh() {
  vi.resetModules();
  return await import("./coldStart.ts");
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("① 记录器真的会记（否则第 ③ 组是同义反复）", () => {
  it("`fetch` 会被记下", async () => {
    const m = await fresh();
    const spy = vi.fn(async () => new Response("{}"));
    vi.stubGlobal("fetch", spy);
    m.installColdStartRecorder();
    await globalThis.fetch("https://example.invalid/a.json");
    const got = m.coldStartAttempts();
    expect(got).toHaveLength(1);
    expect(got[0]?.via).toBe("fetch");
    expect(got[0]?.url).toContain("example.invalid");
  });

  it("`XMLHttpRequest` 会被记下", async () => {
    const m = await fresh();
    m.installColdStartRecorder();
    // ⚠️ 不真的发出去 —— `open` 只是配置，而记录发生在 `open` 里。
    // 一个 mock 掉 `send` 的实现在 jsdom 下会**真的发一个请求**，
    // 而那会让这条测试变慢且依赖网络。
    const x = new XMLHttpRequest();
    x.open("GET", "https://example.invalid/b.json");
    const got = m.coldStartAttempts();
    expect(got.some((a) => a.via === "xhr" && a.url.includes("example.invalid"))).toBe(true);
  });

  it("`sendBeacon` 会被记下", async () => {
    const m = await fresh();
    const spy = vi.fn(() => true);
    // ⚠️ `navigator` 在 jsdom 下是**只读**的 —— 所以这里用
    // `vi.stubGlobal` 换掉整个 `navigator`，而不是给它加一个属性。
    vi.stubGlobal("navigator", { sendBeacon: spy });
    m.installColdStartRecorder();
    navigator.sendBeacon("https://example.invalid/c", "x");
    const got = m.coldStartAttempts();
    expect(got.some((a) => a.via === "beacon")).toBe(true);
  });

  it("🔴 装两次只记一份（幂等）", async () => {
    // ⚠️ React 的 StrictMode 会双调一些东西，而记录器不该因此记两遍 ——
    // 否则"两条请求"这个数会**凭空翻倍**，而那个数正是断言的对象。
    const m = await fresh();
    const spy = vi.fn(async () => new Response("{}"));
    vi.stubGlobal("fetch", spy);
    m.installColdStartRecorder();
    m.installColdStartRecorder();
    await globalThis.fetch("https://example.invalid/d");
    expect(m.coldStartAttempts()).toHaveLength(1);
  });
});

describe("② 记的内容对", () => {
  it("带时间戳，而 `atMs` 是单调的", async () => {
    const m = await fresh();
    vi.stubGlobal("fetch", vi.fn(async () => new Response("{}")));
    m.installColdStartRecorder();
    await globalThis.fetch("https://example.invalid/1");
    await globalThis.fetch("https://example.invalid/2");
    const got = m.coldStartAttempts();
    expect(got).toHaveLength(2);
    expect(got[1]!.atMs).toBeGreaterThanOrEqual(got[0]!.atMs);
  });

  it("超长的 URL 会被截断（日志不该被一个 URL 撑爆）", async () => {
    const m = await fresh();
    vi.stubGlobal("fetch", vi.fn(async () => new Response("{}")));
    m.installColdStartRecorder();
    const long = "https://example.invalid/" + "x".repeat(5000);
    await globalThis.fetch(long);
    expect(m.coldStartAttempts()[0]!.url.length).toBeLessThanOrEqual(200);
  });
});

describe("🔴 ③ M4 那条承诺：这个应用的**启动路径**上零条", () => {
  it("`App` 的模块图求值完之后，零条记录", async () => {
    // ⚠️ **这一条才是验收。** 它 import 的是**应用的入口那一族**
    //（`main.tsx` 会 import 的一切），而断言的是"求值它们不发请求"。
    //
    // 而它**在测试环境里**跑，所以有一件事要说清：
    // `main.tsx` 不能被 import（它会 `createRoot` 并挂到一个不存在的
    // `#root` 上）。所以这里 import 的是**那些模块本身**：
    // 路由、数据层、外观、岛、标题栏。
    //
    // 一个"启动时预热能力表"的实现会在这里红 —— 而那正是这条要拦的。
    const m = await fresh();
    m.installColdStartRecorder();
    await import("../routes/router.tsx");
    await import("../api/query.ts");
    await import("../appearance/useAppearance.ts");
    await import("../island/IslandProvider.tsx");
    await import("../titlebar/TitleBar.tsx");
    expect(m.coldStartAttempts()).toEqual([]);
  });

  it("🔴 而 `assertNoColdStartNetwork` 在**有记录时**会喊出来", async () => {
    // ⚠️ **反证**：一个只会 `return` 的实现在上面那条绿的
    // 同时**什么都不检查**。
    const m = await fresh();
    vi.stubGlobal("fetch", vi.fn(async () => new Response("{}")));
    m.installColdStartRecorder();
    await globalThis.fetch("https://example.invalid/leak");
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    m.assertNoColdStartNetwork();
    // ⚠️ 它等 `DOMContentLoaded` 或一轮宏任务 —— 而 jsdom 下
    // `readyState` 已经是 `complete`，所以走的是 `setTimeout` 那条。
    await new Promise((r) => setTimeout(r, 10));
    expect(spy).toHaveBeenCalled();
    expect(String(spy.mock.calls[0]?.[0])).toContain("冷启动期间发了 1 个请求");
    spy.mockRestore();
  });

  it("而零记录时它**不喊**（否则它就是个噪音源）", async () => {
    const m = await fresh();
    m.installColdStartRecorder();
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    m.assertNoColdStartNetwork();
    await new Promise((r) => setTimeout(r, 10));
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });
});
