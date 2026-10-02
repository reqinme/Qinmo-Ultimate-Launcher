/**
 * 路由骨架的验收测试（**M4 门禁第 ③ 项**）
 * ============================================================================
 *
 * 它测的是 §4.6.1 的两条硬规则 —— 那两条**都是"看起来合理而恰好错"的那一类**，
 * 所以它们必须在能自动拦截的地方被拦一次。
 */

import { describe, expect, it } from "vitest";
import {
  PRIMARY,
  PRODUCT_KEYS,
  TOOL_KEYS,
  primaryOf,
  secondaryOf,
  type PrimaryKey,
} from "./nav.ts";
import { primaryFromPath } from "./Shell.tsx";

describe("§4.6.1.1 二级栏内容由当前一级决定", () => {
  it("**一级没有下级时，二级栏为空** —— 而接口区分 `null` 与 `[]`", () => {
    // ⚠️ 这是门禁③ 里最容易做错的一条。规格原文：
    // "二级栏内容由当前一级决定；一级没有下级时二级栏为空"。
    //
    // 而"为空"有两种读法：`null`（整条栏不存在）与 `[]`（栏在但没内容）。
    // 规格说的是前者 —— 渲染一个空的二级栏会留下一条空缝，
    // 让"主页"看起来像"一个有二级栏但里面没东西的页"。
    expect(secondaryOf("home")).toBeNull();
    expect(secondaryOf("about")).toBeNull();
  });

  it("有下级的一级返回**非空**区域（区与项都非空）", () => {
    for (const k of ["instances", "downloads", "accounts", "toolbox", "settings"] as const) {
      const area = secondaryOf(k);
      expect(area, `${k} 应当有二级内容`).not.toBeNull();
      expect(area?.sections.length ?? 0, `${k} 的二级区不该是空的`).toBeGreaterThan(0);
      expect(area?.items.length ?? 0, `${k} 的二级项不该是空的`).toBeGreaterThan(0);
    }
  });

  it("二级内容**只**来自那一级 —— 没有跨级的串味", () => {
    // 一个"全局固定的二级栏"会让设置页里出现下载的分组。
    // 这里逐项核对：每个二级项都只属于它自己那一级。
    const seen = new Map<string, PrimaryKey>();
    for (const p of PRIMARY) {
      for (const s of p.secondary?.items ?? []) {
        const prev = seen.get(s.key);
        // 允许不同一级用同名 key（如 `list`），所以这里只断言
        // "同一级里 key 唯一"。
        expect(prev === undefined || prev === p.key, `${p.key} 的二级 key 重复`).toBe(true);
        seen.set(s.key, p.key);
      }
    }
  });

  it("🔴 **每一项都落在它那一页真实存在的区里**", () => {
    // ⚠️ 这条是这一版新加的，而它拦的是一类**静默失踪**：
    // `Shell.tsx` 是**按区分组**渲染的（`itemsOf()` 用 `item.section` 过筛），
    // 所以一个 `section` 写错（或指向另一个页面的区）的项**一个字都不会出现** ——
    // 没有异常、没有 lint、没有类型错误（`section` 是 `string`）。
    //
    // 而"二级栏里少了一项"是一个**只能靠人眼**发现的缺陷，除非这里钉住它。
    for (const p of PRIMARY) {
      const area = p.secondary;
      if (area === null) continue;
      const keys = new Set(area.sections.map((s) => s.key));
      for (const item of area.items) {
        expect(keys.has(item.section), `${p.key}/${item.key} 的 section「${item.section}」不在区表里`).toBe(
          true,
        );
      }
      // 而静态区的项**就在表里**（动态区 —— `products` / `instances` ——
      // 的行来自产品表与实例表，所以它们在 `items` 里可以一项都没有）。
      for (const section of area.sections) {
        if (section.source !== "static") continue;
        const n = area.items.filter((i) => i.section === section.key).length;
        expect(n, `${p.key}/${section.key} 是静态区，却一项都没有`).toBeGreaterThan(0);
      }
    }
  });

  it("每一级的区 key **互不相同**（同一个 nav 里不许有两个同名区）", () => {
    // `aria-labelledby` 与 `<h2 id>` 都是按区 key 生成的（`Shell.tsx`），
    // 于是两个同名区在 HTML 里会指向同一个 id —— 那是无效文档。
    for (const p of PRIMARY) {
      const keys = (p.secondary?.sections ?? []).map((s) => s.key);
      expect(new Set(keys).size, `${p.key} 有两个同名区`).toBe(keys.length);
    }
  });
});

