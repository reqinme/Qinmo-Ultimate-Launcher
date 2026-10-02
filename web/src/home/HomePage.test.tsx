import { useState } from "react";
import { describe, expect, it } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import { coverTone } from "./cover.ts";
import { HomePage, type HeroSummary } from "./HomePage.tsx";
import { PluginShape, PluginSpan, type PluginCatalogue } from "./plugins.ts";

/**
 * 主页的验收测试（M4 主干 · §4.6.8）
 * ============================================================================
 *
 * ## 三条硬规则本身在 `plugins.test.ts`（27 项）
 *
 * 而这里测的是**只有渲染才能验**的那部分：
 *
 * | 它 | 为什么必须在组件层验 |
 * |---|---|
 * | **横幅与插件头部不在滚动区里** | 它是 **DOM 嵌套** —— 纯函数验不到 |
 * | **三个操作都在**（⠿ / ⋮ / ⚙） | §4.6.8 说"缺一不可" |
 * | **占宽落到 `grid-column`** | 它是渲染结果 |
 * | **产品切换不写分支** | 换一组产品，组件行为完全相同 |
 */

const HERO: HeroSummary = {
  headline: "26.3 · 已就绪",
  detail: "76 个文件已部署",
  actions: [{ label: "启动", primary: true }, { label: "管理实例" }, { label: "打开目录" }],
};

const PRODUCTS = [
  { id: "a", label: "原生" },
  { id: "b", label: "另一形态" },
];

function cat(): PluginCatalogue {
  return {
    all: [
      { id: "最近运行", shown: true, span: PluginSpan.Half, shape: PluginShape.Rich },
      { id: "快速启动", shown: true, span: PluginSpan.Quarter, shape: PluginShape.Rich },
      { id: "下载队列", shown: true, span: PluginSpan.Quarter, shape: PluginShape.Rich },
      { id: "游玩统计", shown: false, span: PluginSpan.Quarter, shape: PluginShape.Rich },
    ],
  };
}

/**
 * ⚠️ **两个我踩过的坑，写在这里省得下一个人再踩。**
 *
 * ### ① 它**返回 `render()` 的结果**，而调用点是 `ui()` 而不是 `render(ui())`
 *
 * 我第一版写成"`ui()` 里 render、调用点再 `render(ui())`" ——
 * 于是把一个 `RenderResult` 当 React child 渲染，
 * **14 条测试一起报** `Objects are not valid as a React child`。
 *
 * ### ② 它**必须把新清单存回去**
 *
 * 主页是**受控**的：它只 `onCatalogueChange(newCat)`，不自己存。
 * 所以一个"回调里什么都不做"的桩会让"点了 ⋮ 之后那一块消失"这类断言
 * **看不到任何效果** —— 而那三条失败**看起来像组件坏了**，其实是桩不完整。
 */
function ui(over: Partial<Parameters<typeof HomePage>[0]> = {}) {
  return render(<Harness over={over} />);
}

function Harness({
  over,
}: {
  readonly over: Partial<Parameters<typeof HomePage>[0]>;
}): React.ReactElement {
  const [catalogue, setCatalogue] = useState<PluginCatalogue>(over.catalogue ?? cat());
  // ⚠️ **把这两个从 `over` 里摘出去**，免得与下面那两个同名属性冲突
  //（JSX 不允许重复属性）。于是清单由这一层持有，而 `over` 里其余的东西照样覆盖。
  const { catalogue: _ignoredCatalogue, onCatalogueChange: _ignoredChange, ...rest } = over;
  void _ignoredCatalogue;
  void _ignoredChange;

  return (
    <HomePage
      products={PRODUCTS}
      activeProduct="a"
      onProductChange={() => undefined}
      hero={HERO}
      renderPlugin={(id, shape) => <span>{`${id} 的正文（${shape}）`}</span>}
      {...rest}
      catalogue={catalogue}
      onCatalogueChange={setCatalogue}
    />
  );
}

// ============================================================================
// 🔴 结构：谁不滚
// ============================================================================

