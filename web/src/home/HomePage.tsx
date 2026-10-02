import { useState, type ReactElement, type ReactNode } from "react";
import {
  PluginShape,
  PluginSpan,
  Preset,
  applyPreset,
  movePlugin,
  spanForDropWidth,
  visiblePlugins,
  type PluginCatalogue,
} from "./plugins.ts";
import "./HomePage.css";

/**
 * 主页（**M4 主干** · §4.6.8）
 * ============================================================================
 *
 * ## 结构（§4.6.8 原文）
 *
 * ```text
 * 主内容区
 * ┌── 大横幅（**固定**）──────────────────────────────┐
 * │ [Java 版|基岩版]   产品切换                        │  ← §4.6.1.2 要求主页可选产品
 * │ 1.20.1 · Fabric 已就绪 · 38 个模组                 │
 * │ [▶ 启动] [管理实例] [打开目录]                      │
 * └───────────────────────────────────────────────────┘
 *   插件  拖拽调位置与占宽        [＋添加插件] [⚙插件设置]   ← 头部固定
 * ┌── 插件区（**这块滚**）──────────────────────────────┐
 * │ … 12 列网格 …                                      │
 * └───────────────────────────────────────────────────┘
 * ```
 *
 * ## 🔴 三条"不动"的落点
 *
 * | 谁 | 为什么 |
 * |---|---|
 * | **横幅** | §4.6.8：*"固定在顶部，**随内容滚会失去'它是主角'的意思**"* |
 * | **插件头部**（＋添加 / ⚙设置） | 原文：*"**固定在滚动区之外** —— 否则插件一多，这两个按钮就被滚走了"* |
 * | **插件区** | **它才滚** |
 *
 * ## 而"产品切换"这一段**不写分支**
 *
 * 规格 §4.6.1.2 要求主页可选产品，而 §1.4 的纪律是界面不认识具体游戏。
 * 所以产品列表由 `products` 属性给（来自后端的能力描述符），
 * 而这里只画胶囊 —— **没有一处** `if (product === "…")`。
 */

/** 一个产品（**来自后端的能力描述符**）。 */
export interface ProductChip {
  readonly id: string;
  readonly label: string;
}

/** 横幅上的一条状态摘要。 */
export interface HeroSummary {
  readonly headline: string;
  readonly detail: string;
  readonly actions: readonly { readonly label: string; readonly primary?: boolean; readonly onClick?: () => void }[];
}

export interface HomePageProps {
  readonly products: readonly ProductChip[];
  readonly activeProduct: string;
  readonly onProductChange: (id: string) => void;
  readonly hero: HeroSummary;
  readonly catalogue: PluginCatalogue;
  readonly onCatalogueChange: (c: PluginCatalogue) => void;
  /** 每块插件的正文（**由调用方给** —— 组件不认识任何插件）。 */
  readonly renderPlugin: (id: string, shape: PluginShape) => ReactNode;
}