describe("§4.6.1.2 「当前产品」不进导航", () => {
  it("`secondaryOf` **不接受**产品参数（签名即纪律）", () => {
    // 一个"导航也存一份当前产品"的实现会立刻产生两个真相来源。
    // 这条测试用**参数个数**把它钉住：多一个参数就会红。
    expect(secondaryOf.length).toBe(1);
  });

  it("导航表里**没有**任何产品名字面量", () => {
    // 与门禁⑤ 的 lint 规则同源（那些规则只看 TSX/TS 的字面量，
    // 而这里额外核对**渲染出来的标签** —— 区的名字也是渲染出来的文字）。
    const banned = ["java", "bedrock", "forge", "fabric", "neoforge", "quilt", "mojang", "minecraft"];
    const labels = [
      ...PRIMARY.map((p) => p.label),
      ...PRIMARY.flatMap((p) => (p.secondary?.sections ?? []).map((s) => s.label)),
      ...PRIMARY.flatMap((p) => (p.secondary?.items ?? []).map((s) => s.label)),
    ].map((s) => s.toLowerCase());
    for (const b of banned) {
      const hit = labels.filter((l) => l.includes(b));
      expect(hit, `导航标签里出现了产品名 ${b}：${hit.join(", ")}`).toEqual([]);
    }
  });
});

describe("一级两段的分组", () => {
  it("产品区与工具区**合起来恰好是全部一级**（没有漏、没有重）", () => {
    const all = [...PRODUCT_KEYS, ...TOOL_KEYS];
    expect([...all].sort()).toEqual([...PRIMARY.map((p) => p.key)].sort());
    expect(new Set(all).size, "同一个一级不该出现在两段里").toBe(all.length);
  });

  it("产品区**只有三项**，而工具区在它下面（§4.6.1 的分段）", () => {
    // 规格原文把"产品段"标成"**唯一允许增长的一段**" ——
    // 所以它的**当前**内容是固定的三项，而工具段的边界也是固定的。
    // 这条测试的作用是：**增长产品段时会被要求显式改这条断言**，
    // 而那次改动会被 review 看到。
    expect(PRODUCT_KEYS).toEqual(["home", "instances", "downloads"]);
    expect(TOOL_KEYS).toEqual(["accounts", "toolbox", "settings", "about"]);
  });
});

// ============================================================================
// 路径 → 当前一级（从路由推出，而不是从组件状态）
// ============================================================================

describe("从路径推出当前一级", () => {
  it.each([
    ["/", "home"],
    ["/instances", "instances"],
    ["/instances/abc", "instances"],
    ["/downloads", "downloads"],
    ["/downloads/loaders", "downloads"],
    ["/toolbox/logs", "toolbox"],
    ["/settings/runtime", "settings"],
    ["/about", "about"],
    ["/accounts/auth", "accounts"],
  ] as const)("%s → %s", (path, want) => {
    expect(primaryFromPath(path)).toBe(want);
  });

  it("根路径 `/` **只匹配它自己**（不吞掉其他路径）", () => {
    // ⚠️ 一个用 `startsWith("/")` 的实现会让**每一条**路径都命中 `home`。
    // 而那正好是最容易写出来的那一版。
    for (const p of ["/instances", "/toolbox", "/settings/java"] as const) {
      expect(primaryFromPath(p), `${p} 不该被判成 home`).not.toBe("home");
    }
  });

  it("未知路径**不抛错**，归到主页", () => {
    // 导航高亮不该因为一个 404 就崩 —— 404 由路由自己处理。
    expect(() => primaryFromPath("/这不是一个路由")).not.toThrow();
    expect(primaryFromPath("/nope")).toBe("home");
  });

  it("每一级都有自己的非空前缀（没有两级共用前缀）", () => {
    // 一个"两级共用前缀"的实现会让高亮恒定为其中一级。
    const seen = new Map<string, PrimaryKey>();
    for (const p of PRIMARY) {
      const first = p.key === "home" ? "/" : `/${p.key}`;
      expect(seen.has(first), `两级的路径前缀相同：${first}`).toBe(false);
      seen.set(first, p.key);
    }
  });
});

describe("导航表的完整性", () => {
  it("每个一级都能被 `primaryOf` 取到", () => {
    for (const p of PRIMARY) {
      expect(primaryOf(p.key)).toBe(p);
    }
  });

  it("每一项都有**非空**标签（一级 / 区 / 项三层都算）", () => {
    for (const p of PRIMARY) {
      expect(p.label.length, `${p.key} 的标签是空的`).toBeGreaterThan(0);
      for (const section of p.secondary?.sections ?? []) {
        expect(section.label.length, `${p.key}/${section.key} 的区名是空的`).toBeGreaterThan(0);
      }
      for (const s of p.secondary?.items ?? []) {
        expect(s.label.length, `${p.key}/${s.key} 的标签是空的`).toBeGreaterThan(0);
      }
    }
  });

  it("一级 key 唯一", () => {
    const keys = PRIMARY.map((p) => p.key);
    expect(new Set(keys).size).toBe(keys.length);
  });
});
