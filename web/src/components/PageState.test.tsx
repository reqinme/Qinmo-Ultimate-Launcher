/**
 * `PageState` 的测试（§7.6 的六条规则里，机器能核对的那几条）
 * ============================================================================
 *
 * ## 这些用例在防什么
 *
 * | 用例 | §7.6 的哪一条 |
 * |---|---|
 * | 加载态是骨架屏、**不是转圈** | 规则 2 |
 * | 骨架屏最短 300 ms | 规则 6 |
 * | 空态 **不出现"暂无数据"** | 规则 3（+ §4.3.1 的措辞纪律） |
 * | 错误态是"码 + 原因 + 建议 + 操作"四件套 | 规则 4 |
 * | 三态类名一致（`pageState--<kind>`） | 规则 1（同一个容器） |
 *
 * ## ⚠️ 300 ms 那条**必须**用假计时器
 *
 * 用真的 `setTimeout` 等 300 ms，会让这个文件成为整个测试套件里最慢的一个，
 * 而它测的恰恰是"时间"这件事 —— 假计时器把时间变成可复现的输入。
 * 每个用了假计时器的用例都在 `finally` 里恢复真计时器：
 * 忘了恢复会**污染同一个文件里后面的用例**（它们会一起卡住不动）。
 *
 * ## 断言的两个习惯
 *
 * - 查"没有转圈"用 `queryByRole("progressbar")` 而不是查类名：
 *   `.spinner` 只是**目前**的写法，而"加载态不许宣告进度"是规格。
 * - 对比文本一律用 `textContent ?? ""`（`noUncheckedIndexedAccess` 之下
 *   `textContent` 是 `string | null`，直接 `toContain` 会过不了类型检查）。
 */

import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PageState, type PageStateProps } from "./PageState.tsx";

/** 渲染一个 `PageState`。**每个用例都从干净的 DOM 开始**（testing-library 自动清理）。 */
function renderState(props: PageStateProps): HTMLElement {
  return render(<PageState {...props} />).container;
}

/**
 * 推进假计时器。
 *
 * ⚠️ **必须包在 `act()` 里**：计时器到点之后 React 要处理一次状态更新，
 * 而 `vi.advanceTimersByTime()` 自己不会把它 flush 掉 —— 不包就会出现
 * "计时器明明触发了，屏幕上却还是骨架屏"这种**假红**，
 * 而它会让人去改实现（把 300 ms 调小 / 干脆去掉等待），把真行为改坏。
 */
function advance(ms: number): void {
  act(() => {
    vi.advanceTimersByTime(ms);
  });
}

/** 收集容器上所有以 `pageState--` 开头的类名（态类）。 */
function stateClasses(container: HTMLElement): readonly string[] {
  const element = container.querySelector(".pageState");
  if (element === null) throw new Error("没有渲染出 .pageState 容器");
  return Array.from(element.classList).filter((name) => name.startsWith("pageState--"));
}

describe("PageState · Loading（§7.6 规则 2）", () => {
  it("画骨架屏并宣告 aria-busy，而不是转圈", () => {
    const container = renderState({ kind: "loading", title: "存档列表", lines: 3 });

    // 区域是可命名的（局部失败/局部加载时，读屏用户需要知道"哪一块"）
    const region = screen.getByRole("region", { name: "存档列表" });
    expect(region.getAttribute("aria-busy")).toBe("true");

    // 骨架行数 = 传进去的 lines
    expect(container.querySelectorAll(".pageState__skeletonLine")).toHaveLength(3);
    // 骨架屏对读屏是**装饰**：它由 aria-busy 代表
    expect(container.querySelector(".pageState__skeleton")?.getAttribute("aria-hidden")).toBe("true");

    // ⑧ 反向断言：没有进度条、没有转圈。
    //    这一条是规则 2 的机器版 —— 一旦有人"顺手"加个 Spinner，它立刻红。
    expect(screen.queryByRole("progressbar")).toBeNull();
    expect(container.querySelector(".spinner")).toBeNull();
    expect(container.querySelector(".ring")).toBeNull();
  });

  it("最后一行短一截：骨架屏要暗示「这里是一段正文」", () => {
    const container = renderState({ kind: "loading", title: "存档列表", lines: 3 });
    const lines = Array.from(container.querySelectorAll(".pageState__skeletonLine"));

    expect(lines[lines.length - 1]?.classList.contains("pageState__skeletonLine--short")).toBe(true);
    expect(lines[0]?.classList.contains("pageState__skeletonLine--short")).toBe(false);
  });

  it("行数默认不是 0：lines 未给时仍然有骨架（空盒子等于没有加载指示）", () => {
    const container = renderState({ kind: "loading", title: "存档列表" });
    expect(container.querySelectorAll(".pageState__skeletonLine").length).toBeGreaterThan(0);
  });
});

