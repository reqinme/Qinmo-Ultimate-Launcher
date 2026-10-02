/**
 * 外壳的行为测试（§4.6.1 / §4.6.1.1 / §4.6.2 / §4.1）
 * ============================================================================
 *
 * ## 🔴 为什么需要这个文件，而 `Shell.a11y.test.tsx` 不够
 *
 * `Shell.a11y.test.tsx` 钉的是 M4 那条验收（**每个可聚焦元素都有名字、
 * 焦点顺序有意义**）。它是一张"最坏情况下也不能退化"的网。
 *
 * 而这一轮给外壳加了四件**有状态**的东西：
 *
 * | 加的东西 | 它能怎么坏 |
 * |---|---|
 * | 窄栏（§4.6.2） | 标签 `display: none` ⇒ 名字从可访问树里**消失** |
 * | 二级栏分区（§4.6.1.1） | 区标题没有 id ⇒ `aria-labelledby` 指向空气 |
 * | 状态条（§4.1） | "更多"展开了而 `aria-expanded` 没变 |
 * | 产品行（§4.6.1.2 单一真源） | 侧栏换了产品而状态条还写着上一个 |
 *
 * 四件都能"看起来对"，而四件都能被测出来。所以下面每一条都对应上表一行。
 *
 * ## ⚠️ 而它**验不了**的两件事要写在这里
 *
 * 1. **窄栏到底有多宽**：jsdom 没有布局引擎，`getComputedStyle` 给不出
 *    真实的 `inline-size`。所以"56 px / 200 px"由
 *    `tools/check-shell-contract.ps1` 在**源码**上核对（规格表 ↔ CSS）。
 * 2. **`display: none` 真的生效了吗**：这里只能验**替代机制存在**
 *    （每个窄栏控件都有一个 `aria-label`）。真正的因果链在 CSS 里，
 *    而它在真窗口里由人眼/截图验。
 */

import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactElement } from "react";
import { describe, expect, it } from "vitest";
import { IslandProvider } from "../island/IslandProvider.tsx";
import { Shell } from "./Shell.tsx";

/**
 * 渲染真的 `Shell`，路由树里放一个 `<p>内容页</p>`。
 *
 * ⚠️ **两个 Provider 不在这里**：`ProductProvider` / `RailPrefProvider`
 * 由 `Shell` 自己挂（见 `Shell.tsx` 顶部）—— 所以"只渲染外壳"的测试
 * 不需要知道它们的存在。而 `IslandProvider` **必须**在外面，
 * 因为 `useIsland()` 在它之外会抛。
 */
function renderShell(path = "/"): void {
  const rootRoute = createRootRoute({ component: Shell });
  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/",
    component: (): ReactElement => <p>内容页</p>,
  });
  const instancesRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/instances",
    component: (): ReactElement => <p>实例列表</p>,
  });
  const downloadsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/downloads",
    component: (): ReactElement => <p>下载目录</p>,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([indexRoute, instancesRoute, downloadsRoute]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  render(
    <IslandProvider>
      <RouterProvider router={router as never} />
    </IslandProvider>,
  );
}

/** 状态条上某一项的值（按左边的名词找）。 */
function statusValue(term: string): string {
  const items = Array.from(document.querySelectorAll<HTMLElement>(".shell__statusItem"));
  const hit = items.find(
    (item) => item.querySelector(".shell__statusLabel")?.textContent === term,
  );
  if (hit === undefined) throw new Error(`状态条上没有「${term}」这一项`);
  return hit.querySelector(".shell__statusValue")?.textContent ?? "";
}

/**
 * ⚠️ **等路由树渲染完。**
 *
 * `createRouter` 的首次匹配是**异步**的（它要 `router.load()`），
 * 所以 `render()` 返回时文档里只有一个空的 `<div />` ——
 * 任何同步查询都会说"没有这个角色"，而那看起来像"外壳坏了"。
 */
async function mounted(): Promise<void> {
  await waitFor(() => {
    expect(screen.getByRole("banner")).toBeTruthy();
  });
}

describe("侧边导航 · 窄栏的替代机制（§4.6.2）", () => {
  it("默认是窄栏，而每一项仍带着非空的可访问名", async () => {
    renderShell();
    await mounted();
    const shell = document.querySelector(".shell");
    expect(shell?.getAttribute("data-rail")).toBe("narrow");

    const nav = screen.getByRole("navigation", { name: "主导航" });
    const links = within(nav).getAllByRole("link");
    // 七个一级项 + 产品段的两行
    expect(links.length).toBeGreaterThan(6);
    for (const link of links) {
      const label = link.getAttribute("aria-label");
      expect(label).toBeTruthy();
      // ⚠️ 这一条是**窄栏的关键**：标签那一段被 `display: none` 藏掉之后，
      // 可访问名只能来自 `aria-label` 自己。
      expect(link.textContent).not.toBe("");
    }
  });

  it("窄栏的气泡文字来自 `data-tip`，而它与可访问名一致", async () => {
    renderShell();
    await mounted();
    const nav = screen.getByRole("navigation", { name: "主导航" });
    for (const link of within(nav).getAllByRole("link")) {
      // CSS 里气泡是 `content: attr(data-tip)` —— 两者不一致时，
      // 鼠标用户和读屏用户会看到/听到两个不同的名字。
      expect(link.getAttribute("data-tip")).toBe(link.getAttribute("aria-label"));
    }
  });
});