describe("结构（§4.6.8：横幅固定，插件区自己滚）", () => {
  it("🔴 **横幅与插件头部在滚动区之外**", () => {
    // ⚠️ 这条只能在这个层次验 —— 它是 **DOM 嵌套**。
    //
    // 原文两条：横幅"固定在顶部，随内容滚会失去'它是主角'的意思"；
    // 插件头部"**固定在滚动区之外** —— 否则插件一多，这两个按钮就被滚走了"。
    const { container } = ui();
    const scroller = container.querySelector(".home__gridWrap");
    expect(scroller).not.toBeNull();
    expect(scroller?.querySelector(".home__hero")).toBeNull();
    expect(scroller?.querySelector(".home__pluginBar")).toBeNull();
    expect(scroller?.querySelector(".home__grid")).not.toBeNull();
  });

  it("横幅上有产品切换的胶囊（§4.6.1.2 要求主页可选产品）", () => {
    const { container } = ui();
    expect(container.querySelector('[role="tablist"]')).not.toBeNull();
    expect(container.querySelectorAll('[role="tab"]').length).toBe(2);
  });

  it("三个主操作都在（启动 / 管理实例 / 打开目录）", () => {
    const { container } = ui();
    const labels = [...container.querySelectorAll(".home__actions button")].map(
      (b) => b.textContent,
    );
    expect(labels).toEqual(["启动", "管理实例", "打开目录"]);
  });
});

// ============================================================================
// 🔴 三个操作（缺一不可）
// ============================================================================

describe("每块插件的三个操作（§4.6.8：缺一不可）", () => {
  it("显示的每一块都有 ⠿ 手柄 + ⋮ 菜单 + ⚙ 设置", () => {
    const { container } = ui();
    for (const id of ["最近运行", "快速启动", "下载队列"]) {
      expect(container.querySelector(`[aria-label="${id} 的菜单"]`), `${id} 缺 ⋮`).not.toBeNull();
      expect(container.querySelector(`[aria-label="${id} 的设置"]`), `${id} 缺 ⚙`).not.toBeNull();
    }
    // ⠿ 是 `aria-hidden`（它不承载信息，只是一个抓取点），所以按类名数
    expect(container.querySelectorAll(".home__grip").length).toBe(3);
  });

  it("每块都是可拖的（`draggable`）", () => {
    const { container } = ui();
    const tiles = [...container.querySelectorAll(".home__tile")];
    expect(tiles.length).toBe(3);
    for (const t of tiles) {
      expect(t.getAttribute("draggable")).toBe("true");
    }
  });

  it("⚙ **就地**切换丰富/极简（不用跳去设置页）", () => {
    const { container } = ui();
    fireEvent.click(container.querySelector('[aria-label="快速启动 的设置"]') as HTMLElement);
    // ⚠️ 它换的是**那一块**的形态 —— 一个"设置跳去设置页"的实现正是
    // §4.6.8 反对的（"就地操作，不用跳去设置页"）。
    expect(container.querySelector('[data-plugin="快速启动"]')?.getAttribute("data-shape")).toBe(
      "minimal",
    );
    expect(container.querySelector('[data-plugin="最近运行"]')?.getAttribute("data-shape")).toBe(
      "rich",
    );
  });
});

// ============================================================================
// 🔴 规则 1 在界面上的形状
// ============================================================================

describe("规则 1 的界面形状：关掉 = 从网格消失，而**清单不变**", () => {
  it("⋮ 之后那一块从网格消失", () => {
    const { container } = ui();
    expect(container.querySelectorAll(".home__tile").length).toBe(3);
    fireEvent.click(container.querySelector('[aria-label="下载队列 的菜单"]') as HTMLElement);
    expect(container.querySelectorAll(".home__tile").length).toBe(2);
    expect(container.querySelector('[data-plugin="下载队列"]')).toBeNull();
  });

  it("初始 `shown: false` 的那一块**不画**（而它也不报错）", () => {
    const { container } = ui();
    expect(container.querySelector('[data-plugin="游玩统计"]')).toBeNull();
  });
});

// ============================================================================
// 🔴 产品切换：数据换、行为不变
// ============================================================================

describe("产品切换**不写分支**（§1.4 / §4.6.1.2）", () => {
  it("换一组产品之后组件行为**完全相同**", () => {
    // ⚠️ 这条是"界面不认识具体游戏"的机器版：
    // 把产品列表整组换掉，而组件的结构一个字节都没变。
    const a = ui({ products: [{ id: "x", label: "原生" }], activeProduct: "x" });
    const countA = a.container.querySelectorAll(".home__tile").length;
    const labelA = a.container.querySelector('[role="tab"]')?.textContent;
    a.unmount();

    const b = ui({ products: [{ id: "y", label: "另一个完全不同的东西" }], activeProduct: "y" });
    const countB = b.container.querySelectorAll(".home__tile").length;
    expect(countA).toBe(countB);
    expect(labelA).toBe("原生");
    expect(b.container.querySelector('[role="tab"]')?.textContent).toBe("另一个完全不同的东西");
  });

  it("当前产品的胶囊是 `aria-selected` 的", () => {
    const { container } = ui();
    const tabs = [...container.querySelectorAll('[role="tab"]')];
    expect(tabs[0]?.getAttribute("aria-selected")).toBe("true");
    expect(tabs[1]?.getAttribute("aria-selected")).toBe("false");
  });

  it("点另一个产品会回调（而**组件自己不存**当前产品）", () => {
    // §4.6.1.2：「当前产品」是**全局单一真源** —— 组件里再存一份
    // 会立刻产生两个真相来源。
    let picked = "";
    const { container } = ui({ onProductChange: (id) => (picked = id) });
    fireEvent.click(container.querySelectorAll('[role="tab"]')[1] as HTMLElement);
    expect(picked).toBe("b");
  });
});

