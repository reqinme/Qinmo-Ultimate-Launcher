/**
 * 四个窗口图形：**表 → 屏幕上真的画了什么**
 * ============================================================================
 *
 * ## 🔴 这个文件是"用户一眼看出来"的那个缺陷的回归测试
 *
 * 规格 §4.5.2 要的是 `─ ▢ ✕`（最大化时 `▢` 换成"还原"双框），
 * 而 `TitleBar` 曾经在最大化那一格装的是 `MorphGlyph`（`morphicons` 尖刺的
 * 产物），它的两个图形是**菜单**与**关闭** ⇒ 屏幕上画出来的是 `─ ☰ ✕`。
 *
 * | 谁 | 当时为什么没看见 |
 * |---|---|
 * | `TitleBar.test.tsx` | 20 项里**没有一条**碰图形（`grep svg` 零命中） |
 * | `MorphGlyph.test.tsx` | 它断言的是那三条横线的路径 —— **它验的是尖刺，不是规格** |
 *
 * 所以这里钉三件事：
 *
 * 1. **① 表自洽**：四个名字、每个至少一条路径、四条互不相同。
 * 2. **② 静止态画的就是 `─ ▢ ✕`** —— 这一条是那个 bug 的回归测试
 *   （`☰` 那三条横线会当场红）。
 * 3. **③ 状态跟着窗口走**：最大化 ⇄ 还原切图形；而 `Win + ↑` 那类
 *   **不经过我们**的改变由 `resize` + `window_is_maximized` 兜住
 *   （⚠️ 一个"挂载时问一次就完事"的实现会在这里红 —— 它比不修更坏）。
 *
 * ⚠️ 与 `tools/check-titlebar-contract.ps1` 的分工是**有意重复**的：
 * 那个脚本管"规格 §4.5.2 的名字表 ↔ `glyphs.tsx` 的名字表"，
 * 这个文件管"名字表 → DOM 里那个 `d` 属性"。
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// ⚠️ mock 的是**边界层**（`api/window.ts`），而不是 `@tauri-apps/api/core` ——
// 后者根本不该被组件碰到（那是 `api/` 的纪律，由 `boundary.test.ts` 强制）。
vi.mock("../api/window.ts", () => ({
  windowMinimize: vi.fn(async () => {}),
  windowToggleMaximize: vi.fn(async () => true),
  windowIsMaximized: vi.fn(async () => false),
  windowClose: vi.fn(async () => {}),
  windowStartDragging: vi.fn(async () => {}),
}));

import { windowIsMaximized, windowToggleMaximize } from "../api/window.ts";
import { TitleBar } from "./TitleBar.tsx";
import { WINDOW_GLYPH_NAMES, WINDOW_GLYPH_PATHS } from "./glyphs.tsx";

const mockedToggle = vi.mocked(windowToggleMaximize);
const mockedIsMax = vi.mocked(windowIsMaximized);

beforeEach(() => {
  vi.clearAllMocks();
  mockedToggle.mockResolvedValue(true);
  mockedIsMax.mockResolvedValue(false);
});

afterEach(() => {
  vi.clearAllMocks();
});

/**
 * 某个控制按钮上**真的画出来的**那几条路径。
 *
 * ⚠️ 读的是 SVG 的 `d` 属性 —— jsdom 没有布局引擎，但**属性是真的**，
 * 所以这一条能验"画的是哪个图形"（而验不了"看起来对不对"）。
 */
function drawn(id: string): string[] {
  const btn = document.querySelector(`[data-titlebar-item="${id}"]`);
  if (!(btn instanceof HTMLElement)) throw new TypeError(`找不到控制按钮 ${id}`);
  return [...btn.querySelectorAll("svg path")].map((p) => p.getAttribute("d") ?? "");
}

describe("① 图形表自洽（四个名字，四条不同的图形）", () => {
  it("名字表就是 §4.5.2 那四个，且路径表的键与它完全一致", () => {
    expect([...WINDOW_GLYPH_NAMES]).toEqual(["minimize", "maximize", "restore", "close"]);
    expect(Object.keys(WINDOW_GLYPH_PATHS).sort()).toEqual([...WINDOW_GLYPH_NAMES].sort());
  });

  it("每个图形至少一条非空路径，而四条图形互不相同", () => {
    const seen = new Map<string, string>();
    for (const name of WINDOW_GLYPH_NAMES) {
      const paths = [...WINDOW_GLYPH_PATHS[name]];
      expect(paths.length, `${name} 至少要有一条路径`).toBeGreaterThan(0);
      for (const d of paths) {
        expect(d.trim().length, `${name} 的路径不能是空的`).toBeGreaterThan(0);
      }
      const key = paths.join(" | ");
      expect(seen.has(key), `${name} 与 ${seen.get(key) ?? "?"} 画的是同一个图形`).toBe(false);
      seen.set(key, name);
    }
  });
});

