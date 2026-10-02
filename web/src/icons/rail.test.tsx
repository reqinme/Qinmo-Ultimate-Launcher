/**
 * 七个导航图形：**表 → 屏幕上真的画了什么**
 * ============================================================================
 *
 * ## 这个文件守的是"窄栏能不能用"，而不只是"图形好不好看"
 *
 * §4.6.2 把左栏定成"窄栏 56 px 仅图标 / 宽栏 200 px 图标 + 名称"两态，
 * 而**两态共用同一套图标**。窄栏下**名称是不显示的**（§4.6.2 第 4 条还要求
 * 它是 `display:none` 而不是缺失），于是"这一项是什么"**只剩图形在说**：
 *
 * | 如果图形出错 | 用户看到的是 |
 * |---|---|
 * | 两个图形一样 | 两项**无法区分**，只能靠点击去试 |
 * | 一条路径跑到格子外 | 图形被裁掉一角，看起来像渲染坏了 |
 * | 图形画成了别的东西 | **它比没有图形更坏** —— 图形会让人确信一个错误的意思 |
 *
 * 所以这里钉四件事：
 *
 * 1. **① 表自洽**：七个名字、每个至少一条路径、**七个图形两两不同**。
 * 2. **② 画的就是表里那条路径**，且七条的坐标都收在 3…21 的格子里。
 * 3. **③ `size` 真的是尺寸**（默认 20；传值生效；并且它不是 `className` 那种"让 CSS 猜"）。
 * 4. **④ 回归测试**：没有一条路径是**三条等距横线**（§4.5.2 那个缺陷）。
 *
 * ## ⚠️ 关于 ④ 为什么不写"断言 `settings` 不是汉堡菜单"
 *
 * §4.5.2 的教训是"**图形长成了别的东西，而没有任何一条测试在看图形**"。
 * 把它写成"`settings` 的路径 ≠ 那三条横线"是**假的**回归测试：
 * 它只认那一个字符串。而真正的判据是**图形本身的形状**——
 * "三根等距横线"这件事与坐标怎么写无关（`M4 7h16` 与 `M4.5 7H19.5` 是同一根线）。
 * 所以 ④ 先把**每条路径拆成竖线/横线/斜线**，再断言不存在
 * "三**横**线且 y 依次为 7 / 12 / 17"的图形。
 *
 * ⚠️ 这里与 `titlebar/glyphs.test.tsx` 的分工是**有意重复**的：
 * 那边验的是窗口控制那四个图形，这边验的是左栏这七个 —— 两套图形、
 * 两个文件，而"三条等距横线"这个形状两边都不许出现。
 */

import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { RAIL_ICON_NAMES, RAIL_ICON_PATHS, RailIcon } from "./rail.tsx";

/**
 * 某个图形**表里**的那几条路径。
 *
 * ⚠️ 不走 `Object.entries` 之类的放宽写法，而是**逐位按名字表取** ——
 * 这样"表里多了一个名字而这个名字没有图形"会当场红，
 * 而不是静静地少测一项。
 */
function pathsOf(name: (typeof RAIL_ICON_NAMES)[number]): string[] {
  return [...RAIL_ICON_PATHS[name]];
}

/**
 * 渲染一个图形，取出它**真的画出来**的那几条 `d`。
 *
 * ⚠️ `size` 用**条件展开**而不是 `size={size}`：`tsconfig` 开着
 * `exactOptionalPropertyTypes`，于是"显式传一个 `undefined`"与"不传"是两件事
 * （前者会被拒绝）。而这里两者都可能是"不传" —— 不这样就验不了默认值。
 */
function drawn(name: (typeof RAIL_ICON_NAMES)[number], size?: number): string[] {
  const { container } = render(<RailIcon name={name} {...(size === undefined ? {} : { size })} />);
  return [...container.querySelectorAll("svg path")].map((p) => p.getAttribute("d") ?? "");
}

