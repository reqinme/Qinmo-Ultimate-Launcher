/**
 * 事件契约的**行为**验收（`docs/UI设计规格.md` §4.5.1）
 * ============================================================================
 *
 * ## 这个文件与 `TitleBar.test.tsx` 的分工
 *
 * `TitleBar.test.tsx` 是**手写的、逐条**的（"品牌上按下不拖窗口"一条、
 * "双击关闭只关闭"一条……）。它抓到了那个真 bug，而它有一个结构性弱点：
 * **它只覆盖有人想到要写的那几格。**
 *
 * 本文件换一个方向：**从契约表出发遍历**。于是它的断言数不是人写的，
 * 而是 `元素数 × 事件族数` —— 而现在"漏一格"这件事有了两种红的可能：
 *
 * | 漏的类型 | 谁红 |
 * |---|---|
 * | 表里没写（而根部那道闸要吃它） | 本文件（表 ↔ DOM 的集合断言） |
 * | 表里写了、而根部那道闸没兑现 | 本文件（逐格派发） |
 * | 表与规格对不上 | `tools/check-titlebar-contract.ps1` |
 *
 * ⚠️ **而这一遍遍历当场抓到过一个真缺陷**：`disabled` 的搜索框**收不到
 * React 的合成 `onDoubleClick`**（React 对"实际禁用"的表单元素不派发鼠标类
 * 合成事件）—— 于是"把排除挂在元素自己身上"漏了一格：双击搜索框冒到根部，
 * 窗口被最大化。那道闸因此搬到了**根部**（见 `contract.ts` 的 `swallows()`）。
 * 而 `TitleBar.test.tsx` 当初只测了搜索框的 `pointerdown`，所以它一直绿着。
 *
 * ⚠️ **而"根部到底处理哪几个族"这件事本身也是被钉住的**：
 * 根部处理 `pointerdown`（拖窗口）与 `dblclick`（最大化/还原），
 * 而 `contextmenu` 是**决定不做**（§4.5.1 的决定 1）—— 所以第三块的断言是
 * "**右键在任何元素上都不引发窗口命令**"，而不是"右键应该做点什么"。
 */

