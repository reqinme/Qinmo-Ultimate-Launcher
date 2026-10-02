/**
 * 横幅"画面"的配色（**M4 主干** · `docs/UI设计规格.md` §4.6.1）
 * ============================================================================
 *
 * ## 规格原文（这一条是**红线级别**的）
 *
 * > **横幅与卡片里的封面图，一律自绘或用户自有。**
 * > **禁止使用 Mojang / Microsoft 的官方素材、十六进制素材包、商店截图。**
 *
 * | 允许 | 做法 |
 * |---|---|
 * | **纯抽象自绘** | 几何图形 + 渐变，**每个实例一个配色**（由实例 id 稳定生成，**不随机**） |
 * | **用户自有** | 玩家自己的游戏截图 |
 * | **中性占位** | 没有图时用**中性渐变** + 明确标注"占位" |
 *
 * > **"按实例 id 稳定生成配色"很要紧**：**同一个实例每次看到的颜色必须一样**——
 * > 随机配色会让用户无法通过颜色认实例。这也让它**可测试**（同输入同输出）。
 *
 * ## 🔴 所以这个模块只做一件事：把 id 变成**一个档号**
 *
 * ```text
 *   "26.3" ──coverTone──▶ 7   ──CSS──▶ --cover-step: 7 ──▶ hue-rotate(210deg)
 * ```
 *
 * **颜色本身一个字节都不在这里** —— 它在 `HomePage.css` 里由
 * `data-cover-tone` 选一档，而那几档用的全是令牌（`--accent-*` / `--gray-*`）。
 * 一个在这里返回 `hsl(...)` 的实现会同时违反两条纪律：
 * *"界面里禁止字面颜色函数"*（`eslint.config.js` 的 `no-restricted-syntax`）
 * 与 *"零字面颜色值"*（`web/src/tokens.css` 的令牌层）。
 *
 * ## 为什么是"档"而不是 0–359 的度数
 *
 * 度数会让"同一档"这件事在 CSS 里无法被**数出来**（12 条规则 vs 一条 `calc`）。
 * 而 12 档已经够用：**相邻两档的色相差 30°**，人眼能分辨，
 * 而 12 档也小到能把每一条都写死在样式表里、能被 `check-css-classes` 之类看见。
 */

/**
 * 画面上限档数（`HomePage.css` 里必须有 **这么多条** `[data-cover-tone="N"]` 规则）。
 *
 * ⚠️ 它同时是**相邻色相差**的分母：每档 `30°`。
 */
export const COVER_TONES = 12;

/** 没有实例时的档号 —— §4.6.1 的"中性占位"那一档（配"封面图为占位"的标注）。 */
export const COVER_TONE_NONE = "none";

/**
 * 由 **实例 id** 稳定算出配色档号（`0 .. COVER_TONES - 1`）。
 *
 * ⚠️ **不要换成 `Math.random()` 或 `Date.now()`**：那会让同一个实例
 * 每次打开都是另一种颜色，而规格说那正是不能做的事。
 *
 * 用的是 FNV-1a（32 位）—— 选它是因为：**逐字符、无状态、纯函数**，
 * 于是"同输入同输出"是**结构上**成立的，而不是靠"记得别加随机"。
 */
export function coverTone(id: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < id.length; i += 1) {
    hash ^= id.charCodeAt(i);
    // ⚠️ `Math.imul` 而不是 `*` —— 32 位乘法在 JS 里会超出 2^53，
    // 而超出之后低位会被**丢掉**，于是不同的 id 撞到同一档。
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash % COVER_TONES;
}

/**
 * 档号 → `data-cover-tone` 的值。
 *
 * ⚠️ **它返回字符串**，因为那个属性是给 CSS 看的（`[data-cover-tone="7"]`）。
 * 一个把 `number` 直接交给 JSX 的实现也能跑（React 会转成字符串），
 * 但那让"没有实例"这一档（`"none"`）**在类型上无处安放**。
 */
export function coverToneAttr(coverId: string | undefined): string {
  return coverId === undefined ? COVER_TONE_NONE : String(coverTone(coverId));
}
