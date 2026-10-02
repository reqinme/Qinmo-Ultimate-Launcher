/**
 * 主页插件网格的规则测试（**M4 主干** · §4.6.8）
 * ============================================================================
 *
 * ## 它测的三条硬规则，而每一条都有一个"看起来更自然"的错法
 *
 * | 规则 | 那个"更自然"的错法 |
 * |---|---|
 * | **1. 关掉 ≠ 找不回来** | `cat.all.filter(p => p.shown)` —— 于是被关掉的**从数据里消失** |
 * | **2. 插件不是导航层级** | 给插件一个 `level` 或让它进 `PRIMARY` |
 * | **3. 同一件事不许两个入口** | 按**名字**查重（改个名字就漏了） |
 */

import { describe, expect, it } from "vitest";
import {
  ALL_SPANS,
  GRID_COLUMNS,
  PluginShape,
  PluginSpan,
  Preset,
  applyPreset,
  bringPluginToFront,
  duplicateEntryPoints,
  movePlugin,
  setPluginShape,
  setPluginShown,
  settingsPlugins,
  spanForDropWidth,
  visiblePlugins,
  type PluginCapability,
  type PluginCatalogue,
  type PluginSetting,
} from "./plugins.ts";

/** 一个五块的清单（含**两块被关掉的**）。 */
function catalogue(): PluginCatalogue {
  const p = (
    id: string,
    shown: boolean,
    span: PluginSpan = PluginSpan.Third,
    shape: PluginShape = PluginShape.Rich,
  ): PluginSetting => ({ id, shown, span, shape });
  return {
    all: [
      p("recent", true, PluginSpan.Half),
      p("quick-launch", true, PluginSpan.Quarter),
      p("downloads", true, PluginSpan.Quarter),
      p("stats", false, PluginSpan.Quarter),
      p("health", false, PluginSpan.Third),
    ],
  };
}

// ============================================================================
// 🔴 规则 1
// ============================================================================

describe("规则 1：关掉 = 主页不显示，但**设置里仍然列着**", () => {
  it("🔴 被关掉的**留在清单里**（「关掉就再也找不回来」的反面）", () => {
    // ⚠️ **这条测试是本文件存在的理由。**
    //
    // 那个错法的代码形态是 `cat.all = cat.all.filter(p => p.shown)` ——
    // 于是 `all` 从 5 块变成 3 块，而被关掉的两块**永远回不来**。
    const cat = catalogue();
    expect(cat.all.length).toBe(5);
    // 而"该画哪些"是**算出来的**，不是把清单改掉。
    expect(visiblePlugins(cat).length).toBe(3);
    // **清单仍然是 5**
    expect(cat.all.length, "`visiblePlugins` 不该改动清单").toBe(5);
  });

  it("`settingsPlugins` 返回**全部**（含当前未显示的）", () => {
    const cat = catalogue();
    expect(settingsPlugins(cat).length).toBe(5);
    expect(settingsPlugins(cat).map((p) => p.id)).toContain("stats");
  });

  it("而「该画哪些」与「该列哪些」是**两个函数**（不是一个带参数的）", () => {
    // 一个 `plugins(forSettings: boolean)` 的实现会让调用点看不出
    // 自己拿到的是哪一份 —— 而那个区别正是规则 1 的全部内容。
    const cat = catalogue();
    expect(visiblePlugins(cat).length).not.toBe(settingsPlugins(cat).length);
  });

  it("`visiblePlugins` **不改动**输入的清单（纯函数）", () => {
    const cat = catalogue();
    const before = JSON.stringify(cat);
    void visiblePlugins(cat);
    expect(JSON.stringify(cat)).toBe(before);
  });
});

// ============================================================================
// 占宽四档
// ============================================================================

describe("占宽四档（§4.6.8 的 12 列网格）", () => {
  it("四档就是 3 / 4 / 6 / 12，且都在 1..12 内", () => {
    expect([...ALL_SPANS]).toEqual([3, 4, 6, 12]);
    for (const s of ALL_SPANS) {
      expect(s).toBeGreaterThan(0);
      expect(s).toBeLessThanOrEqual(GRID_COLUMNS);
    }
  });

  it("**默认档是 1/3**（`span 4`）", () => {
    // §4.6.8 的表里 `1/3 宽` 那一行的用途写着"**默认档**"。
    expect(PluginSpan.Third).toBe(4);
  });
});

describe("拖动落点 → 占宽档", () => {
  it.each([
    [3, PluginSpan.Quarter],
    [4, PluginSpan.Third],
    [6, PluginSpan.Half],
    [12, PluginSpan.Full],
  ])("落点 %i 列 ⇒ span %i", (cols, want) => {
    expect(spanForDropWidth(cols)).toBe(want);
  });

  it("落在两档之间时取**最接近**的那一档", () => {
    expect(spanForDropWidth(5)).toBe(PluginSpan.Third); // 4 比 6 近
    expect(spanForDropWidth(9)).toBe(PluginSpan.Half); // 6 比 12 近
  });

  it("任何落点都给出**四档之一**（不产生中间值）", () => {
    for (let c = 1; c <= GRID_COLUMNS; c += 1) {
      expect(ALL_SPANS).toContain(spanForDropWidth(c));
    }
  });
});

