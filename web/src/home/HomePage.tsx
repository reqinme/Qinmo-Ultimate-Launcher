import { useState, type ReactElement, type ReactNode } from "react";
import { Button, Dialog, Switch } from "../components/index.tsx";
import { coverToneAttr } from "./cover.ts";
import {
  PluginShape,
  PluginSpan,
  Preset,
  applyPreset,
  bringPluginToFront,
  movePlugin,
  setPluginShape,
  setPluginShown,
  settingsPlugins,
  spanForDropWidth,
  visiblePlugins,
  type PluginCatalogue,
  type PluginSetting,
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
 *
 * ## 而"画面"这一层是**自绘的，且颜色稳定**
 *
 * §4.6.1 的「封面图的纪律」是**红线级**的：*"横幅与卡片里的封面图，
 * 一律自绘或用户自有；禁止使用 Mojang / Microsoft 的官方素材"*。
 * 三档做法里它选**纯抽象自绘**（几何图形 + 渐变）与**中性占位**：
 *
 * - 有实例 ⇒ `hero.coverId` 给实例 id，[`coverToneAttr`] 把它换成
 *   `data-cover-tone="0".."11"`（**稳定、不随机** —— 同输入同输出）；
 * - 没有实例 ⇒ `data-cover-tone="none"`，**并且显式标注"占位"**。
 *
 * ⚠️ **颜色一个字节都不写在这里**：`HomePage.css` 按
 * `[data-cover-tone="N"]` 选档。一个在这里拼 `hsl(...)` 的实现会被
 * `eslint.config.js` 的字面颜色规则直接踩红。
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
  /**
   * 当前实例的 id（**横幅配色的种子** · §4.6.1）。
   *
   * ⚠️ **不给就是"还没有实例"** —— 那时横幅走"中性占位 + 明确标注占位"
   * 那一档，而不是随便挑一个颜色糊上去（§4.6.1：随机配色会让用户
   * 无法靠颜色认实例，而"没有实例"确实是一种要如实说出来的状态）。
   */
  readonly coverId?: string;
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

  /**
   * 设置面的**一份实现**（`＋添加插件` 与 `⚙插件设置` 共用）。
   *
   * ⚠️ 两个入口的差别**只有喂进来的那一份数据**：
   *
   * | 入口 | 喂什么 | 它回答的问题 |
   * |---|---|---|
   * | `⚙插件设置` | `settingsPlugins(catalogue)`（**含未显示的**） | "我关掉的那个去哪了" |
   * | `＋添加插件` | 上一份里 `shown === false` 的那些 | "还有什么没放上来" |
   *
   * 一个"两处各写一遍"的实现会让**规则 1 在其中一个入口悄悄失效** ——
   * 而"关掉就再也找不回来"正是 §4.6.8 点名要防的错法。
   */
  const renderSettings = (items: readonly PluginSetting[], empty: string): ReactNode => (
    <div className="home__settings">
      {items.length === 0 ? (
        // §4.6.8：「空态不占半屏」—— 对话框里也就一行字。
        <p className="home__settingsEmpty">{empty}</p>
      ) : (
        items.map((p) => (
          <div className="home__settingsRow" key={p.id} data-plugin={p.id}>
            <span className="home__settingsName">{p.id}</span>
            {/*
              ⚠️ 开关的可访问名里**带上 id** —— 一行一个开关，
              而"在主页显示"这四个字对读屏来说九个开关长得一模一样。
            */}
            <Switch
              label={`${p.id} 在主页显示`}
              checked={p.shown}
              onCheckedChange={(v) => onCatalogueChange(setPluginShown(catalogue, p.id, v))}
            />
            <Button
              tone="secondary"
              onClick={() =>
                onCatalogueChange(
                  setPluginShape(
                    catalogue,
                    p.id,
                    // 插件级只有两档（"精简"是预设级的）—— 所以这里是二选一。
                    p.shape === PluginShape.Rich ? PluginShape.Minimal : PluginShape.Rich,
                  ),
                )
              }
            >
              {p.shape === PluginShape.Rich ? "丰富" : "极简"}
            </Button>
            <Button
              tone="secondary"
              onClick={() => onCatalogueChange(bringPluginToFront(catalogue, p.id))}
            >
              移到最前
            </Button>
          </div>
        ))
      )}
    </div>
  );

  return (
    <div className="home">
      {/*
        🔴 **横幅固定**（它不在这块滚动区里）。
        一个把横幅放进滚动区的实现会在插件一多时把它滚走 ——
        而 §4.6.8 说那"会失去'它是主角'的意思"。
      */}
      <section
        className="home__hero"
        aria-label="启动横幅"
        data-cover-tone={coverToneAttr(hero.coverId)}
      >
        {/*
          画面层：**纯自绘**（几何 + 渐变），一个官方素材都没有。
          它是 `position: absolute` 的，所以不占版面 —— 横幅的排版
          仍然由下面那几个孩子决定。
        */}
        <div className="home__cover" aria-hidden="true" />
        {/*
          ⚠️ **没有实例时显式标注"占位"**（§4.6.1 的三档做法里
          最后一档要求"中性渐变 + 明确标注占位"）。一个不标注的实现
          会让人以为那是某个真实实例的画面。
        */}
        {hero.coverId === undefined ? (
          <span className="home__coverNote">封面图为占位 · 真实产品需自绘或用户自有</span>
        ) : null}
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

        {/*
          🔴 §4.6.8 原文：*"插件头部（＋添加 / ⚙设置）**固定在滚动区之外** ——
          否则插件一多，这两个按钮就被滚走了"*。所以它们在 `<header>` 里，
          也就是在 `.home__gridWrap`（唯一会滚的那块）之外。

          ⚠️ 两个对话框都是**不受控**的（不给 `open`）—— Radix 自己管开合，
          而"用 `useState(false)` 记一个开关"正是 `eslint.config.js`
          禁掉的那种字面布尔状态。
        */}
        <div className="home__pluginTools">
          <Dialog
            title="添加插件"
            description="这里只列当前没显示的那些 —— 放回去之后它立刻出现在主页上。"
            trigger={<Button tone="secondary">＋ 添加插件</Button>}
          >
            {renderSettings(
              settingsPlugins(catalogue).filter((p) => !p.shown),
              "所有插件都已经在主页上了。",
            )}
          </Dialog>
          <Dialog
            title="插件设置"
            description="含当前没显示的那些 —— 关掉一个不等于它消失了（「关掉就再也找不回来」是这一页最容易犯的错）。"
            trigger={<Button tone="secondary">⚙ 插件设置</Button>}
          >
            {renderSettings(settingsPlugins(catalogue), "清单是空的。")}
          </Dialog>
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
