/**
 * 横幅画面的配色测试（`docs/UI设计规格.md` §4.6.1）
 * ============================================================================
 *
 * ## 这一条规格的全部内容是**"稳定"**
 *
 * > **同一个实例每次看到的颜色必须一样** —— 随机配色会让用户无法通过颜色认实例。
 * > 这也让它**可测试**（同输入同输出）。
 *
 * 所以本文件测的**不是**"颜色好不好看"（那验不了），而是四件可验的事：
 *
 * | # | 断言 | 它挡住的错法 |
 * |---|---|---|
 * | 1 | 同输入同输出 | `Math.random()` / `Date.now()` |
 * | 2 | 档号落在 `0 .. COVER_TONES-1` 且是整数 | 忘了取模、或 `*` 溢出后给出小数 |
 * | 3 | **晚一位**的字符也能改变档号 | `Math.imul` 被写成 `*`（低位被丢掉） |
 * | 4 | **CSS 里真的有那么多条规则** | TS 加了档而样式表没跟上（或反过来） |
 */

import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { COVER_TONE_NONE, COVER_TONES, coverTone, coverToneAttr } from "./cover.ts";

/**
 * 读样式表 —— **而"怎么读"在这里踩过两个坑，两次都是静默的**。
 *
 * | 试过的写法 | 症状 |
 * |---|---|
 * | `readFileSync(new URL("./HomePage.css", import.meta.url))` | **整份文件在收集期炸**：`TypeError: The URL must be of scheme file`（vite-node 之下 `import.meta.url` 不是 `file:`）—— 一份**一条都没跑**的测试比一条失败的测试更坏，因为它看起来还在 |
 * | `import css from "./HomePage.css?raw"` | 编译过了、跑起来了，而字符串是 **`''`** —— 于是"12 档齐不齐""有没有字面色值"**全部静默通过**（`''` 里当然没有色值） |
 *
 * 所以现在是**显式候选路径 + 非空断言**：读不到就抛，读到空也算读不到。
 * 下面那条"样式表读到了"的测试就是为第二个坑留的。
 */
function readHomeCss(): string {
  const candidates = [
    // 从仓库根跑（`pnpm test` 的 cwd 可能是根）
    "web/src/home/HomePage.css",
    // 从 web/ 跑（vitest 的 root 是 web）
    "src/home/HomePage.css",
  ];
  for (const rel of candidates) {
    try {
      const text = readFileSync(resolve(process.cwd(), rel), "utf8");
      if (text.length > 0) return text;
    } catch {
      // 换下一个候选；两个都读不到时下面那行会带着 cwd 抛出来。
    }
  }
  throw new Error(`读不到 HomePage.css（cwd = ${process.cwd()}）`);
}

const css = readHomeCss();

/** 一批像实例 id 的样本。 */
const IDS = [
  "26.3",
  "1.21.120",
  "26.3-fabric",
  "26.3-forge",
  "instance-1",
  "instance-2",
  "某个中文实例名",
  "a",
  "",
];

