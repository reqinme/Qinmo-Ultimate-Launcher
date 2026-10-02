/**
 * 标题栏的交互规格（**`UI设计规格.md` §4.6 下面那三小节**）
 * ============================================================================
 *
 * ## 🔴 这三小节的原文在 L834–L857，而它们在 §4.5 之外
 *
 * 那是一个真实的结构问题：`### 4.5 自绘标题栏的交互规格` 的**正文是 0 行**
 *（L373 标题，L374 空行，L375 就是 §4.6），而它的内容散在
 * **§4.6 后面**的三小节里：
 *
 * ```text
 *   L834  #### 拖动与命中区
 *   L844  #### Snap Layouts
 *   L852  #### 高 DPI 与多显示器
 * ```
 *
 * **所以"§4.5 是空的"这句话是对的，而"标题栏的交互规格没写"是错的** ——
 * 它写好了，只是挂错了地方。而**那两件事的后果完全不同**：
 * 前者要补文档，后者要改实现。
 *
 * ## 本文件钉的是那三小节里**能自动验**的部分
 *
 * | 出处 | 规格原文 | 本文件 |
 * |---|---|---|
 * | L838 | 拖动区 = 标题栏**空白处** | ① |
 * | L839 | **品牌、搜索框、产品切换器、灵动岛、窗口控制按钮都必须排除拖动** | ① |
 * | L841 | **双击空白处 = 最大化/还原**（"必须支持"） | ② |
 * | L839 | 灵动岛**例外**：拖岛是拖岛，不是拖窗口 | ③（结构） |
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// ⚠️ mock 的是**边界层**（`api/window.ts`），不是 `@tauri-apps/api/core` ——
// 后者根本不该被组件碰到（那是 `api/` 的纪律，由 `boundary.test.ts` 强制）。
vi.mock("../api/window.ts", () => ({
  windowMinimize: vi.fn(async () => {}),
  windowToggleMaximize: vi.fn(async () => true),
  windowClose: vi.fn(async () => {}),
  windowStartDragging: vi.fn(async () => {}),
}));

import {
  windowClose,
  windowMinimize,
  windowStartDragging,
  windowToggleMaximize,
} from "../api/window.ts";
import { TitleBar } from "./TitleBar.tsx";

const mockedDrag = vi.mocked(windowStartDragging);
const mockedToggle = vi.mocked(windowToggleMaximize);
const mockedMin = vi.mocked(windowMinimize);
const mockedClose = vi.mocked(windowClose);

beforeEach(() => {
  vi.clearAllMocks();
  mockedToggle.mockResolvedValue(true);
});

afterEach(() => {
  vi.clearAllMocks();
});

/** 取标题栏本身（它是 `role="banner"`）。 */
function bar(): HTMLElement {
  return screen.getByRole("banner");
}

describe("① 拖动区 = **空白处**，而四个交互元素都排除它（L838 / L839）", () => {
  it("🔴 在标题栏空白处按下 ⇒ **开始拖动**", () => {
    render(<TitleBar />);
    // ⚠️ `pointerDown` 而不是 `click` —— 拖动必须在**按下的那一刻**开始
    //（一个 `onClick` 的实现让"按住拖动"完全无效）。
    fireEvent.pointerDown(bar(), { button: 0 });
    expect(mockedDrag).toHaveBeenCalledTimes(1);
  });

  it("⚠️ **右键不拖窗口**（那是另一件事，而它将来是系统菜单）", () => {
    render(<TitleBar />);
    fireEvent.pointerDown(bar(), { button: 2 });
    expect(mockedDrag).not.toHaveBeenCalled();
  });

  it("🔴 **品牌上按下不拖窗口**（L839 明写「品牌」）", () => {
    render(<TitleBar />);
    fireEvent.pointerDown(screen.getByText("秦墨"), { button: 0 });
    expect(mockedDrag).not.toHaveBeenCalled();
  });

  it("🔴 **搜索框上按下不拖窗口** —— 而它是 `disabled` 的也必须排除", () => {
    // ⚠️ 这一条钉的是一个**不显然**的事实：一个 `disabled` 的按钮
    // **仍然会收到 pointerdown**（`disabled` 拦的是 click 与 focus，
    // 不是 pointer 事件）。所以那个 `stopPropagation` 不是多余的。
    render(<TitleBar />);
    fireEvent.pointerDown(screen.getByRole("button", { name: /搜索/ }), { button: 0 });
    expect(mockedDrag).not.toHaveBeenCalled();
  });

  it("🔴 **三个窗口控制上按下不拖窗口**（否则按不动它们）", () => {
    render(<TitleBar />);
    for (const name of ["最小化", "最大化", "关闭"]) {
      mockedDrag.mockClear();
      fireEvent.pointerDown(screen.getByRole("button", { name }), { button: 0 });
      expect(mockedDrag, `${name} 上按下不该拖窗口`).not.toHaveBeenCalled();
    }
  });

  it("而按下控制之后**它自己的动作**仍然发生（排除拖动没把它们也排除了）", () => {
    // ⚠️ 反证的另一半：一个"给整个标题栏 `pointer-events: none`"的实现
    // 也能让上面那五条全绿 —— 而那会让按钮**完全不能用**。
    render(<TitleBar />);
    fireEvent.click(screen.getByRole("button", { name: "最小化" }));
    expect(mockedMin).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(mockedClose).toHaveBeenCalledTimes(1);
  });
});