// ============================================================================
// 一键预设
// ============================================================================

describe("一键预设（§4.6.8：切一次，各插件的开关与形态一起变）", () => {
  it("三个预设按钮都在，且默认是丰富", () => {
    const { container } = ui();
    const btns = [...container.querySelectorAll(".home__presetBtn")];
    expect(btns.map((b) => b.textContent)).toEqual(["丰富", "精简", "极简"]);
    expect(btns[0]?.getAttribute("aria-pressed")).toBe("true");
  });

  it("切到极简之后每块的形态都变成 minimal", () => {
    const { container } = ui();
    fireEvent.click([...container.querySelectorAll(".home__presetBtn")][2] as HTMLElement);
    const tiles = [...container.querySelectorAll(".home__tile")];
    expect(tiles.length).toBeGreaterThan(0);
    for (const t of tiles) {
      expect(t.getAttribute("data-shape")).toBe("minimal");
    }
  });

  it("再切回丰富，形态都回到 rich", () => {
    const { container } = ui();
    const btns = [...container.querySelectorAll(".home__presetBtn")];
    fireEvent.click(btns[2] as HTMLElement);
    fireEvent.click(btns[0] as HTMLElement);
    for (const t of [...container.querySelectorAll(".home__tile")]) {
      expect(t.getAttribute("data-shape")).toBe("rich");
    }
  });

  it("极简之后**网格不为空**（至少还留着能启动游戏的入口）", () => {
    // ⚠️ 一个"极简 = 全关掉"的实现会让主页变成一张白纸 ——
    // 而那与 §4.6.8 的"只留最关键的一行/一个按钮"完全不同。
    const { container } = ui();
    fireEvent.click([...container.querySelectorAll(".home__presetBtn")][2] as HTMLElement);
    expect(container.querySelectorAll(".home__tile").length).toBeGreaterThan(0);
  });
});

// ============================================================================
// 占宽
// ============================================================================

describe("占宽（§4.6.8 的 12 列网格）", () => {
  it("每一块的 `grid-column` 就是它的 span（不是统一一个值）", () => {
    const { container } = ui();
    const spans = [...container.querySelectorAll(".home__tile")].map(
      (t) => (t as HTMLElement).style.gridColumn,
    );
    expect(spans).toContain("span 6"); // 最近运行是 1/2 档
    expect(spans.filter((s) => s === "span 3").length).toBe(2);
  });
});

// ============================================================================
// 🔴 横幅的"画面"层（§4.6.1 的封面图纪律）
// ============================================================================

describe("横幅画面：**自绘**、配色稳定、没有实例时**显式标注占位**", () => {
  it("画面层在，而且是 `aria-hidden`（它不承载信息）", () => {
    const { container } = ui();
    expect(container.querySelector(".home__cover")).not.toBeNull();
    expect(container.querySelector(".home__cover")?.getAttribute("aria-hidden")).toBe("true");
  });

  it("有实例 ⇒ `data-cover-tone` 就是 `coverTone(id)`（**颜色由 id 稳定决定**）", () => {
    // ⚠️ 它断言的是**接线**：组件把 `hero.coverId` 交给 `coverToneAttr`。
    // 而"同一个 id 永远同色"本身在 `cover.test.ts` 里验（那才是纯函数的事）。
    const { container } = ui({ hero: { ...HERO, coverId: "26.3" } });
    expect(container.querySelector(".home__hero")?.getAttribute("data-cover-tone")).toBe(
      String(coverTone("26.3")),
    );
    // 有实例时**不该**出现"占位"标注 —— 那会让人以为真实画面还没做。
    expect(container.querySelector(".home__coverNote")).toBeNull();
  });

  it("🔴 没有实例 ⇒ `none`，**并且明确标注占位**", () => {
    // §4.6.1 的三档做法里最后一档：*"没有图时用中性渐变 + 明确标注'占位'"*。
    const { container } = ui();
    expect(container.querySelector(".home__hero")?.getAttribute("data-cover-tone")).toBe("none");
    expect(container.querySelector(".home__coverNote")?.textContent).toContain("占位");
  });
});