describe("PageState · 骨架屏最短显示 300 ms（§7.6 规则 6）", () => {
  afterEach(() => {
    // ⚠️ 恢复真计时器：它会**跨用例**影响同一个文件里的其他测试
    vi.useRealTimers();
  });

  it("刚进加载态就切走：300 ms 之内仍然画骨架屏，过了才换内容", () => {
    vi.useFakeTimers();

    const { rerender } = render(<PageState kind="loading" title="存档列表" lines={3} />);
    expect(document.querySelector(".pageState--loading")).not.toBeNull();

    // "闪一下就没"的那个瞬间：数据在 0 ms 就到了
    rerender(
      <PageState
        kind="empty"
        title="还没有存档"
        body="创建第一个存档后，它会出现在这里。"
        action={<button type="button">新建存档</button>}
      />,
    );

    // ① 还没到 300 ms —— 屏幕上**还是**骨架屏，而不是空态
    expect(document.querySelector(".pageState--loading")).not.toBeNull();
    expect(document.querySelector(".pageState--empty")).toBeNull();
    expect(screen.queryByRole("button", { name: "新建存档" })).toBeNull();

    // ② 差一点点：还在显示 —— ⚠️ 目标时间从"切走的那一帧"算起，
    //    而那一帧的 `Date.now()` 可能与进入加载态时**同一毫秒**
    //    （`remaining` = 300 而不是 299），所以这里推进 200 而不是 299。
    advance(200);
    expect(document.querySelector(".pageState--loading")).not.toBeNull();

    // ③ 到点：交出错过的那个空态
    advance(200);
    expect(document.querySelector(".pageState--loading")).toBeNull();
    expect(screen.getByRole("button", { name: "新建存档" })).toBeTruthy();
  });

  it("加载超过 300 ms 再切走：不再额外等待，立刻换内容", () => {
    vi.useFakeTimers();

    const { rerender } = render(<PageState kind="loading" title="存档列表" />);
    // 真实地"加载了很久"
    advance(1000);

    rerender(
      <PageState
        kind="empty"
        title="还没有存档"
        body="创建第一个存档后，它会出现在这里。"
        action={<button type="button">新建存档</button>}
      />,
    );

    // 已经显示满 300 ms 了，所以**这一帧**就该是空态
    expect(document.querySelector(".pageState--loading")).toBeNull();
    expect(screen.getByRole("button", { name: "新建存档" })).toBeTruthy();
  });

  it("骨架屏又回来了就不会被上一轮计时器提前收掉（重试 / 轮询）", () => {
    vi.useFakeTimers();

    const { rerender } = render(<PageState kind="loading" title="存档列表" />);
    rerender(
      <PageState
        kind="empty"
        title="还没有存档"
        body="创建第一个存档后，它会出现在这里。"
        action={<button type="button">新建存档</button>}
      />,
    );
    // 计时器还没到，用户点了"重试" ⇒ 又回到加载态
    rerender(<PageState kind="loading" title="存档列表" />);

    // 上一轮的计时器到点了，但**这一趟加载**才刚开始
    advance(400);
    expect(document.querySelector(".pageState--loading")).not.toBeNull();
  });

  it("失败不等待：error 态一拿到就上屏（压在骨架屏后面会像「卡住」）", () => {
    vi.useFakeTimers();

    const { rerender } = render(<PageState kind="loading" title="存档列表" />);
    rerender(
      <PageState
        kind="error"
        title="存档列表"
        code="QINMO-STORAGE-002"
        reason="读取存档目录时被拒绝访问。"
        advice="关闭正在占用该目录的程序，然后重试。"
        actions={<button type="button">重试</button>}
      />,
    );

    expect(screen.getByRole("alert")).toBeTruthy();
    expect(document.querySelector(".pageState--loading")).toBeNull();
  });
});