// ============================================================================
// 重排
// ============================================================================

describe("重排（拖动）", () => {
  it("**移动一次就同时改了顺序与占宽**（§4.6.8）", () => {
    const cat = catalogue();
    const next = movePlugin(cat, 0, 4, PluginSpan.Full);
    expect(next.all[4]?.id).toBe("recent");
    expect(next.all[4]?.span).toBe(PluginSpan.Full);
  });

  it("🔴 **重排不丢块**（含被关掉的）", () => {
    // ⚠️ 一个"移动时重建数组"的实现很容易**顺手丢掉**被关掉的那些 ——
    // 而那正是"关掉就再也找不回来"的另一种形态。
    const cat = catalogue();
    const next = movePlugin(cat, 0, 4, PluginSpan.Half);
    expect(next.all.length).toBe(5);
    expect(new Set(next.all.map((p) => p.id))).toEqual(new Set(cat.all.map((p) => p.id)));
  });

  it("越界的下标**原样返回**（不 panic、不产出一个残缺的清单）", () => {
    const cat = catalogue();
    expect(movePlugin(cat, -1, 0, PluginSpan.Third)).toBe(cat);
    expect(movePlugin(cat, 0, 99, PluginSpan.Third)).toBe(cat);
  });

  it("它是**不可变**的（返回新数组，原清单不动）", () => {
    const cat = catalogue();
    const order = cat.all.map((p) => p.id);
    void movePlugin(cat, 0, 4, PluginSpan.Third);
    expect(cat.all.map((p) => p.id)).toEqual(order);
  });
});

// ============================================================================
// 一键预设
// ============================================================================

describe("一键预设（丰富 / 精简 / 极简）", () => {
  it("丰富：全部显示、全部 rich", () => {
    const next = applyPreset(catalogue(), Preset.Rich);
    expect(next.all.every((p) => p.shown)).toBe(true);
    expect(next.all.every((p) => p.shape === PluginShape.Rich)).toBe(true);
  });

  it("精简：全部显示，而形态全变 minimal", () => {
    const next = applyPreset(catalogue(), Preset.Simplified);
    expect(next.all.every((p) => p.shown)).toBe(true);
    expect(next.all.every((p) => p.shape === PluginShape.Minimal)).toBe(true);
  });

  it("极简：**只留前几块**，而其余**仍然在清单里**", () => {
    const next = applyPreset(catalogue(), Preset.Minimal);
    expect(visiblePlugins(next).length).toBe(3);
    // 🔴 **而清单还是 5 块** —— 规则 1 在预设上同样成立。
    expect(next.all.length).toBe(5);
    expect(settingsPlugins(next).length).toBe(5);
  });

  it("🔴 预设**不改占宽**（那是用户拖出来的）", () => {
    // §4.6.8 说预设管的是"各插件的**开关与形态**" —— 而占宽不在其中。
    // 一个把占宽也一起重置的预设会让用户拖了半天的布局一次点掉。
    const cat = catalogue();
    const next = applyPreset(cat, Preset.Minimal);
    expect(next.all.map((p) => p.span)).toEqual(cat.all.map((p) => p.span));
  });

  it("三种预设**互不相同**（不是恒等于某一个）", () => {
    const cat = catalogue();
    const a = JSON.stringify(applyPreset(cat, Preset.Rich).all);
    const b = JSON.stringify(applyPreset(cat, Preset.Simplified).all);
    const c = JSON.stringify(applyPreset(cat, Preset.Minimal).all);
    expect(new Set([a, b, c]).size).toBe(3);
  });
});

// ============================================================================
// 插件设置面板用的三个变换（**规则 1 的另一半**）
// ============================================================================