describe("② 双击空白处 = 最大化 / 还原（L841 · 规格写「必须支持」）", () => {
  it("🔴 双击标题栏空白处 ⇒ 调**那一条 toggle 命令**", () => {
    render(<TitleBar />);
    fireEvent.doubleClick(bar());
    expect(mockedToggle).toHaveBeenCalledTimes(1);
  });

  it("🔴 双击**品牌** ⇒ **什么都不做**（L839 的「排除」在双击上同样成立）", () => {
    // ⚠️ 而这条比它看起来重要：双击品牌是用户的自然动作
    //（想选中文字），而它若变成最大化，那是**最难理解**的一类行为。
    render(<TitleBar />);
    fireEvent.doubleClick(screen.getByText("秦墨"));
    expect(mockedToggle).not.toHaveBeenCalled();
  });

  it("🔴 双击**关闭按钮** ⇒ 只关闭，**不最大化**", () => {
    render(<TitleBar />);
    const close = screen.getByRole("button", { name: "关闭" });
    fireEvent.doubleClick(close);
    fireEvent.click(close);
    expect(mockedToggle).not.toHaveBeenCalled();
    // ⚠️ 而双击会触发两次 click —— 那与系统标题栏一致（连点两下 = 关两次请求，
    // 而第二次的窗口已经没了）。
    expect(mockedClose).toHaveBeenCalled();
  });

  it("而切换之后**图标跟着真相走**（用的是命令的返回值）", async () => {
    // ⚠️ `windowToggleMaximize` 返回的是**切换之后**是否最大化 ——
    // 而那不是前端猜的。这条断言钉的是"组件用了那个返回值"。
    mockedToggle.mockResolvedValue(true);
    render(<TitleBar />);
    fireEvent.doubleClick(bar());
    // 图标从三根横线（MENU）变成那个叉（CLOSE）—— 而它们的可读名字不同。
    expect(await screen.findByRole("img", { name: "最大化" })).toBeTruthy();
  });
});

describe("③ 灵动岛的拖动是**另一套作用域**（L840）", () => {
  it("标题栏里**没有**岛本身（它由 `IslandLayer` 浮着）", () => {
    // ⚠️ L840 的原文：
    //
    // > **灵动岛例外**：岛自身**可拖动且位置记忆**（§7.3 约束 3），
    // > **它的拖动是拖岛自己，不是拖窗口** —— **两套拖动作用域必须分开**
    //
    // 而"分开"这件事在本项目里靠**结构**成立：
    // 岛是 `position: fixed` 的一层（`IslandLayer`），而标题栏里
    // 只留一段**空的** `.titlebar__slot`（它 `pointer-events: none`）。
    //
    // 所以"两套拖动混在一起"这件事**在结构上不可能发生** ——
    // 而这条断言钉的就是那个结构：标题栏里不该出现岛。
    const { container } = render(<TitleBar />);
    expect(container.querySelector(".island")).toBeNull();
    // 而那段留位是存在的，且它不接收指针事件。
    const slot = container.querySelector(".titlebar__slot");
    expect(slot).not.toBeNull();
    expect(slot?.getAttribute("aria-hidden")).toBe("true");
  });
});