describe("① 图形表自洽（七个名字，七个不同的图形）", () => {
  it("名字表就是 §4.6.1 那七项，且路径表的键与它**逐位**相同", () => {
    expect([...RAIL_ICON_NAMES]).toEqual([
      "home",
      "instances",
      "downloads",
      "accounts",
      "toolbox",
      "settings",
      "about",
    ]);
    // 逐位比，而不是"排序后比"：顺序是"上段三项 + 下段四项"这个信息本身，
    // 而排序比法会把"下载与实例对调了"放过。
    expect(Object.keys(RAIL_ICON_PATHS)).toEqual([...RAIL_ICON_NAMES]);
  });

  it("每个图形至少一条非空路径，而七个图形两两不同", () => {
    const seen = new Map<string, string>();
    for (const name of RAIL_ICON_NAMES) {
      const paths = pathsOf(name);
      expect(paths.length, `${name} 至少要有一条路径`).toBeGreaterThan(0);
      for (const d of paths) {
        expect(d.trim().length, `${name} 的路径不能是空的`).toBeGreaterThan(0);
      }
      const key = paths.join(" | ");
      expect(seen.has(key), `${name} 与 ${seen.get(key) ?? "?"} 画的是同一个图形`).toBe(false);
      seen.set(key, name);
    }
    // 七个都进来过（防止上面的循环因为某个名字没被遍历而少查一项）。
    expect(seen.size).toBe(RAIL_ICON_NAMES.length);
  });
});

describe("② 渲染出来的 `<svg>` 就是表里那几条路径，且都在格子里", () => {
  it.each([...RAIL_ICON_NAMES])("%s：`d` 与表逐条相同", (name) => {
    expect(drawn(name)).toEqual(pathsOf(name));
  });

  it("每个图形都只有一个 `<svg>`，且 viewBox 是 24 × 24", () => {
    const { container } = render(<RailIcon name="home" />);
    const svgs = [...container.querySelectorAll("svg")];
    expect(svgs.length).toBe(1);
    expect(svgs[0]?.getAttribute("viewBox")).toBe("0 0 24 24");
  });

  it("图形是**装饰性**的：读屏听到的是导航项的名字，不是一个图形", () => {
    const { container } = render(<RailIcon name="accounts" />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("aria-hidden")).toBe("true");
    expect(svg?.getAttribute("focusable")).toBe("false");
  });

  it("颜色只走 `currentColor`：`stroke` 跟着文字色、`fill: none`", () => {
    const { container } = render(<RailIcon name="downloads" />);
    const svg = container.querySelector("svg");
    // §5.3：界面里禁止字面颜色值 ⇒ 图形不能自己带颜色，否则 §4.6.2 的
    // "未选中 text.secondary / 选中 text.primary"就得在这里再写一遍。
    expect(svg?.getAttribute("stroke")).toBe("currentColor");
    expect(svg?.getAttribute("fill")).toBe("none");
  });

  /**
   * 一条路径里**绝对**画点的那些坐标。
   *
   * ⚠️ **只取绝对命令（`M` / `L` / `H` / `V`）。** 相对命令里的数字是**位移**，
   * 不是坐标：`home` 的屋脊 `l8-7.4` 里 `-7.4` 是"往上 7.4 个单位"，
   * 而 4 + 8 = 12、11 − 7.4 = 3.6 都好好地在格子里。
   * 把位移当坐标判，是一条**会误报的**检查 —— 而误报的检查最后一定被人关掉。
   */
  function absolutePoints(d: string): { readonly axis: string; readonly at: number }[] {
    const points: { axis: string; at: number }[] = [];
    const CMD = /\b([MLHV])((?:\s*-?\d+(?:\.\d+)?)+)/g;
    for (const m of d.matchAll(CMD)) {
      const nums = (m[2] ?? "").match(/-?\d+(?:\.\d+)?/g) ?? [];
      const cmd = m[1];
      if (cmd === "M" || cmd === "L") {
        points.push({ axis: "x", at: Number(nums[0]) }, { axis: "y", at: Number(nums[1]) });
      } else if (cmd === "H") {
        points.push({ axis: "x", at: Number(nums[0]) });
      } else {
        points.push({ axis: "y", at: Number(nums[0]) });
      }
    }
    return points;
  }

  it("七条图形的坐标都收在 3…21（16 px 下留得住内边距）", () => {
    let checked = 0;
    for (const name of RAIL_ICON_NAMES) {
      for (const d of pathsOf(name)) {
        for (const p of absolutePoints(d)) {
          checked += 1;
          expect(p.at, `${name} 的路径 ${d} 里 ${p.axis}=${p.at} 顶到格子边上了`).toBeGreaterThanOrEqual(3);
          expect(p.at, `${name} 的路径 ${d} 里 ${p.axis}=${p.at} 顶到格子边上了`).toBeLessThanOrEqual(21);
        }
      }
    }
    // ⚠️ 没有这一条，一次写坏的正则会让上面那个循环**一个点都不查**，
    // 而它仍然是绿的（"检查了零个东西"与"检查通过"必须分得开）。
    expect(checked, "至少要真的量到几十个坐标").toBeGreaterThan(30);
  });
});

