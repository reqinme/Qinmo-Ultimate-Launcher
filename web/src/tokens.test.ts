/**
 * 设计令牌的验收测试（**M4 门禁第 ① 项**）
 *
 * ============================================================================
 * 它测的三件事，以及为什么每一件都必须测
 * ============================================================================
 *
 * ## ① 规格点名的令牌**一个都不许少**
 *
 * `docs/UI设计规格.md` §5.1 有一段原文：
 *
 * > **⚠️ 此前的问题**：只定义了 `surface.panel/card/raised` + `stroke` + `text`
 * > + `accent`，而 §6.3 的门禁却要求"每个组件含 6 种状态"——**没有
 * > hover/active/selected/disabled 这些令牌，那 20 个组件根本无法落地**。
 * > 实测缺口：`surface.hover` / `surface.active` / `surface.selected` /
 * > `surface.disabled` / `text.on-accent` / `focus.ring` / `overlay.scrim` /
 * > `easing` / `cubic-bezier` —— **全是 0 处**。本节补齐。
 *
 * 所以本测试把**那一份清单**变成断言。理由是"缺令牌"这件事的症状
 * **不在令牌文件里**，而在**四天之后**某个组件作者发现"没有 selected 态可用"
 * 而随手写了一个 `rgba(...)` —— 那是 lint 拦不住的（他会定义一个新变量）。
 *
 * ## ② `text.on-accent` 必须**按公式算出来的**
 *
 * §5.3 原文："默认：`accent.5` 的相对亮度 > 0.45 → 用深色文字；否则用浅色文字。
 * **实现时按公式算，不写死**"。
 *
 * 而令牌文件里**必然是一个写死的值**（运行时算的是 M4 主干的事）。
 * 所以本测试**独立实现 WCAG 的相对亮度公式**，拿它复算那个值 ——
 * 于是"将来换了强调色却忘了改 `text.on-accent`"会被测出来，
 * **而那正是规格担心的情形**（对比度是可访问性的硬线）。
 *
 * ## ③ 两处浅色取值必须**一致**
 *
 * `tokens.css` 里浅色写了两次（`@media (prefers-color-scheme: light)` 与
 * `[data-theme="light"]`），因为 CSS 没有"媒体查询 **或** 属性选择器"。
 * 重复是刻意的，而**它的风险是"改了一处忘了另一处"** —— 所以本测试比对它们。
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";

const HERE = dirname(fileURLToPath(import.meta.url));
const CSS = readFileSync(join(HERE, "tokens.css"), "utf8");

/** 取出某个选择器块里的自定义属性（**按大括号配对**，不是正则到下一个 `}`）。 */
function blockOf(selector: string): string {
  const i = CSS.indexOf(selector);
  if (i < 0) {
    throw new Error(`tokens.css 里找不到选择器：${selector}`);
  }
  const open = CSS.indexOf("{", i);
  let depth = 0;
  for (let j = open; j < CSS.length; j += 1) {
    if (CSS[j] === "{") depth += 1;
    else if (CSS[j] === "}") {
      depth -= 1;
      if (depth === 0) return CSS.slice(open + 1, j);
    }
  }
  throw new Error(`选择器 ${selector} 的大括号没闭合`);
}

/** 一块里定义的全部 `--x` 名字。 */
function namesIn(block: string): Set<string> {
  const out = new Set<string>();
  for (const m of block.matchAll(/(--[a-z0-9-]+)\s*:/g)) {
    // ⚠️ 正则捕获组在 `noUncheckedIndexedAccess` 下是 `string | undefined`。
    // 而"捕获组没匹配上"在这里**不可能**（整个 match 就是以它为基础的）——
    // 所以判空是给类型系统的，不是给运行时的。
    const name = m[1];
    if (name !== undefined) out.add(name);
  }
  return out;
}

/** 全文件里出现过的全部 `--x` 名字（**定义或引用**）。 */
function allNames(): Set<string> {
  const out = new Set<string>();
  for (const m of CSS.matchAll(/(--[a-z0-9-]+)/g)) {
    const name = m[1];
    if (name !== undefined) out.add(name);
  }
  return out;
}

