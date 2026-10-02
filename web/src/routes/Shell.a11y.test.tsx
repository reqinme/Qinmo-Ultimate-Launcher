/**
 * M4 验收：「**键盘与读屏通过**」
 * ============================================================================
 *
 * ## 🔴 那一条验收里能**机械核对**的部分
 *
 * §8 的 M4 行只写了五个字，而它可以拆成四件可验的事：
 *
 * | # | 它要求 | 能不能机器验 |
 * |---|---|---|
 * | 1 | 每个可聚焦元素**都有可读名字** | ✅ 本文件 |
 * | 2 | **焦点顺序**是有意义的（标题栏 → 侧栏 → 内容） | ✅ 本文件 |
 * | 3 | **焦点指示器可见**（不是 `outline: none`） | ⚠️ 部分（见下） |
 * | 4 | `Win + ←/→/↑` 的键盘贴靠仍然可用（§4.5 的降级线） | ❌ **要人来验** —— 见本文件最后一节 |
 *
 * ## ⚠️ 而第 3 条只能核到"有那条规则"，核不到"它看得见"
 *
 * jsdom **不算样式**（它没有布局引擎），所以 `getComputedStyle` 给不出
 * 真实的 `outline-width`。所以这里改成一个**静态的源码断言**：
 * 标题栏的 CSS 里**必须有** `:focus-visible` 规则。
 *
 * 那比"我写了它"强一点（它会被构建打断），而比"它真的看得见"弱 ——
 * **而这个强弱差别必须写出来**，否则下一个人会以为这一条已经被证明了。
 */

import { createMemoryHistory, createRootRoute, createRoute, createRouter, RouterProvider } from "@tanstack/react-router";
import { render, screen, waitFor } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { Shell } from "./Shell.tsx";
import { IslandProvider } from "../island/IslandProvider.tsx";

const HERE = dirname(fileURLToPath(import.meta.url));

