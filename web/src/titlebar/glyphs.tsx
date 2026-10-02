/**
 * 窗口控制按钮的四个图形（**自绘**，而名字与路径是这里唯一的来源）
 * ============================================================================
 *
 * ## 🔴 这个文件是"用户一眼看出来"的那个缺陷的产物
 *
 * `docs/UI设计规格.md` §4.5.2 写得很清楚：
 *
 * > `#### 4.5.2 窗口控制按钮（右侧 ─ ▢ ✕）`
 * > | 最大化时 | `▢` 图标换为"还原"双框图标；按钮位置不变 |
 *
 * 而屏幕上画出来的是 `─ ☰ ✕` —— **最大化按钮画的是汉堡菜单**，
 * 而窗口最大化之后画的是一把**叉**。
 *
 * ## 根因不是"画错了"，是**接错了**
 *
 * `TitleBar` 把 `MorphGlyph`（`morphicons` 尖刺的产物，两个图形是
 * **菜单**与**关闭**）装到了最大化那一格上 —— 它的两个图形本来要展示的是
 * "三根横线**插值**成那个叉"这个**能力**。
 * 于是规格里那个 `▢` 不是被画错，而是**从来没有被画过**。
 *
 * ## ⚠️ 而这件事一个自动检查都没看见
 *
 * | 谁 | 为什么没看见 |
 * |---|---|
 * | `MorphGlyph.test.tsx` | 它断言的是"菜单那三条路径" —— **它验的是尖刺，不是规格** |
 * | `TitleBar.test.tsx` | 20 项里**没有一条**碰图形（`grep svg` 零命中） |
 * | `tools/check-titlebar-contract.ps1` | 它只管**事件**那一张表 |
 *
 * 所以这里补上两样：**图形本身**（下面两张表），以及两个读者 ——
 * `glyphs.test.tsx`（表 → 屏幕上真的画了什么）与
 * `tools/check-titlebar-contract.ps1` 的第二条检查
 *（规格 §4.5.2 那张名字表 ↔ `WINDOW_GLYPH_NAMES`）。
 *
 * ⚠️ **一个名字写错的代价**：`WindowGlyphName` 是字面量联合，
 * 而 `TitleBar` 的 `CONTROLS` 用 `satisfies` 钉住它 ⇒ **拼错是编译错误**。
 *
 * ## 图形的口径
 *
 * - viewBox 一律 `0 0 24 24`，渲染成 **16 × 16 CSS px**。
 *   于是 `M4 12h16`（16 个用户单位）画出来是 10.67 px —— 与系统标题栏
 *   那 10 px 的图标尺度接近（§4.5.2 的"不要自创"）。
 * - 一律 `fill: none` + `stroke: currentColor`：颜色**跟着按钮的 `color` 走**，
 *   所以 hover 的 `text.primary` 与关闭按钮那两档红底白字**不用在这里写第二遍**
 *  （§4.5.2 的前三行 + `:146` 的那条规则就是它们）。
 * - 不加 `stroke-linecap`：系统的这几个图形是**平头**的
 *  （`round` 会让那根横线看起来短一截，而那是"差一点"的来源）。
 */

import type { ReactElement } from "react";

/** 四个图形名，**顺序即规格 §4.5.2 那张表的顺序**（检查脚本按顺序比）。 */
export const WINDOW_GLYPH_NAMES = ["minimize", "maximize", "restore", "close"] as const;

/** 图形的名字。 */
export type WindowGlyphName = (typeof WINDOW_GLYPH_NAMES)[number];

/**
 * 每个图形 = 一条或几条子路径。
 *
 * ⚠️ **`restore` 是"两个错位的方框"，而后面那个只画左、上两条边**
 *（`M5 15V5h10`）—— 那是 Windows 的"还原"图形。
 * 一个"画两个完整方框"的实现会让人读成**两个窗口**。
 */
export const WINDOW_GLYPH_PATHS: Readonly<Record<WindowGlyphName, readonly string[]>> = {
  /** `─` 一根横线。 */
  minimize: ["M4 12h16"],
  /** `▢` 一个方框。 */
  maximize: ["M5 5h14v14H5z"],
  /** `⧉` 两个错位的方框：前面那个完整，后面那个只留左边与上边。 */
  restore: ["M9 9h10v10H9z", "M5 15V5h10"],
  /** `✕` 两条对角线。 */
  close: ["M6 6l12 12", "M6 18L18 6"],
};

export interface WindowGlyphProps {
  readonly name: WindowGlyphName;
}

/**
 * 画一个窗口控制图形。
 *
 * ## 🔴 它是**装饰性**的，而这一条是刻意的
 *
 * `aria-hidden` + `focusable="false"`：读屏用户听到的是**按钮上的动作名**
 *（"最小化" / "最大化" / "还原" / "关闭"，见 `TitleBar`），
 * 而不是"一个图形"。
 *
 * ⚠️ 而这正是 `MorphGlyph` 那一次的教训的反面：库在**不给 `label`** 时
 * 自己加 `aria-hidden`，而它给的那个 `label` 会把 `role="img"` 塞进按钮里 ——
 * 于是按钮的可访问名变成"图形名"，而不是动作名。
 */
export function WindowGlyph({ name }: WindowGlyphProps): ReactElement {
  return (
    <svg
      className="titlebar__glyph"
      viewBox="0 0 24 24"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      aria-hidden="true"
      focusable="false"
    >
      {WINDOW_GLYPH_PATHS[name].map((d) => (
        <path key={d} d={d} />
      ))}
    </svg>
  );
}