// ============================================================================
// ① 规格点名的令牌
// ============================================================================

describe("规格 §5.1 点名的令牌（一个都不许少）", () => {
  const root = blockOf(":root {");
  const names = namesIn(root);
  const everywhere = allNames();

  /**
   * §5.1「实测缺口」那一段点名的东西。
   *
   * ⚠️ **这是规格自己列的清单**，不是我从实现里反推的 ——
   * 一个"从实现里反推的清单"会在实现缺东西时**一起缺**，从而永远通过。
   */
  const SPEC_NAMED = [
    "--surface-hover",
    "--surface-active",
    "--surface-selected",
    "--surface-disabled",
    "--text-on-accent",
    "--focus-ring",
    "--overlay-scrim",
    "--easing-standard",
    "--easing-enter",
    "--easing-exit",
    "--divider",
    "--text-link",
  ];

  it.each(SPEC_NAMED)("%s 存在", (name) => {
    expect(everywhere.has(name), `缺少 ${name}`).toBe(true);
  });

  it("三层结构都齐：调色板 / 语义 / 表现", () => {
    // 第一层
    for (const g of [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]) {
      expect(names.has(`--gray-${g}`), `缺 --gray-${g}`).toBe(true);
    }
    // 第二层里"组件只准用这一层"的几个族
    for (const t of [
      "--surface-panel",
      "--surface-card",
      "--surface-raised",
      "--stroke-subtle",
      "--stroke-strong",
      "--text-primary",
      "--text-secondary",
      "--text-disabled",
      "--accent",
    ]) {
      expect(names.has(t), `缺 ${t}`).toBe(true);
    }
    // 第三层（§5.4.4）
    for (const t of [
      "--intensity-edge-gradient",
      "--intensity-edge-gradient-hover",
      "--intensity-motion-countup",
      "--intensity-motion-progress-ease",
    ]) {
      expect(everywhere.has(t), `缺表现层令牌 ${t}`).toBe(true);
    }
  });

  it("状态背景**成对**带配套前景（§5.1：不能只取背景）", () => {
    // §5.1 原文："`state.*.bg` 必须各自标注配套文字色，
    // 不能'背景用了就用默认文字'"
    for (const s of ["success", "warning", "danger", "info"]) {
      expect(names.has(`--state-${s}-bg`), `缺 --state-${s}-bg`).toBe(true);
      expect(names.has(`--state-${s}-fg`), `缺 --state-${s}-fg`).toBe(true);
    }
  });

  it("灰阶是 10 档且**数字越大对比越强**（§5.1 的命名修正）", () => {
    // 无法在静态文本里判"对比度"，但**可以判"深浅两端的位置是对的"**：
    // 深色主题下 gray-9 必须比 gray-0 亮（它是"对比最强"的那端）。
    // ⚠️ **不用 `const [r,g,b] = arr.map(...)`** —— 在 `noUncheckedIndexedAccess`
    // 之下每个元素都是 `T | undefined`，于是下面三个乘法过不了 tsc。
    // 拆成三个具名常量让"取到了几个"这件事显式。
    const chan = (v: string, i: number): number =>
      parseInt(v.slice(i, i + 2), 16) / 255;
    const lum = (hex: string): number => {
      const v = hex.replace("#", "");
      return 0.2126 * chan(v, 0) + 0.7152 * chan(v, 2) + 0.0722 * chan(v, 4);
    };
    const g0 = /--gray-0:\s*(#[0-9a-f]{6})/i.exec(root)?.[1];
    const g9 = /--gray-9:\s*(#[0-9a-f]{6})/i.exec(root)?.[1];
    expect(g0).toBeTruthy();
    expect(g9).toBeTruthy();
    expect(
      lum(g9 as string),
      `深色下 gray-9(${g9}) 应当比 gray-0(${g0}) 亮 —— 否则"数字越大对比越强"不成立`,
    ).toBeGreaterThan(lum(g0 as string));
  });
});

// ============================================================================
// ② text.on-accent 按 WCAG 公式复算
// ============================================================================

describe("`--text-on-accent` 与强调色的对比度（§5.3 硬线）", () => {
  /** WCAG 2.x 的相对亮度。**独立实现**，不从 CSS 里抄。 */
  function relLuminance(hex: string): number {
    const v = hex.replace("#", "");
    const chan = (i: number): number => {
      const c = parseInt(v.slice(i, i + 2), 16) / 255;
      return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * chan(0) + 0.7152 * chan(2) + 0.0722 * chan(4);
  }

  /** WCAG 对比度。 */
  function contrast(a: string, b: string): number {
    // ⚠️ 同样不用解构（`noUncheckedIndexedAccess`）。而这里的语义是
    // "大的除以小的"，所以直接算两个再比 —— 比排序后解构更直白。
    const la = relLuminance(a);
    const lb = relLuminance(b);
    const hi = Math.max(la, lb);
    const lo = Math.min(la, lb);
    return (hi + 0.05) / (lo + 0.05);
  }

  /** 取某个主题块里某个令牌的字面色值。 */
  function hexOf(blockSelector: string, token: string): string | undefined {
    const b = blockOf(blockSelector);
    const re = new RegExp(`${token}:\\s*(#[0-9a-f]{6})`, "i");
    return re.exec(b)?.[1];
  }

  it("🔴 规格 §5.3 的公式在 `#4C9AFF` 上**落在错误的一侧**（实测证据）", () => {
    // §5.3 原文的判据："accent.5 的相对亮度 > 0.45 → 用深色文字；否则用浅色文字"
    const accent = hexOf(":root {", "--accent-5") as string;
    expect(accent).toBeTruthy();
    const l = relLuminance(accent);
    // ① 公式会选"浅色文字"
    expect(l, `accent-5=${accent} 的亮度 ${l.toFixed(4)} 应当 ≤ 0.45`).toBeLessThanOrEqual(0.45);

    // ② 而**公式选的那一侧不过硬线** —— 这是这条测试存在的理由
    const white = "#ffffff";
    const formulaChoiceContrast = contrast(accent, white);
    expect(
      formulaChoiceContrast,
      `公式说配浅色文字，而实测只有 ${formulaChoiceContrast.toFixed(2)}:1 —— ` +
        `连大字号的 3:1 都不到。所以这条公式在 #4C9AFF 上是**错的**。`,
    ).toBeLessThan(3);

    // ③ 深色那一侧过
    const dark = "#1a1a1c";
    expect(contrast(accent, dark)).toBeGreaterThanOrEqual(4.5);

    // ④ 所以令牌文件按**实测赢了的那一侧**落值
    const onAccent = hexOf(":root {", "--text-on-accent") as string;
    expect(
      relLuminance(onAccent),
      "--text-on-accent 必须取深色（规格公式推荐的那个浅色只有 2.85:1）",
    ).toBeLessThan(0.5);
  });

  it("深色：强调色之上的文字**对比度 ≥ 4.5:1**（正文硬线）", () => {
    const accent = hexOf(":root {", "--accent-5") as string;
    const onAccent = hexOf(":root {", "--text-on-accent") as string;
    const c = contrast(accent, onAccent);
    expect(c, `#4C9AFF 上的文字对比度 ${c.toFixed(2)}:1 必须 ≥ 4.5:1`).toBeGreaterThanOrEqual(
      4.5,
    );
  });

  it("浅色：强调色之上的文字**也满足 ≥ 4.5:1**", () => {
    const accent = hexOf('[data-theme="light"] {', "--accent-5") as string;
    const onAccent = hexOf('[data-theme="light"] {', "--text-on-accent") as string;
    expect(accent).toBeTruthy();
    expect(onAccent).toBeTruthy();
    const c = contrast(accent, onAccent);
    expect(c, `#0A66C2 上的文字对比度 ${c.toFixed(2)}:1 必须 ≥ 4.5:1`).toBeGreaterThanOrEqual(
      4.5,
    );
  });
});

// ============================================================================
// ③ 两处浅色取值一致（重复是刻意的，而"忘了改另一处"必须被拦）
// ============================================================================

describe("浅色主题只写了一次吗", () => {
  /** 从一块里取出全部 `--name: value`（值只留一层括号内的内容）。 */
  function decls(block: string): Map<string, string> {
    const out = new Map<string, string>();
    for (const m of block.matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)) {
      const name = m[1];
      const value = m[2];
      if (name !== undefined && value !== undefined) {
        out.set(name, value.trim().replace(/\s+/g, " "));
      }
    }
    return out;
  }

  it("`data-theme=light` 与系统浅色**在调色板与色相方向上一致**", () => {
    const media = decls(blockOf("@media (prefers-color-scheme: light)"));
    const attr = decls(blockOf('[data-theme="light"] {'));

    // ⚠️ 只比对**两处都定义了**的那些键。
    // 一个"要求两处键集合完全相同"的断言会拦掉合理的差异
    //（例如 `@media` 那支不需要写 `color-scheme`）。
    const shared = [...media.keys()].filter((k) => attr.has(k));
    expect(shared.length, "两处共享的键太少 —— 那说明它们已经各自漂移了").toBeGreaterThan(20);

    const mismatched = shared.filter((k) => media.get(k) !== attr.get(k));
    expect(
      mismatched,
      `这些键在两处浅色定义里取值不同：${mismatched.join(", ")} —— ` +
        `改了系统那一支却忘了 data-theme 那一支，会导致"显式选浅色"与` +
        `"跟随系统变浅色"看起来不一样`,
    ).toEqual([]);
  });
});

// ============================================================================
// ④ 四层视觉偏好的**书写顺序**（纪律 ① 的机器版）
// ============================================================================

describe("视觉偏好的优先级（§5.4.1 纪律 ①）", () => {
  it("系统能力媒体查询写在用户可调档**之后**（同优先级下后写的赢）", () => {
    const reducedMotion = CSS.indexOf("@media (prefers-reduced-motion: reduce)");
    const contrast = CSS.indexOf("@media (prefers-contrast: more)");
    const enhanced = CSS.indexOf('[data-intensity="enhanced"]');
    const materialNone = CSS.indexOf('[data-material="none"]');

    expect(enhanced, "找不到 data-intensity=enhanced").toBeGreaterThan(-1);
    expect(materialNone, "找不到 data-material=none").toBeGreaterThan(-1);
    expect(reducedMotion, "找不到 prefers-reduced-motion").toBeGreaterThan(-1);
    expect(contrast, "找不到 prefers-contrast").toBeGreaterThan(-1);

    // **纪律 ①：第 1 层（系统）必须压在用户档之上。**
    // 若顺序反过来，一个开了 Enhanced 的用户在高对比度系统上
    // 会拿到被收紧的表现层 —— **而那正是规格要求的**（收紧，不是放开）。
    expect(
      reducedMotion > enhanced && reducedMotion > materialNone,
      "`prefers-reduced-motion` 必须写在 data-intensity / data-material 之后",
    ).toBe(true);
    expect(
      contrast > enhanced && contrast > materialNone,
      "`prefers-contrast` 必须写在 data-intensity / data-material 之后",
    ).toBe(true);
  });

  it("高对比度下表现层被**收紧**（纪律 ①：不生效且不报错）", () => {
    const b = blockOf("@media (prefers-contrast: more)");
    expect(b, "高对比度那一段应当把 intensity-edge-gradient 归零").toMatch(
      /--intensity-edge-gradient:\s*none/,
    );
    expect(b).toMatch(/--intensity-motion-countup:\s*0ms/);
  });

  it("减少动态**不**取消进度条的缓动（§5.4.3：它是信息性）", () => {
    const b = blockOf("@media (prefers-reduced-motion: reduce)");
    // ⚠️ 这条是"信息性 vs 装饰性"判据的机器版：
    // 进度条缓动让"快到了"与"卡住了"可区分 ⇒ **信息性** ⇒ 永远存在。
    expect(
      b.includes("--intensity-motion-progress-ease"),
      "`prefers-reduced-motion` 不应取消 --intensity-motion-progress-ease —— " +
        "进度条的缓动是信息性的（§5.4.3），去掉它用户就无法判断" +
        "「快到了」与「卡住了」",
    ).toBe(false);
  });
});