import { fireEvent, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// ⚠️ mock 的是**边界层**（`api/window.ts`），不是 `@tauri-apps/api/core` ——
// 后者根本不该被组件碰到（那是 `api/` 的纪律，由 `boundary.test.ts` 强制）。
vi.mock("../api/window.ts", () => ({
  windowMinimize: vi.fn(async () => {}),
  windowToggleMaximize: vi.fn(async () => true),
  windowClose: vi.fn(async () => {}),
  windowStartDragging: vi.fn(async () => {}),
}));

import { windowStartDragging, windowToggleMaximize } from "../api/window.ts";
import {
  EVENT_FAMILIES,
  EVERY_FAMILY_IS_COVERED,
  TITLEBAR_ITEMS,
  itemOf,
  swallows,
  type EventFamily,
  type TitlebarId,
} from "./contract.ts";
import { TitleBar } from "./TitleBar.tsx";

const mockedDrag = vi.mocked(windowStartDragging);
const mockedToggle = vi.mocked(windowToggleMaximize);

/** 根部处理的两个族各自的**可观测副作用**（用它判断"到底冒到根部没有"）。 */
const ROOT_EFFECTS = { pointerdown: mockedDrag, dblclick: mockedToggle } as const;

/** 根部真的处理的那两个族（`contextmenu` 不在其中，见 §4.5.1 的决定 1）。 */
type HandledFamily = keyof typeof ROOT_EFFECTS;

const HANDLED_FAMILIES: readonly HandledFamily[] = ["pointerdown", "dblclick"];

beforeEach(() => {
  vi.clearAllMocks();
  mockedToggle.mockResolvedValue(true);
});

/** 在一个元素上派发某个事件族。 */
function fire(el: Element, family: EventFamily): void {
  const target = el as HTMLElement;
  if (family === "pointerdown") {
    // ⚠️ 必须带 `button: 0` —— 根部只响应主键（右键拖动是另一件事）。
    fireEvent.pointerDown(target, { button: 0 });
  } else if (family === "dblclick") {
    fireEvent.doubleClick(target);
  } else {
    fireEvent.contextMenu(target);
  }
}

/** 渲染一次标题栏，并按 `data-titlebar-item` 取元素。 */
function bar(): { container: HTMLElement; el: (id: TitlebarId) => Element } {
  const { container } = render(<TitleBar />);
  return {
    container,
    el: (id) => {
      const found = container.querySelector(`[data-titlebar-item="${id}"]`);
      if (found === null) throw new Error(`DOM 里没有这个元素：${id}`);
      return found;
    },
  };
}

const DONE = TITLEBAR_ITEMS.filter((item) => item.status === "done");
const NOT_DONE = TITLEBAR_ITEMS.filter((item) => item.status !== "done");

describe("① 表 ↔ DOM：没有一个元素能绕过契约", () => {
  it("🔴 DOM 里的 `data-titlebar-item` 集合**正好等于**表里 `done` 的那些 id", () => {
    // ⚠️ 这条是"新增一个元素却忘了写进表"的拦截器：DOM 里会多出一个没有
    // 表行的 id ⇒ 两边集合不等 ⇒ 红。反过来，"表里写了 done 而元素被删了"
    // 也在这里红 —— 两个方向都拦。
    const { container } = bar();
    const inDom = [...container.querySelectorAll("[data-titlebar-item]")]
      .map((el) => el.getAttribute("data-titlebar-item"))
      .sort();
    const declared = DONE.map((item) => item.id).sort();
    expect(inDom).toEqual(declared);
  });

  it("🔴 `pending` / `structural` 的元素**此刻不在** DOM 里", () => {
    // 产品切换器（§4.4）是 `pending`：它一旦被画出来就必须按表排除，
    // 而"画了半个"（既不在表里又不完整）在这里红。
    // 灵动岛是 `structural`：它**不该**出现在标题栏的子树里（§4.5.3 的例外）。
    const { container } = bar();
    for (const item of NOT_DONE) {
      expect(
        container.querySelector(`[data-titlebar-item="${item.id}"]`),
        `${item.label} 的状态是 ${item.status}，不该出现在标题栏里`,
      ).toBeNull();
    }
  });

  it("表本身是自洽的：id 唯一、每一行的三个族都有一格", () => {
    const ids = TITLEBAR_ITEMS.map((item) => item.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const item of TITLEBAR_ITEMS) {
      for (const family of EVENT_FAMILIES) {
        expect(
          ["swallow", "bubble", "skip", "na"],
          `${item.label} 的 ${family} 那一格不是合法符号`,
        ).toContain(item.cells[family]);
      }
    }
  });

  it("⚠️ 编译期的闩与运行时一致：`EVERY_FAMILY_IS_COVERED` 覆盖了每一个族", () => {
    // 那个常量存在的意义是"加一个族就编译不过"。这条是它的运行时镜像 ——
    // 万一有人把类型放宽（`Record<string, true>`），它仍会红。
    expect(Object.keys(EVERY_FAMILY_IS_COVERED).sort()).toEqual([...EVENT_FAMILIES].sort());
  });

  it("`itemOf` 对表里的每一行都取得回**同一行**", () => {
    // ⚠️ 而"取不到就抛"那条路径**故意不在这里造**：`TitlebarId` 是字面量联合，
    // 要绕过去得写两处 `as`（`as unknown as`），而**两处 `as` 本身是更坏的东西** ——
    // 它会把"类型系统挡住了这个错误"变成一个测试里的人为构造。
    for (const item of TITLEBAR_ITEMS) {
      expect(itemOf(item.id).id).toBe(item.id);
    }
  });
});

describe("② 逐格派发：`吞` 的到不了根部，`冒` 的会到（§4.5.1 那张表）", () => {
  // ⚠️ 测试是**从表里长出来的**：遍历 `done` 的元素 × 根部处理的族。
  for (const item of DONE) {
    for (const family of HANDLED_FAMILIES) {
      const cell = item.cells[family];
      const verb = cell === "swallow" ? "吞 ⇒ 到不了根部" : "冒 ⇒ 根部处理它";
      it(`${item.label} · ${family} —— ${verb}`, () => {
        const { el } = bar();
        fire(el(item.id), family);
        const effect = ROOT_EFFECTS[family];
        if (cell === "swallow") {
          expect(effect, `${item.label} 上的 ${family} 不该冒到标题栏根部`).not.toHaveBeenCalled();
        } else {
          expect(effect, `${item.label} 上的 ${family} 应当由根部处理`).toHaveBeenCalledTimes(1);
        }
      });
    }
  }
});

describe("③ 右键：§4.5.1 的决定 1 是**不做**，而那是可断言的", () => {
  it("🔴 在**任何**元素上右键都不引发窗口命令（没有 `contextmenu` 处理器）", () => {
    // ⚠️ 这条钉的是"一个明确的'不做'"。一个将来给空白处接上系统菜单的实现
    // 会走别的路径（`WM_NCRBUTTONUP` / `TrackPopupMenu`，在 Rust 侧），
    // 于是它**不会**让这条红 —— 而如果有人在**前端**加一个 contextmenu
    // 处理器去调窗口命令，这条就会红。
    const { container, el } = bar();
    expect(container.querySelector("[data-titlebar-item]")).not.toBeNull();
    for (const item of DONE) {
      fire(el(item.id), "contextmenu");
    }
    expect(mockedDrag).not.toHaveBeenCalled();
    expect(mockedToggle).not.toHaveBeenCalled();
  });
});

describe("④ 派生关系本身：`swallows()` 对每一格给出的答案就是表里的那个字", () => {
  it("🔴 每一个元素 × 每一个族的答案**正好**对应它那一格是不是 `吞`", () => {
    // 这条是"表 ⇒ 根部那道闸"的单元侧证据：只要有人把 `swallows()` 改成
    // "一律吞"或者"看 id 前缀"，它立刻红。
    // ⚠️ 而它**不能**替代行为测试（②）：② 问的是"根部到底有没有被叫到"，
    // 而这一条只问"判据对不对"。两者都在，漏一格才有两条路会红。
    for (const item of TITLEBAR_ITEMS) {
      for (const family of EVENT_FAMILIES) {
        expect(
          swallows(item.id, family),
          `${item.label} 的 ${family} 那一格判错了`,
        ).toBe(item.cells[family] === "swallow");
      }
    }
  });

  it("⚠️ 表外的 id ⇒ **不吞**（一个多出来的元素该在测试里红，而不是被静默吞掉）", () => {
    // 这个 id 是故意编的：它代表"将来被画出来、却还没写进 §4.5.1 的元素"。
    // 安全默认是**不吞** —— 宁可让那一下按"标题栏空白处"处理（用户看得见、
    // 也报得出来），也不要让一个没人审过的元素静默吃掉事件。
    for (const family of EVENT_FAMILIES) {
      expect(swallows("还没写进契约表的元素", family)).toBe(false);
    }
  });
});