describe("PageState · Empty（§7.6 规则 3 + §4.3.1）", () => {
  it("三样都给：这是干什么的 / 推荐从哪开始 / 怎么看到全部", () => {
    const container = renderState({
      kind: "empty",
      title: "这里还没有存档",
      body: "存档记录你的进度，创建后会在这里列出。",
      action: <button type="button">新建存档</button>,
    });

    expect(screen.getByRole("region", { name: "这里还没有存档" })).toBeTruthy();
    expect(screen.getByRole("heading", { level: 2, name: "这里还没有存档" })).toBeTruthy();
    expect(screen.getByText("存档记录你的进度，创建后会在这里列出。")).toBeTruthy();
    expect(screen.getByRole("button", { name: "新建存档" })).toBeTruthy();

    // 没给 icon ⇒ 用内置的自绘图（48 px 线稿，见 PageState.tsx）
    const icon = container.querySelector("svg.pageState__icon");
    expect(icon?.getAttribute("aria-hidden")).toBe("true");
    expect(icon?.getAttribute("width")).toBe("48");
    expect(icon?.getAttribute("height")).toBe("48");
  });

  it("不给 icon 时**不画 emoji、不画彩色插画**，只画一个自绘线稿", () => {
    const container = renderState({
      kind: "empty",
      title: "这里还没有存档",
      body: "存档记录你的进度，创建后会在这里列出。",
      action: <button type="button">新建存档</button>,
    });

    // 自绘的两个证据：`currentColor` + `fill: none`（§4.3.1：图标继承文字色）
    const icon = container.querySelector("svg.pageState__icon");
    expect(icon?.getAttribute("stroke")).toBe("currentColor");
    expect(icon?.getAttribute("fill")).toBe("none");
    // 线稿之外不该有第二个图形（emoji 会作为文本节点混进文案里）
    expect(container.querySelectorAll("svg")).toHaveLength(1);
  });

  it("给了 icon 就用它，并且仍然是同一套类名", () => {
    const container = renderState({
      kind: "empty",
      title: "收藏夹是空的",
      body: "把常用工具固定到这里，下次一步就能打开。",
      action: <button type="button">去全部工具</button>,
      icon: <span data-testid="my-icon" />,
    });

    expect(container.querySelector("[data-testid='my-icon']")).not.toBeNull();
    expect(container.querySelector("svg.pageState__icon")).toBeNull();
    expect(stateClasses(container)).toEqual(["pageState--empty"]);
  });

  it("**不出现「暂无数据」**（§4.3.1 的措辞纪律）", () => {
    const container = renderState({
      kind: "empty",
      title: "这里还没有存档",
      body: "存档记录你的进度，创建后会在这里列出。",
      action: <button type="button">新建存档</button>,
    });

    expect(container.textContent ?? "").not.toContain("暂无数据");
  });
});

describe("PageState · Error（§7.6 规则 4）", () => {
  it("四件套都在：错误码 + 原因 + 建议 + 操作，且容器是 role=alert", () => {
    const container = renderState({
      kind: "error",
      title: "存档列表",
      code: "QINMO-STORAGE-002",
      reason: "读取存档目录时被拒绝访问。",
      advice: "关闭正在占用该目录的程序，然后重试。",
      actions: (
        <>
          <button type="button">重试</button>
          <button type="button">导出诊断</button>
        </>
      ),
    });

    // alert 是这里唯一的状态播报（加载态不许有）
    const alert = screen.getByRole("alert");
    expect(alert.classList.contains("pageState--error")).toBe(true);

    expect(screen.getByText("QINMO-STORAGE-002")).toBeTruthy();
    expect(screen.getByText("读取存档目录时被拒绝访问。")).toBeTruthy();
    expect(screen.getByText("关闭正在占用该目录的程序，然后重试。")).toBeTruthy();
    expect(screen.getByRole("button", { name: "重试" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "导出诊断" })).toBeTruthy();

    // 图形是内置的（属性里没有 icon，失败的样子由容器负责）
    expect(container.querySelector("svg.pageState__icon")).not.toBeNull();
  });
});

describe("PageState · 三态共用同一套类名（§7.6 规则 1）", () => {
  it("每个态都带 pageState + pageState--<kind>，且只有态类不同", () => {
    const cases: readonly { readonly props: PageStateProps; readonly state: string }[] = [
      { props: { kind: "loading", title: "列表" }, state: "pageState--loading" },
      {
        props: {
          kind: "empty",
          title: "还没有内容",
          body: "内容会在这里列出。",
          action: <button type="button">开始</button>,
        },
        state: "pageState--empty",
      },
      {
        props: {
          kind: "error",
          title: "列表",
          code: "QINMO-STORAGE-002",
          reason: "读取失败。",
          advice: "重试一次。",
          actions: <button type="button">重试</button>,
        },
        state: "pageState--error",
      },
    ];

    for (const { props, state } of cases) {
      const container = renderState(props);
      const element = container.querySelector(".pageState");
      expect(element).not.toBeNull();
      expect(element?.classList.contains(state)).toBe(true);
      // 只有一个态类 —— 两个态类同时挂着就是"两个态同时成立"，那不可能
      expect(stateClasses(container)).toEqual([state]);
    }
  });
});