/** 一个最小的路由树：**真的 `Shell`** + 一个假的内容页。 */
function renderShell() {
  const rootRoute = createRootRoute({ component: Shell });
  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: () => <p>内容页</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([indexRoute]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  return render(
    // ⚠️ `IslandProvider` 必需 —— `Shell` 里 `useIsland()` 在它之外**会抛**
    //（那个"抛"是有意的，见 `IslandProvider.tsx`）。
    <IslandProvider>
      <RouterProvider router={router as never} />
    </IslandProvider>,
  );
}

/**
 * `document` 里**按 DOM 顺序**的全部可聚焦元素。
 *
 * ⚠️ 用 `querySelectorAll` 而不是 `getAllByRole("button")`：
 * 后者**只找按钮**，而"焦点顺序"这件事的对象是**所有**可聚焦的东西
 *（链接、输入、`[tabindex]`）。
 */
function focusables(): HTMLElement[] {
  return Array.from(
    document.querySelectorAll<HTMLElement>(
      'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ),
  );
}

/**
 * 一个元素的**可读名字**。
 *
 * ## ⚠️ 而这是 accessibility 里最容易被做错的一处，所以它的规则要写下来
 *
 * 计算可访问名字（accname）的**简化版**，够用的那几条：
 *
 * | 顺序 | 来源 |
 * |---|---|
 * | 1 | `aria-labelledby` 指向的元素文本 |
 * | 2 | `aria-label` |
 * | 3 | 元素自身的文本内容（含子元素；`aria-hidden` 的要排除） |
 * | 4 | `title`（**最弱** —— 读屏不一定念，但它比什么都没有强） |
 * | 5 | `<img>` 的 `alt` |
 *
 * ⚠️ **而一个纯 `aria-hidden` 的图标不算名字** —— 那正是
 * `TitleBar` 上一版那个 `<span aria-hidden="true">✕</span>` 的形状：
 * 它**看起来**像按钮里有东西，而读屏**什么都读不到**。
 */
function accessibleName(el: HTMLElement): string {
  const byLabelledBy = el.getAttribute("aria-labelledby");
  if (byLabelledBy) {
    const t = byLabelledBy
      .split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent ?? "")
      .join(" ")
      .trim();
    if (t) return t;
  }
  const label = el.getAttribute("aria-label");
  if (label?.trim()) return label.trim();

  // 自己与后代的文本，**跳过 `aria-hidden`** 的子树。
  const clone = el.cloneNode(true) as HTMLElement;
  clone.querySelectorAll('[aria-hidden="true"]').forEach((n) => n.remove());
  const text = (clone.textContent ?? "").replace(/\s+/g, " ").trim();
  if (text) return text;

  const title = el.getAttribute("title");
  if (title?.trim()) return title.trim();

  const alt = el.querySelector("img[alt]")?.getAttribute("alt");
  if (alt?.trim()) return alt.trim();

  return "";
}

describe("① 每个可聚焦元素都有可读名字", () => {
  it("🔴 `Shell` 里**一个都没有例外**", async () => {
    renderShell();
    await waitFor(() => {
      expect(screen.getByRole("banner")).toBeTruthy();
    });

    const all = focusables();
    // ⚠️ 先证明"这里真的有东西" —— 否则下面那条断言在**空数组**上也成立。
    expect(all.length).toBeGreaterThan(6);

    const nameless = all
      .map((el) => ({ el, name: accessibleName(el) }))
      .filter((x) => x.name === "")
      .map((x) => `<${x.el.tagName.toLowerCase()}> ${x.el.outerHTML.slice(0, 90)}`);

    expect(
      nameless,
      `这些可聚焦元素**没有可读名字**（读屏只会念出它的角色）：\n${nameless.join("\n")}`,
    ).toEqual([]);
  });
});

describe("② 焦点顺序是有意义的", () => {
  it("🔴 标题栏的三个窗口控制在**最前面**，且顺序是 最小化 → 最大化 → 关闭", async () => {
    renderShell();
    await waitFor(() => {
      expect(screen.getByRole("banner")).toBeTruthy();
    });
    const names = focusables().map(accessibleName);
    // ⚠️ 那三个在 DOM 里最靠前（标题栏是整个网格的第一行）——
    // 而"最靠前"这件事**必须**成立：一个把它们放在内容之后的实现
    // 会让键盘用户**按十几次 Tab 才能关窗口**。
    expect(names.slice(0, 3)).toEqual(["最小化", "最大化", "关闭"]);
  });

  it("标记与导航都注册成了**地标**（读屏能跳过去）", async () => {
    renderShell();
    await waitFor(() => {
      expect(screen.getByRole("banner")).toBeTruthy();
    });
    // ⚠️ `<header>` 在**不是** `<article>`/`<section>` 的子元素时是 `banner`；
    // `<nav>` 是 `navigation`。而"读屏能不能跳过整块"这件事**只看地标有没有**。
    expect(screen.getByRole("banner")).toBeTruthy();
    expect(screen.getByRole("navigation", { name: "主导航" })).toBeTruthy();
  });

  it("侧栏的每一项都可聚焦，而它们**排在标题栏之后**", async () => {
    renderShell();
    await waitFor(() => {
      expect(screen.getByRole("banner")).toBeTruthy();
    });
    const nav = screen.getByRole("navigation", { name: "主导航" });
    const navLinks = Array.from(nav.querySelectorAll<HTMLElement>("a[href], button"));
    expect(navLinks.length).toBeGreaterThan(3);

    // ⚠️ 而"都排在标题栏之后"用**文档位置**判，而不是用数组下标 ——
    // 下标会随"标题栏里多了几个控件"而变。
    const all = focusables();
    const firstNav = all.indexOf(navLinks[0]!);
    const closeBtn = all.findIndex((e) => accessibleName(e) === "关闭");
    expect(closeBtn).toBeGreaterThanOrEqual(0);
    expect(firstNav).toBeGreaterThan(closeBtn);
  });
});

describe("③ 焦点指示器：**只能核到「有那条规则」**", () => {
  it("标题栏的三个控制有 `:focus-visible` 规则", () => {
    // ⚠️ 用「」而不是半角引号 —— 那是本仓库的一条纪律
    //（在中文串里嵌半角引号会让 TypeScript 解析失败，而报错指向别处）。
    // 而这一行**恰好就是那条纪律的又一个实例**：我写这一条时犯了它。
    // ⚠️ **这条断言比它看起来弱** —— 它读的是**源码**，不是算出来的样式。
    //
    // jsdom 没有布局引擎，所以 `getComputedStyle` 给不出真实的
    // `outline-width`。而"看得见"这件事在自动化里**验不了**。
    //
    // 所以这里的强度是：**"有人删掉那条规则"会被拦**，
    // 而"那条规则在真实浏览器里够不够显眼"**留给人看**。
    const css = readFileSync(join(HERE, "..", "titlebar", "TitleBar.css"), "utf8");
    expect(css).toMatch(/\.titlebar__btn:focus-visible\s*\{/);
    // 而它有真实的描边，不是 `outline: none` 那种"看起来处理了"。
    expect(css).toMatch(/outline:\s*2px\s+solid/);
  });
});

describe("④ `Win + ←/→/↑` 的键盘贴靠 —— 结构前提", () => {
  it("🔴 窗口**可调整大小**（贴靠的必要条件）", () => {
    // ## ⚠️ 而这一条**验不到那个行为本身**，只能验它的前提
    //
    // §4.5.4（`docs/UI设计规格.md` 的「Snap Layouts」一节）把键盘贴靠写成**降级线**：
    //
    // > 若做不到：**降级**：至少支持 `Win + ←/→/↑` 的键盘贴靠 ——
    // > **这不能也不该被自绘窗口破坏**
    //
    // 而"按 Win+← 看它贴不贴"**没有 API 能替我看** —— 那要一个真实
    // 的键盘事件与一个真实的窗口管理器。
    //
    // 所以这里验的是**它的前提**：一个 `resizable: false` 的窗口
    // **一定**不能被贴靠，而一个 `decorations: false` 的窗口**仍然可以**
    //（那是我们在 §4.5 里选 A 计划的代价，而它是可接受的）。
    //
    // **这一条是"必要条件"，不是"充分条件"。** 那句话说清楚了，
    // 下一个人才不会以为键盘贴靠已经被证明。
    const conf = JSON.parse(
      readFileSync(join(HERE, "..", "..", "..", "src-tauri", "tauri.conf.json"), "utf8"),
    ) as { app: { windows: { resizable?: boolean; decorations?: boolean; maximizable?: boolean }[] } };
    const win = conf.app.windows[0];
    expect(win?.resizable, "窗口必须可调整大小，否则 Win+←/→ 一定无效").toBe(true);
    // ⚠️ **`decorations: false` 是我们有意选的**（§4.5 的 A 计划：岛浮在顶部）。
    // 所以这一条断言的是"它确实是 false" —— 那让**将来有人改成 true**
    // 时这条测试红，而那时 §4.5 的整段推理要重新走一遍。
    expect(win?.decorations).toBe(false);
    // ⚠️ 而 `maximizable` **没有在配置里显式写**（默认 `true`）。
    // 这一条把它钉成显式的事实：如果哪天它被写成 `false`，
    // `Win + ↑` 就会失效，而那是**降级线里的一条**。
    expect(win?.maximizable ?? true, "窗口必须可最大化，否则 Win+↑ 无效").toBe(true);
  });
});