export function HomePage({
  products,
  activeProduct,
  onProductChange,
  hero,
  catalogue,
  onCatalogueChange,
  renderPlugin,
}: HomePageProps): ReactElement {
  const [preset, setPreset] = useState<Preset>(Preset.Rich);
  const shown = visiblePlugins(catalogue);

  return (
    <div className="home">
      {/*
        🔴 **横幅固定**（它不在这块滚动区里）。
        一个把横幅放进滚动区的实现会在插件一多时把它滚走 ——
        而 §4.6.8 说那"会失去'它是主角'的意思"。
      */}
      <section className="home__hero" aria-label="启动横幅">
        {/*
          ⚠️ **产品切换由调用方给数据** —— 这里没有一处判断。
          §1.4：界面不认识具体游戏；§4.6.1.2：当前产品是全局单一真源。
        */}
        <div className="home__products" role="tablist" aria-label="产品">
          {products.map((p) => (
            <button
              key={p.id}
              type="button"
              role="tab"
              aria-selected={p.id === activeProduct}
              className={
                p.id === activeProduct ? "home__product home__product--on" : "home__product"
              }
              onClick={() => onProductChange(p.id)}
            >
              {p.label}
            </button>
          ))}
        </div>

        <h1 className="home__headline">{hero.headline}</h1>
        <p className="home__detail">{hero.detail}</p>

        <div className="home__actions">
          {hero.actions.map((a) => (
            <button
              key={a.label}
              type="button"
              className={a.primary === true ? "btn btn--primary" : "btn"}
              onClick={a.onClick}
            >
              {a.label}
            </button>
          ))}
        </div>
      </section>

      {/*
        🔴 **插件头部在滚动区之外**（§4.6.8 的原文要求）。
      */}
      <header className="home__pluginBar">
        <span className="home__pluginLabel">插件</span>
        <span className="home__pluginHint">拖动调位置与占宽</span>

        <div className="home__preset" role="group" aria-label="一键预设">
          {(
            [
              [Preset.Rich, "丰富"],
              [Preset.Simplified, "精简"],
              [Preset.Minimal, "极简"],
            ] as const
          ).map(([v, label]) => (
            <button
              key={v}
              type="button"
              className={preset === v ? "home__presetBtn home__presetBtn--on" : "home__presetBtn"}
              aria-pressed={preset === v}
              onClick={() => {
                setPreset(v);
                // ⚠️ **预设作用在整份清单上** —— 而被关掉的插件
                // **仍然留在 `all` 里**（规则 1）。
                onCatalogueChange(applyPreset(catalogue, v));
              }}
            >
              {label}
            </button>
          ))}
        </div>
      </header>

      {/*
        🔴 **只有这一块滚。**
      */}
      <section className="home__gridWrap" aria-label="插件">
        <div className="home__grid">
          {shown.map((p, i) => (
            <article
              className="home__tile"
              key={p.id}
              style={{ gridColumn: `span ${p.span}` }}
              data-plugin={p.id}
              data-shape={p.shape}
              /*
                ⚠️ **三个操作缺一不可**（§4.6.8 的表）：
                ⠿ 拖拽手柄 / ⋮ 菜单 / ⚙ 设置。
                而这里 `draggable` 用的是原生 HTML5 拖放 —— 一个自绘拖拽
                的实现要重写键盘可达性，而原生的那个**空格键就能拖**。
              */
              draggable
              onDragStart={(e) => {
                e.dataTransfer?.setData("text/plain", String(i));
              }}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.preventDefault();
                const from = Number(e.dataTransfer?.getData("text/plain") ?? "-1");
                if (!Number.isInteger(from) || from < 0) return;
                // ⚠️ **落点决定占宽**（§4.6.8："拖动插件到别的位置时，
                // 按落点决定它变成哪一档"）。
                const rect = e.currentTarget.getBoundingClientRect();
                const cols = Math.max(
                  1,
                  Math.round((e.clientX - rect.left) / (rect.width / PluginSpan.Third)) *
                    PluginSpan.Third,
                );
                onCatalogueChange(movePlugin(catalogue, from, i, spanForDropWidth(cols)));
              }}
            >
              <header className="home__tileBar">
                <span className="home__grip" aria-hidden="true" title="拖动调位置与占宽">
                  ⠿
                </span>
                <h2 className="home__tileTitle">{p.id}</h2>
                <div className="home__tileActions">
                  {/* ⋮ 菜单与 ⚙ 设置：**就地操作，不用跳去设置页**（§4.6.8）。 */}
                  <button
                    type="button"
                    className="home__tileBtn"
                    aria-label={`${p.id} 的菜单`}
                    onClick={() => {
                      // "隐藏这张" —— **而它留在清单里**（规则 1）。
                      onCatalogueChange({
                        all: catalogue.all.map((x) =>
                          x.id === p.id ? { ...x, shown: false } : x,
                        ),
                      });
                    }}
                  >
                    ⋮
                  </button>
                  <button
                    type="button"
                    className="home__tileBtn"
                    aria-label={`${p.id} 的设置`}
                    onClick={() => {
                      // 切换丰富/极简（§4.6.8 的"切「丰富·极简」"）。
                      onCatalogueChange({
                        all: catalogue.all.map((x) =>
                          x.id === p.id
                            ? {
                                ...x,
                                shape:
                                  x.shape === PluginShape.Rich
                                    ? PluginShape.Minimal
                                    : PluginShape.Rich,
                              }
                            : x,
                        ),
                      });
                    }}
                  >
                    ⚙
                  </button>
                </div>
              </header>
              <div className="home__tileBody">{renderPlugin(p.id, p.shape)}</div>
            </article>
          ))}
        </div>
      </section>
    </div>
  );
}