// ============================================================================
// 🔴 两个入口：＋添加插件 / ⚙插件设置
// ============================================================================

/** 插件头部那两个按钮之一（按可见文字找）。 */
function toolButton(container: HTMLElement, text: string): HTMLElement {
  const hit = [...container.querySelectorAll(".home__pluginTools button")].find((b) =>
    (b.textContent ?? "").includes(text),
  );
  if (hit === undefined) throw new Error(`找不到「${text}」按钮`);
  return hit as HTMLElement;
}

/** 对话框里的那些行（Radix 把它渲染到 `document.body` 的 portal 里）。 */
function settingsRows(): string[] {
  const dialog = document.querySelector(String.raw`[role="dialog"]`);
  return [...(dialog?.querySelectorAll(".home__settingsRow") ?? [])].map(
    (r) => r.getAttribute("data-plugin") ?? "",
  );
}

describe("插件头部的两个入口（§4.6.8：＋添加插件 / ⚙插件设置）", () => {
  it("两个按钮都在，且**都在滚动区之外**", () => {
    // 原文：*"插件头部（＋添加 / ⚙设置）**固定在滚动区之外** ——
    // 否则插件一多，这两个按钮就被滚走了"*。
    const { container } = ui();
    const tools = container.querySelector(".home__pluginTools");
    expect(tools).not.toBeNull();
    expect(container.querySelector(".home__gridWrap .home__pluginTools")).toBeNull();
    const labels = [...(tools?.querySelectorAll("button") ?? [])].map((b) => b.textContent ?? "");
    expect(labels.some((l) => l.includes("添加插件"))).toBe(true);
    expect(labels.some((l) => l.includes("插件设置"))).toBe(true);
  });

  it("🔴 `⚙插件设置` 列出**含未显示**的全部（规则 1 的后半句）", () => {
    // ⚠️ 这一条就是"关掉就再也找不回来"的机器版：夹具里的「游玩统计」
    // 是 `shown: false`（网格里没有它），而它**必须**出现在这个列表里。
    const { container } = ui();
    fireEvent.click(toolButton(container, "插件设置"));
    expect(document.querySelector(String.raw`[role="dialog"]`)).not.toBeNull();
    expect(settingsRows()).toEqual(["最近运行", "快速启动", "下载队列", "游玩统计"]);
  });

  it("`＋添加插件` **只列当前没显示的**那些", () => {
    const { container } = ui();
    fireEvent.click(toolButton(container, "添加插件"));
    expect(settingsRows()).toEqual(["游玩统计"]);
  });

  it("🔴 拨开关能让被关掉的那块**回到网格**（规则 1 的另一半）", () => {
    const { container } = ui();
    expect(container.querySelector('[data-plugin="游玩统计"]')).toBeNull();
    fireEvent.click(toolButton(container, "添加插件"));
    const sw = document.querySelector(String.raw`[role="dialog"] [role="switch"]`);
    expect(sw).not.toBeNull();
    fireEvent.click(sw as HTMLElement);
    expect(container.querySelector('[data-plugin="游玩统计"]')).not.toBeNull();
  });

  it("`移到最前` 真的把它挪到第一位（而**不改它的占宽**）", () => {
    const { container } = ui();
    fireEvent.click(toolButton(container, "插件设置"));
    const row = document.querySelector(String.raw`[role="dialog"] [data-plugin="下载队列"]`);
    const front = [...(row?.querySelectorAll("button") ?? [])].find((b) =>
      (b.textContent ?? "").includes("移到最前"),
    );
    fireEvent.click(front as HTMLElement);
    const order = [...container.querySelectorAll(".home__tile")].map((t) =>
      t.getAttribute("data-plugin"),
    );
    expect(order[0]).toBe("下载队列");
    // ⚠️ "移到最前"与"改占宽"是两件事 —— 一个顺手把 span 重置的实现
    // 会让"移到最前"变成"顺手改小"。
    expect(
      (container.querySelector('[data-plugin="下载队列"]') as HTMLElement).style.gridColumn,
    ).toBe("span 3");
  });

  it("全都在主页上时给**一行**空态提示（§4.6.8：「空态不占半屏」）", () => {
    const all: PluginCatalogue = {
      all: cat().all.map((p) => ({ ...p, shown: true })),
    };
    const { container } = ui({ catalogue: all });
    fireEvent.click(toolButton(container, "添加插件"));
    const empty = document.querySelector(String.raw`[role="dialog"] .home__settingsEmpty`);
    expect(empty?.textContent).toContain("都已经在主页上");
    expect(settingsRows()).toEqual([]);
  });
});