describe("③ `size` 是尺寸的唯一来源", () => {
  it("默认 20 px（§4.4 的「卡片内 16–20 px」）", () => {
    const { container } = render(<RailIcon name="toolbox" />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("width")).toBe("20");
    expect(svg?.getAttribute("height")).toBe("20");
  });

  it.each([16, 28])("传 size=%i ⇒ 宽高都是它（两态共用一套图形靠的就是这一条）", (size) => {
    const { container } = render(<RailIcon name="settings" size={size} />);
    const svg = container.querySelector("svg");
    expect(svg?.getAttribute("width")).toBe(String(size));
    expect(svg?.getAttribute("height")).toBe(String(size));
  });
});

describe("④ 回归：没有一条路径是**三条等距横线**", () => {
  /**
   * 把一条路径拆成它的**水平段**，取出每段的 y。
   *
   * ⚠️ 只认**绝对**的水平命令（`M` / `H`）与 `h` 的**相对**形式 ——
   * 汉堡菜单那三条横线无论怎么写都落进这里：
   * `M4 7h16`、`M4.5 7H19.5`、`m4 7h16`（后者靠起点的 y 补上）。
   */
  function horizontalLines(d: string): number[] {
    const ys: number[] = [];
    const abs = /\bM\s*(-?\d+(?:\.\d+)?)\s+(-?\d+(?:\.\d+)?)((?:\s*(?:[HhVvLl]\s*-?\d+(?:\.\d+)?)+))/g;
    for (const m of d.matchAll(abs)) {
      const y = Number(m[2]);
      if ((m[3] ?? "").trimStart().startsWith("h")) ys.push(y);
    }
    // 相对起笔：`m4 7h16` —— 绝对坐标是 (0,0)，于是 y 就是第二个数本身。
    const rel = /\bm\s*(-?\d+(?:\.\d+)?)\s+(-?\d+(?:\.\d+)?)\s*h/g;
    for (const m of d.matchAll(rel)) ys.push(Number(m[2]));
    return ys;
  }

  it("🔴 没有任何图形是「三横线，y = 7 / 12 / 17」（§4.5.2 那个汉堡菜单）", () => {
    for (const name of RAIL_ICON_NAMES) {
      const paths = pathsOf(name);
      const ys: number[] = [];
      for (const d of paths) ys.push(...horizontalLines(d));
      // `☰` 的判据是**三条**横线；只有两条（例如"下载"的托盘 + 顶部）不算。
      const isHamburger = ys.length === 3 && ys.every((y) => y === 7 || y === 12 || y === 17);
      expect(isHamburger, `${name} 画成了汉堡菜单（三条等距横线）`).toBe(false);
      expect(ys, `${name} 不该有 7 / 12 / 17 这三条横线`).not.toEqual([7, 12, 17]);
    }
  });

  it("这条回归测试本身认得那个形状（否则它是一条永远绿的摆设）", () => {
    const paths = ["M4 7h16", "M4 12h16", "M4 17h16"];
    const ys: number[] = [];
    for (const d of paths) ys.push(...horizontalLines(d));
    expect(ys).toEqual([7, 12, 17]);
  });
});
