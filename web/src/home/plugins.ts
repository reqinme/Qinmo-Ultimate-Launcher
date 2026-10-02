/**
 * 主页插件网格的规则（**M4 主干** · `docs/UI设计规格.md` §4.6.8）
 * ============================================================================
 *
 * > **这是我们自己的机制，不是"照参考图抄的一个首页"。** 参考图只提供了
 * > **形式**（卡片式、可开关），**内容与规则全部是我们自己的**。
 *
 * ## 🔴 三条硬规则（§4.6.8 原文）
 *
 * | # | 规则 | 理由（原文） |
 * |---|---|---|
 * | **1** | **开关关掉 = 主页不显示，但设置里仍然列着** | **"否则关掉就再也找不回来了"** —— 这是"可配置界面"最容易犯的错 |
 * | **2** | **插件不是导航层级** | 插件是主页内部的磁贴，**不参与**"一级/二级/三级" |
 * | **3** | **同一件事不许两个入口** | 若两个插件做同一件事，**必须合并或让其中一个降级为跳转** |
 *
 * ## 而规则 1 决定了本模块的**形状**
 *
 * "关掉就再也找不回来"这个错法的代码形态是：`plugins.filter(p => p.enabled)` ——
 * 于是被关掉的那些**从数据里消失了**。
 *
 * **所以本模块里没有 `enabled` 这个过滤步骤**：清单**永远包含全部**，
 * 而"该不该画"由 [`visiblePlugins`] 算 —— 一个**纯函数**，
 * 而设置页拿的是**原清单**。
 */

/** 12 列网格（§4.6.8）。 */
export const GRID_COLUMNS = 12;

/**
 * 插件占宽四档（§4.6.8 的表）。
 *
 * ⚠️ **它是"跨几列"，而不是百分比** —— 因为底层是 12 列网格，
 * 而"拖动插件到别的位置时，按落点决定它变成哪一档"需要一个**离散**的答案。
 */
export const PluginSpan = {
  /** `span 3` —— 小件。 */
  Quarter: 3,
  /** `span 4` —— **默认档**。 */
  Third: 4,
  /** `span 6` —— 宽件。 */
  Half: 6,
  /** `span 12` —— 少数需要全宽的。 */
  Full: 12,
} as const;
export type PluginSpan = (typeof PluginSpan)[keyof typeof PluginSpan];

/** 四档，**按宽度升序**。 */
export const ALL_SPANS: readonly PluginSpan[] = [
  PluginSpan.Quarter,
  PluginSpan.Third,
  PluginSpan.Half,
  PluginSpan.Full,
];

/**
 * 形态：丰富 / 极简（§4.6.8）。
 *
 * ⚠️ **插件级只有两档** —— "精简"是**预设级**的（它一次性给所有插件定形）。
 * 一个把三档都放到插件上的实现会让"一块丰富一块精简"成为可能，
 * 而那正是"一键预设"想避免的。
 */
export const PluginShape = { Rich: "rich", Minimal: "minimal" } as const;
export type PluginShape = (typeof PluginShape)[keyof typeof PluginShape];

/** 一键预设：**切一次，各插件的开关与形态一起变**（§4.6.8）。 */
export const Preset = { Rich: "rich", Simplified: "simplified", Minimal: "minimal" } as const;
export type Preset = (typeof Preset)[keyof typeof Preset];

/** 一个插件的用户设置。 */
export interface PluginSetting {
  /** 唯一 id（**内核/前端都用它**，不是显示名）。 */
  readonly id: string;
  /** **它在主页上显示吗。** ⚠️ `false` **不代表它不在清单里** —— 见模块文档。 */
  readonly shown: boolean;
  readonly span: PluginSpan;
  readonly shape: PluginShape;
}

/** 完整清单：**永远包含全部插件**（规则 1 的落点）。 */
export interface PluginCatalogue {
  readonly all: readonly PluginSetting[];
}

/** 主页上该画哪些 —— **纯函数**，而设置页拿的是 `all`。 */
export function visiblePlugins(cat: PluginCatalogue): readonly PluginSetting[] {
  // ⚠️ **这一行就是规则 1 与"关掉就再也找不回来"的分界线。**
  //
  // 一个 `filter` 之后**丢掉**被关掉那些的实现（例如把结果存回配置）
  // 会让它们从清单里消失。而这里**只过滤返回值**，`cat.all` 一个字节不动。
  return cat.all.filter((p) => p.shown);
}

/** 设置页该列哪些 —— **全部**（含当前未显示的）。 */
export function settingsPlugins(cat: PluginCatalogue): readonly PluginSetting[] {
  return cat.all;
}

/**
 * 一键预设：**它作用在整份清单上**，而不是某一块。
 *
 * | 预设 | 效果 |
 * |---|---|
 * | `rich` | 全部显示，全部 `rich` |
 * | `simplified` | 全部显示，全部 `minimal`（但占宽不变） |
 * | `minimal` | **只留前三块**，全部 `minimal` |
 *
 * ⚠️ **"极简"会关掉一些插件，而它们仍然留在 `all` 里** —— 于是
 * `settingsPlugins` 照旧列得出它们（规则 1）。
 */
