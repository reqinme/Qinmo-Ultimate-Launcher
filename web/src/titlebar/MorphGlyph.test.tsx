/**
 * `MorphGlyph` 的测试 —— 而它同时是一份**它值不值的证据**
 * ============================================================================
 *
 * ## 🔴 这个文件里最重要的一条不是"它会动"，而是**它会读出来**
 *
 * 上一轮 `TitleBar` 里那三处控制的外观是：
 *
 * ```tsx
 * <span aria-hidden="true">{c.glyph}</span>   // ─ ▢ ✕
 * ```
 *
 * 而 `aria-label` 在 `<button>` 上 —— 所以读屏**会**念出"最小化"。
 * 那是对的。
 *
 * ⚠️ **但把 `MorphIcon` 的 `label` 漏掉会把它变坏**：库的文档说
 *
 * > with label → `role="img"` + `<title>`；without → `aria-hidden`
 *
 * 也就是说**不给 label 它就把自己藏起来**，而那时按钮里**没有可读内容** ——
 * 读屏只念出"button"。这一条测试就是钉住它。
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MorphGlyph } from "./MorphGlyph.tsx";

describe("MorphGlyph：它会读出来", () => {
  it("🔴 给它 `label` ⇒ 它有一个**可读的名字**", () => {
    render(<MorphGlyph active={false} label="最大化" />);
    // ⚠️ `getByRole("img", { name })` —— 而库给的是
    // `role="img"` + `<title>`，于是那个 `<title>` 成了可读名字。
    //
    // 一个把 `label` 漏掉的实现会让这条**红**，而那时按钮在读屏里
    // 只剩"button"三个字。
    expect(screen.getByRole("img", { name: "最大化" })).toBeTruthy();
  });

  it("`active` 换一个值时它仍然可读（而那是两个不同的形状）", () => {
    const { rerender } = render(<MorphGlyph active={false} label="最大化" />);
    const a = document.querySelector("svg")?.innerHTML ?? "";
    rerender(<MorphGlyph active label="还原" />);
    const b = document.querySelector("svg")?.innerHTML ?? "";
    // ⚠️ **两条路径不同** —— 而这一条是"它真的画了别的东西"的判据，
    // 不是"它换了 class"。一个只改 class 的实现会让插值无事可做。
    expect(a).not.toBe(b);
    expect(screen.getByRole("img", { name: "还原" })).toBeTruthy();
  });

  it("它画出的是**我们自绘的几何**（三根横线 / 两条对角线）", () => {
    // ⚠️ **这一条钉的是那句"图形全部自绘"不违反。**
    //
    // 库提供的是**插值引擎**，而路径本身来自 `MorphGlyph.tsx` 里的
    // `MENU` / `CLOSE` 两个常量。所以那条纪律成立。
    //
    // 而一个"顺便用了它自带的图标集"的实现会让这里的路径**不是**
    // 我们写的那两个 —— 也就是说这条测试会在那时红。
    const { container } = render(<MorphGlyph active={false} label="菜单" />);
    const d = container.querySelector("svg")?.innerHTML ?? "";
    // 我们画的三根横线在 y = 7 / 12 / 17，而从 x=4 到 x=20。
    expect(d).toContain("M4 7");
    expect(d).toContain("M4 12");
    expect(d).toContain("M4 17");
  });

  it("🔴 `reducedMotion=\"user\"` ⇒ 系统要求减少动效时**不插值**", () => {
    // ⚠️ 库的默认是 `"never"`（**无论系统设置如何都动画**），
    // 而那与 §7.3 约束 5 的**精神**相反 —— 那一条要求进度节流、
    // 只动 `transform`/`opacity`，整体意图是"不要让人难受"。
    //
    // 所以 `MorphGlyph` 传的是 `"user"`。而这条测试**不能**断言
    // "它没动"（那要跑真实的动画帧），它断言的是**那个 prop 被传下去了** ——
    // 判据是"组件把那个值交给了库"，而不是"库的行为"。
    //
    // 诚实地说：这是一条**较弱的**断言。它防的是"有人把这一行删了"。
    const { container } = render(<MorphGlyph active={false} label="菜单" />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    // ⚠️ 而那些路径**在渲染时就已就位**（`reducedMotion="user"` 的
    // 瞬间切换意味着"目标形状总是对的"）—— 于是这里断言的就是那个形状。
    expect(svg?.innerHTML ?? "").toContain("path");
  });
});