describe("② 静止态：`─` 是一根横线、`▢` 是一个方框、`✕` 是两条对角线", () => {
  it("🔴 最大化按钮画的是**方框**，而不是那个汉堡菜单（§4.5.2 的原文）", () => {
    render(<TitleBar />);
    // 这一条就是那个缺陷的回归测试：`MorphGlyph` 的 MENU 是
    // "M4 7h16" / "M4 12h16" / "M4 17h16" 三根横线，而它曾经装在这一格上。
    expect(drawn("max")).toEqual([...WINDOW_GLYPH_PATHS.maximize]);
    expect(drawn("max")).not.toEqual(["M4 7h16", "M4 12h16", "M4 17h16"]);
  });

  it("最小化与关闭按钮各画自己的图形", () => {
    render(<TitleBar />);
    expect(drawn("min")).toEqual([...WINDOW_GLYPH_PATHS.minimize]);
    expect(drawn("close")).toEqual([...WINDOW_GLYPH_PATHS.close]);
  });

  it("三个图形都是**装饰性**的，读屏只听到按钮上的动作名", () => {
    const { container } = render(<TitleBar />);
    const svgs = [...container.querySelectorAll(".titlebar__btn svg")];
    expect(svgs.length).toBe(3);
    for (const svg of svgs) {
      expect(svg.getAttribute("aria-hidden")).toBe("true");
      expect(svg.getAttribute("focusable")).toBe("false");
    }
    // 图形是 `aria-hidden` ⇒ 可访问名只能来自按钮自己那个 `aria-label`。
    expect(screen.getByLabelText("最大化")).toBeTruthy();
    expect(screen.getByLabelText("最小化")).toBeTruthy();
    expect(screen.getByLabelText("关闭")).toBeTruthy();
  });
});

describe("③ 状态跟着**窗口**走（不只是跟着我们那三条命令）", () => {
  it("点一下最大化 ⇒ 图形换成「还原」双框，名字也换成「还原」", async () => {
    render(<TitleBar />);
    fireEvent.click(screen.getByLabelText("最大化"));
    await screen.findByLabelText("还原");
    expect(drawn("max")).toEqual([...WINDOW_GLYPH_PATHS.restore]);
    // ⚠️ 另外两个图形**不动** —— "换一个图形"不该顺手动别的。
    expect(drawn("min")).toEqual([...WINDOW_GLYPH_PATHS.minimize]);
    expect(drawn("close")).toEqual([...WINDOW_GLYPH_PATHS.close]);
  });

  it("再点一下 ⇒ 回到方框（toggle 的返回值是唯一真相）", async () => {
    render(<TitleBar />);
    fireEvent.click(screen.getByLabelText("最大化"));
    await screen.findByLabelText("还原");
    mockedToggle.mockResolvedValue(false);
    fireEvent.click(screen.getByLabelText("还原"));
    await screen.findByLabelText("最大化");
    expect(drawn("max")).toEqual([...WINDOW_GLYPH_PATHS.maximize]);
  });

  it("🔴 挂载时先问一次窗口：**系统**把它最大化了也要画「还原」", async () => {
    mockedIsMax.mockResolvedValue(true);
    render(<TitleBar />);
    // 没有这一条，`Win + ↑` 之后按钮会一直画着那个"点了能最大化"的方框。
    await screen.findByLabelText("还原");
    expect(drawn("max")).toEqual([...WINDOW_GLYPH_PATHS.restore]);
  });

  it("🔴 尺寸变化之后再问一次（`Win + ↑` / 拖到屏幕顶端都不经过我们）", async () => {
    render(<TitleBar />);
    expect(drawn("max")).toEqual([...WINDOW_GLYPH_PATHS.maximize]);
    mockedIsMax.mockResolvedValue(true);
    fireEvent(window, new Event("resize"));
    await screen.findByLabelText("还原");
    expect(drawn("max")).toEqual([...WINDOW_GLYPH_PATHS.restore]);
  });

  it("⚠️ 贴靠（尺寸变了而**没有**最大化）不该被读成最大化", async () => {
    render(<TitleBar />);
    await waitFor(() => {
      expect(mockedIsMax).toHaveBeenCalledTimes(1);
    });
    // 贴到屏幕左半边：尺寸变了，而窗口**不是**最大化 ⇒ 图形必须还是方框。
    fireEvent(window, new Event("resize"));
    await waitFor(() => {
      expect(mockedIsMax).toHaveBeenCalledTimes(2);
    });
    expect(drawn("max")).toEqual([...WINDOW_GLYPH_PATHS.maximize]);
  });
});