export function applyPreset(cat: PluginCatalogue, preset: Preset): PluginCatalogue {
  // ⚠️ **"极简"保留前几块** —— 而那是一个**明确的产品判断**，不是"取前三个"。
  // 主页上至少要有"最近运行"那类**唯一入口**的东西，否则极简形态会让
  // 用户找不到任何能启动游戏的入口。
  const KEEP_UNDER_MINIMAL = 3;
  return {
    all: cat.all.map((p, i) => ({
      ...p,
      shown: preset === Preset.Minimal ? i < KEEP_UNDER_MINIMAL : true,
      shape: preset === Preset.Rich ? PluginShape.Rich : PluginShape.Minimal,
      // ⚠️ **占宽不变** —— 预设管的是"开关与形态"（§4.6.8 原文），
      // 而占宽是用户拖出来的，切一次预设就把它冲掉会很难受。
      span: p.span,
    })),
  };
}

/**
 * **拖动插件到别的位置时，按落点决定它变成哪一档**（§4.6.8）。
 *
 * ⚠️ 判据是**落点所在的列宽**：拖到 1/4 宽的位置就变 1/4。
 * 一个"拖动只改顺序、占宽去设置里调"的实现正是 §4.6.8 反对的
 *（"顺序只能拖，**不要在设置页里排**（设置页看不到实时效果）"）。
 *
 * 返回值是**四档里最接近落点宽度的那一档**。
 */
export function spanForDropWidth(widthCols: number): PluginSpan {
  let best: PluginSpan = PluginSpan.Third;
  let bestDelta = Number.POSITIVE_INFINITY;
  for (const s of ALL_SPANS) {
    const d = Math.abs(s - widthCols);
    if (d < bestDelta) {
      bestDelta = d;
      best = s;
    }
  }
  return best;
}

/**
 * **重排**：把 `from` 位置的插件移到 `to`，而 `to` 的占宽按 `span` 更新。
 *
 * ⚠️ 它返回**新数组**（不可变），而 `all` 的长度与成员**一个都不少**
 * —— 一个"移动时重建数组"的实现很容易顺手丢掉被关掉的那些。
 */
export function movePlugin(
  cat: PluginCatalogue,
  from: number,
  to: number,
  span: PluginSpan,
): PluginCatalogue {
  if (from < 0 || from >= cat.all.length || to < 0 || to >= cat.all.length) {
    return cat;
  }
  const next = [...cat.all];
  const [moved] = next.splice(from, 1);
  if (moved === undefined) return cat;
  next.splice(to, 0, { ...moved, span });
  return { all: next };
}

/**
 * 打开/关掉一块插件（**规则 1 的落点**）。
 *
 * ⚠️ **它只改 `shown`，不动 `all` 的长度与成员** —— `shown: false`
 * 不是"从清单里删掉"，而是"主页不画它"。一个
 * `{ all: cat.all.filter(...) }` 的实现会让被关掉的那块**永远回不来**，
 * 而那正是规则 1 存在的理由。
 *
 * ⚠️ 而**找不回来的另一半**是"设置里仍然列着"：那个面拿到的是
 * [`settingsPlugins`]（= `all`），不是 [`visiblePlugins`]。
 */
export function setPluginShown(
  cat: PluginCatalogue,
  id: string,
  shown: boolean,
): PluginCatalogue {
  return { all: cat.all.map((p) => (p.id === id ? { ...p, shown } : p)) };
}

/** 改一块插件的形态（丰富 / 极简）。⚠️ 同样**不动清单的长度与成员**。 */
export function setPluginShape(
  cat: PluginCatalogue,
  id: string,
  shape: PluginShape,
): PluginCatalogue {
  return { all: cat.all.map((p) => (p.id === id ? { ...p, shape } : p)) };
}

/**
 * 把一块插件**移到最前**（§4.6.8 的 ⋮ 菜单三项之一）。
 *
 * ⚠️ 它**不改占宽** —— "改顺序"与"改占宽"是两件事，
 * 而拖动那一条路（[`movePlugin`]）才同时做两件。
 * 一个在这里顺手把 `span` 重置成默认档的实现会让"移到最前"变成"顺手改小"。
 */
export function bringPluginToFront(cat: PluginCatalogue, id: string): PluginCatalogue {
  const from = cat.all.findIndex((p) => p.id === id);
  // 已经在最前（或找不到）⇒ 原样返回，而不是造一份新数组。
  if (from <= 0) return cat;
  const next = [...cat.all];
  const [moved] = next.splice(from, 1);
  if (moved === undefined) return cat;
  next.unshift(moved);
  return { all: next };
}

/**
 * 规则 3 的**机器版**：找出做同一件事的两个插件。
 *
 * ⚠️ 规格原文：*"若两个插件做同一件事（例如'最近运行'与'快速启动'都能启动），
 * **必须合并或让其中一个降级为跳转**"*。
 *
 * 而"做同一件事"的判据是**它声明的能力**（`capability`），不是它的名字 ——
 * 一个按名字查重的实现会在"改了个名字却还是做同一件事"时漏掉。
 */
export interface PluginCapability {
  readonly id: string;
  /** 它能做的事（例如 `launch` / `manage-instance` / `open-folder`）。 */
  readonly capabilities: readonly string[];
}

/** 返回做同一件事的插件对（空数组 = 没有重复入口）。 */
export function duplicateEntryPoints(
  plugins: readonly PluginCapability[],
): readonly (readonly [string, string])[] {
  const out: (readonly [string, string])[] = [];
  for (let i = 0; i < plugins.length; i += 1) {
    for (let j = i + 1; j < plugins.length; j += 1) {
      const a = plugins[i];
      const b = plugins[j];
      if (a === undefined || b === undefined) continue;
      if (a.capabilities.some((c) => b.capabilities.includes(c))) {
        out.push([a.id, b.id]);
      }
    }
  }
  return out;
}