describe("设置面板的三个变换（`setPluginShown` / `setPluginShape` / `bringPluginToFront`）", () => {
  it("🔴 关掉一块之后**清单还是 5 块**（「找得回来」的那一半）", () => {
    // ⚠️ 规则 1 有两半：主页不画它（[`visiblePlugins`]），
    // 而**设置里仍然列着**（[`settingsPlugins`]）。
    // 一个把 `all` 缩小的 `setPluginShown` 会让第二半静默失效。
    const next = setPluginShown(catalogue(), "stats", true);
    expect(next.all.length).toBe(5);
    expect(visiblePlugins(next).length).toBe(4);
    expect(settingsPlugins(next).map((p) => p.id)).toContain("stats");
    // 而"关掉一块"同样只是把它从**可见**那一边拿走
    const off = setPluginShown(catalogue(), "recent", false);
    expect(off.all.length).toBe(5);
    expect(settingsPlugins(off).length).toBe(5);
  });

  it("`setPluginShown` 只动**那一块**（其余逐字节不变）", () => {
    const cat = catalogue();
    const next = setPluginShown(cat, "downloads", false);
    expect(next.all.filter((p) => p.id !== "downloads")).toEqual(
      cat.all.filter((p) => p.id !== "downloads"),
    );
  });

  it("`setPluginShape` 改形态而占宽一个字节不动", () => {
    const cat = catalogue();
    const next = setPluginShape(cat, "recent", PluginShape.Minimal);
    const before = cat.all.find((p) => p.id === "recent");
    const after = next.all.find((p) => p.id === "recent");
    expect(after?.shape).toBe(PluginShape.Minimal);
    expect(after?.span).toBe(before?.span);
    expect(next.all.length).toBe(cat.all.length);
  });

  it("⚠️ 不认识的 id ⇒ 原样返回（不是抛，也不是加一块）", () => {
    const cat = catalogue();
    expect(setPluginShown(cat, "nope", true).all).toEqual(cat.all);
    expect(setPluginShape(cat, "nope", PluginShape.Minimal).all).toEqual(cat.all);
    expect(bringPluginToFront(cat, "nope")).toBe(cat);
  });

  it("`bringPluginToFront` 把它挪到第 0 位，而**占宽不变**", () => {
    const cat = catalogue();
    // `downloads` 是 1/4 档 —— 而"移到最前"不该顺手把它改成默认档。
    const next = bringPluginToFront(cat, "downloads");
    expect(next.all[0]?.id).toBe("downloads");
    expect(next.all[0]?.span).toBe(PluginSpan.Quarter);
    expect(next.all.length).toBe(5);
  });

  it("`bringPluginToFront` 在**已经在最前**时原样返回（不造新数组）", () => {
    const cat = catalogue();
    expect(bringPluginToFront(cat, "recent")).toBe(cat);
  });

  it("🔴 三个变换都**不改成员集合**（只换顺序 / 标志 / 形态）", () => {
    const cat = catalogue();
    const ids = (c: PluginCatalogue): string[] => c.all.map((p) => p.id).sort();
    expect(ids(setPluginShown(cat, "stats", true))).toEqual(ids(cat));
    expect(ids(setPluginShape(cat, "stats", PluginShape.Minimal))).toEqual(ids(cat));
    expect(ids(bringPluginToFront(cat, "health"))).toEqual(ids(cat));
  });
});

// ============================================================================
// 规则 2：插件不是导航层级
// ============================================================================

describe("规则 2：插件**不是导航层级**", () => {
  it("`PluginSetting` 上没有 `level` / `parent` 这类字段", () => {
    // ⚠️ 这条断言的是**形状**：一个给插件加层级字段的实现会让
    // "关掉插件不影响任何导航"这句话不再显然。
    //
    // 而它是可执行的：清单里每个对象的键都必须是**这四个之一**。
    const keys = new Set(Object.keys(catalogue().all[0] ?? {}));
    expect([...keys].sort()).toEqual(["id", "shape", "shown", "span"]);
  });
});

// ============================================================================
// 规则 3：同一件事不许两个入口
// ============================================================================

describe("规则 3：同一件事不许两个入口", () => {
  it("没有重复能力时返回空数组", () => {
    const ps: readonly PluginCapability[] = [
      { id: "recent", capabilities: ["launch"] },
      { id: "downloads", capabilities: ["open-folder"] },
    ];
    expect(duplicateEntryPoints(ps)).toEqual([]);
  });

  it("🔴 两个插件都能启动 ⇒ **被找出来**", () => {
    // §4.6.8 原文的例子就是"最近运行"与"快速启动"都能启动。
    const ps: readonly PluginCapability[] = [
      { id: "recent", capabilities: ["launch"] },
      { id: "quick-launch", capabilities: ["launch"] },
    ];
    expect(duplicateEntryPoints(ps)).toEqual([["recent", "quick-launch"]]);
  });

  it("🔴 判据是**能力**，不是名字（改个名字仍然算重复）", () => {
    // 而这是规则 3 的实质：按名字查重的实现会在"改了个名字却还是
    // 做同一件事"时漏掉。
    const ps: readonly PluginCapability[] = [
      { id: "a-very-different-name", capabilities: ["manage-instance"] },
      { id: "another-name", capabilities: ["manage-instance"] },
    ];
    expect(duplicateEntryPoints(ps).length).toBe(1);
  });

  it("一个插件有多种能力时，只要**有一种**重叠就算重复", () => {
    const ps: readonly PluginCapability[] = [
      { id: "a", capabilities: ["launch", "open-folder"] },
      { id: "b", capabilities: ["open-folder", "stats"] },
    ];
    expect(duplicateEntryPoints(ps)).toEqual([["a", "b"]]);
  });

  it("三块互相重叠时给出**三对**（不是一对）", () => {
    const ps: readonly PluginCapability[] = [
      { id: "a", capabilities: ["launch"] },
      { id: "b", capabilities: ["launch"] },
      { id: "c", capabilities: ["launch"] },
    ];
    expect(duplicateEntryPoints(ps).length).toBe(3);
  });
});