describe("二级栏的分区（§4.6.1.1）", () => {
  it("「实例」页有两个区，而每个区标题的 id 与 `aria-labelledby` 对得上", async () => {
    renderShell("/instances");
    await mounted();
    const secondary = screen.getByRole("navigation", { name: "二级导航" });
    const sections = Array.from(secondary.querySelectorAll(".shell__secondarySection"));
    expect(sections.length).toBe(2);

    const ids = sections.map((section) => {
      const heading = section.querySelector("h2");
      const labelledBy = section.getAttribute("aria-labelledby");
      expect(heading).not.toBeNull();
      expect(labelledBy).toBe(heading?.getAttribute("id"));
      // ⚠️ `aria-labelledby` 指向一个**不存在**的 id 是"无效文档"里
      // 最常见的一种，而读屏在这种情况下会静默地不念任何东西。
      expect(document.getElementById(labelledBy ?? "")).not.toBeNull();
      return labelledBy;
    });
    expect(ids).toEqual(["shell-section-product", "shell-section-group"]);
  });

  it("没有二级栏的一级（主页）真的没有第二个 `nav`", async () => {
    renderShell("/");
    await mounted();
    expect(screen.queryByRole("navigation", { name: "二级导航" })).toBeNull();
  });
});

describe("状态条（§4.1）", () => {
  it("「更多」展开详情、再点收起，而 `aria-expanded` 跟着变", async () => {
    renderShell();
    await mounted();
    const more = screen.getByRole("button", { name: "更多" });
    expect(more.getAttribute("aria-expanded")).toBe("false");
    // 收起时详情项不在文档里（不是 `display:none`）——
    // 于是读屏用户不会 Tab 到一个看不见的东西上。
    expect(document.getElementById("shell-status-details")).toBeNull();

    fireEvent.click(more);
    const collapse = screen.getByRole("button", { name: "收起详情" });
    expect(collapse.getAttribute("aria-expanded")).toBe("true");
    const details = document.getElementById("shell-status-details");
    expect(details).not.toBeNull();
    expect(details?.textContent).toContain("运行时");
    expect(details?.textContent).toContain("内存占用");

    fireEvent.click(collapse);
    expect(document.getElementById("shell-status-details")).toBeNull();
  });

  it("常驻的四项里，没有来源的格子写 `—` 而不是 0", async () => {
    renderShell();
    await mounted();
    expect(statusValue("网速")).toBe("—");
    expect(statusValue("源")).toBe("官方源");
    expect(statusValue("当前产品")).toBe("原生版");
  });
});

describe("「当前产品」只有一个真源（§4.6.1.2）", () => {
  it("在二级栏点另一个产品，侧栏那一行与状态条同时跟着变", async () => {
    renderShell("/instances");
    await mounted();
    const before = screen.getAllByRole("button", { name: "原生版" });
    for (const button of before) expect(button.getAttribute("aria-pressed")).toBe("true");

    // 第二个产品在**两处**各有一个控件：侧栏产品段 + 二级栏的「产品」区。
    const next = screen.getAllByRole("button", { name: "另一形态" });
    expect(next.length).toBe(2);
    fireEvent.click(next[next.length - 1] as HTMLElement);

    // ① 侧栏那一行
    for (const button of screen.getAllByRole("button", { name: "另一形态" })) {
      expect(button.getAttribute("aria-pressed")).toBe("true");
    }
    // ② 状态条（它是**另一个消费者**，不是同一段 JSX 的第二次渲染）
    expect(statusValue("当前产品")).toBe("另一形态");
  });
});

describe("二级项的落点（检查 G 的行为侧）", () => {
  /**
   * ⚠️ 这一组钉的是**一次真缺陷**，而它当时穿过了所有测试：
   *
   * 二级项的路径此前是机械拼的（`一级路径 + "/" + 项 key`），于是
   * 「全部实例」指向 `/instances/all`、「账户列表」指向 `/accounts/list`、
   * 「内建帮助」指向 `/toolbox/help`、「外观与材质」指向 `/settings/appearance`
   * —— **四条路由都不存在**（`secondaryHref()` 的返回类型是 `string`，
   * 所以类型系统一个字都没说），真窗口里点下去是 `Not Found`。
   *
   * 而它们的正确落点就是一级页本身（`/instances` 是全部实例、`/accounts`
   * 是账户列表、`/toolbox` 是内建帮助、`/settings` 是外观与材质）。
   * 下面钉的是**渲染出来的 `href`** —— 也就是用户真正会点的那一个属性。
   */
  function secondaryHrefs(): ReadonlyMap<string, string> {
    const anchors = document.querySelectorAll<HTMLAnchorElement>("a.shell__secondaryItem");
    return new Map([...anchors].map((a) => [a.textContent ?? "", a.getAttribute("href") ?? ""]));
  }

  it("「全部实例」落在这一页本身，「最近使用 / 收藏」各有自己的路径", async () => {
    renderShell("/instances");
    await mounted();
    const hrefs = secondaryHrefs();
    expect(hrefs.get("全部实例")).toBe("/instances");
    expect(hrefs.get("最近使用")).toBe("/instances/recent");
    expect(hrefs.get("收藏")).toBe("/instances/favorites");
  });

  it("下载页的「类型」每一项都有自己的路径（一级页是目录，不是第一项）", async () => {
    renderShell("/downloads");
    await mounted();
    const hrefs = secondaryHrefs();
    expect(hrefs.get("游戏版本")).toBe("/downloads/versions");
    expect(hrefs.get("加载器")).toBe("/downloads/loaders");
  });
});