describe("§4.6.1：按实例 id **稳定**生成配色", () => {
  it("🔴 同一个 id 每次都是同一档（同输入同输出）", () => {
    for (const id of IDS) {
      expect(coverTone(id), id).toBe(coverTone(id));
    }
    // 而"跨调用稳定"要能挡住 `Math.random()`：一个随机实现
    // 在 200 次里撞上同一档的概率是 (1/12)^199 —— 也就是零。
    const first = coverTone("26.3-fabric");
    for (let i = 0; i < 200; i += 1) {
      expect(coverTone("26.3-fabric")).toBe(first);
    }
  });

  it("档号是 `0 .. COVER_TONES-1` 的**整数**", () => {
    for (const id of IDS) {
      const t = coverTone(id);
      expect(Number.isInteger(t), `${id} → ${t}`).toBe(true);
      expect(t).toBeGreaterThanOrEqual(0);
      expect(t).toBeLessThan(COVER_TONES);
    }
  });

  it("⚠️ **晚一位**的字符也改变档号（`Math.imul` 的落点）", () => {
    // 一个把 `Math.imul(h, 0x01000193)` 写成 `h * 0x01000193` 的实现
    // 会让乘积超出 2^53，于是**低位被丢掉** —— 而低位正是最后吃进去的那几个字符。
    // 症状：`…-a` 与 `…-b` 撞到同一档，而"长得像的 id 颜色也一样"。
    const a = coverTone("instance-26.3-fabric-a");
    const b = coverTone("instance-26.3-fabric-b");
    expect(a).not.toBe(b);
  });

  it("一批不同的 id 会散开（不是全挤在同一档）", () => {
    // ⚠️ 12 档必然有碰撞，所以这里断言的是**散开**而不是"两两不同"。
    const tones = new Set(Array.from({ length: 120 }, (_, i) => coverTone(`instance-${i}`)));
    expect(tones.size).toBeGreaterThanOrEqual(8);
  });

  it("12 档**都够得着**（没有永远选不到的那一档）", () => {
    const tones = new Set(Array.from({ length: 400 }, (_, i) => coverTone(`i${i}`)));
    expect(tones.size).toBe(COVER_TONES);
  });
});

describe("`data-cover-tone` 的值（含「没有实例」那一档）", () => {
  it("有实例 ⇒ 档号的字符串", () => {
    expect(coverToneAttr("26.3")).toBe(String(coverTone("26.3")));
  });

  it("🔴 没有实例 ⇒ `none`（§4.6.1 的「中性占位」）", () => {
    // ⚠️ 一个"没有 id 就随便给一档"的实现会让**没有实例的横幅**
    // 看起来像某个具体实例 —— 而那正是"占位必须明确标注"要避免的。
    expect(coverToneAttr(undefined)).toBe(COVER_TONE_NONE);
  });
});

describe("🔴 TS 的档数与 CSS 的规则数**必须对得上**", () => {
  it("⚠️ 样式表**真的读到了**（一个空字符串会让下面三条全部静默通过）", () => {
    // ⚠️ 这一条不是形式主义：`?raw` 那条路给过 **`''`**，
    // 而那时"12 档齐不齐""有没有字面色值"三条**全绿**。
    expect(css.length).toBeGreaterThan(1000);
    expect(css).toContain(".home__hero");
  });

  it(`样式表里有 ${COVER_TONES} 条 data-cover-tone 规则`, () => {
    // ⚠️ 这条是**跨文件**的：`coverTone` 只会返回 0..11，
    // 而一个"TS 加到 16 档、CSS 只写了 12 条"的改动会让 12..15 档
    // **静默地**拿到默认色（没有报错、没有测试失败、只有颜色不对）。
    const declared = new Set(
      [...css.matchAll(/\[data-cover-tone="(\d+)"\]/g)].map((m) => Number(m[1])),
    );
    expect([...declared].sort((a, b) => a - b)).toEqual(
      Array.from({ length: COVER_TONES }, (_, i) => i),
    );
  });

  it('而"没有实例"那一档 `none` 也有规则', () => {
    expect(css).toContain(`[data-cover-tone="${COVER_TONE_NONE}"]`);
  });

  it("⚠️ 这个样式表里**一个色值都没有**（全部走令牌）", () => {
    // ⚠️ `eslint.config.js` 的字面颜色规则只覆盖 `web/src/**/*.{ts,tsx}` ——
    // **它看不见 `.css`**。文件头那句"零字面颜色值（§2 约束 4）"在此之前
    // 只是一句声明，而这条断言让它变成可踩红的东西。
    //
    // ⚠️ **先剥注释再断言** —— 这一条当场踩过"散文被当成代码"那个坑：
    // 文件里那段注释写着 `hsl(...)` 三个字母，于是断言立刻变红，
    // 而真正的问题是**检查看的是散文**。`tools/check-css-tokens.ps1`
    // 的文件头记着同一条：*"一段关于令牌的散文不是一次令牌使用"*。
    const code = css.replace(/\/\*[\s\S]*?\*\//g, "");
    expect(code).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(code).not.toMatch(/\b(?:rgb|rgba|hsl|hsla)\(/);
  });
});
