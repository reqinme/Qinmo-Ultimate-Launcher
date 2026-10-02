import { useState, type ReactElement } from "react";
import { HomePage, type HeroSummary } from "../home/HomePage.tsx";
import { PluginShape, PluginSpan, type PluginCatalogue } from "../home/plugins.ts";

/**
 * 主页（**M4 主干** · §4.6.8）
 * ============================================================================
 *
 * ## ⚠️ 清单与正文都是**夹具**，而结构与规则是真的
 *
 * 真实数据要等内核把"当前实例 / 下载队列 / 游玩统计"接上来。
 * 而这个页面上**已经是真的**那部分：
 *
 * - 三条硬规则（开关 != 找回、插件不是导航层级、同一件事一个入口）
 * - 横幅固定 / 插件头部固定 / 只有网格滚
 * - 一键预设、拖动改顺序与占宽、⋮ 与 ⚙ 的就地操作
 *
 * ## 🔴 而产品列表**来自能力描述符的形状**
 *
 * §4.6.1.2 要求主页可选产品，而 §1.4 说界面不认识具体游戏。
 * 所以这里的产品是**数据**（`ProductChip[]`），而 `HomePage` 里
 * 没有一处 `if (product === …)` —— 接上后端时只换这两个数组。
 */
export function HomeRoute(): ReactElement {
  const [catalogue, setCatalogue] = useState<PluginCatalogue>(FIXTURE_CATALOGUE);
  const [product, setProduct] = useState("a");

  const hero: HeroSummary = {
    headline: "还没有实例",
    detail: "建一个实例之后，这里会显示它的状态与主操作。",
    actions: [
      { label: "新建实例", primary: true },
      { label: "打开数据目录" },
      { label: "导入整合包" },
    ],
  };

  return (
    <>
      <p className="page__note" data-fixture="true">
        ⚠️ 横幅文案与插件正文是<strong>夹具</strong>；而<strong>结构、三条硬规则、
        一键预设、拖动与就地操作</strong>现在就是真的。
      </p>
      <HomePage
        products={PRODUCTS}
        activeProduct={product}
        onProductChange={setProduct}
        hero={hero}
        catalogue={catalogue}
        onCatalogueChange={setCatalogue}
        renderPlugin={(id, shape) => (
          // ⚠️ **正文由调用方给** —— `HomePage` 不认识任何插件。
          // 于是"加一个插件"不需要改 `HomePage` 一个字节。
          <p className="page__stub">
            {shape === PluginShape.Minimal
              ? `${id}（极简）`
              : `${id} 的正文 —— 等内核把这一项的数据接上来。`}
          </p>
        )}
      />
    </>
  );
}

/** 两个产品（**形状来自能力描述符**，内容现在是占位）。 */
const PRODUCTS = [
  { id: "a", label: "原生版" },
  { id: "b", label: "另一形态" },
];

/**
 * 插件清单。
 *
 * ⚠️ **注意最后一块 `shown: false`** —— 它刻意留着不显示，
 * 而它是规则 1 的活证据：**它仍然在这份清单里**，所以设置页列得出它。
 */
const FIXTURE_CATALOGUE: PluginCatalogue = {
  all: [
    { id: "最近运行", shown: true, span: PluginSpan.Half, shape: PluginShape.Rich },
    { id: "快速启动", shown: true, span: PluginSpan.Quarter, shape: PluginShape.Rich },
    { id: "下载队列", shown: true, span: PluginSpan.Quarter, shape: PluginShape.Rich },
    { id: "游玩统计", shown: false, span: PluginSpan.Quarter, shape: PluginShape.Rich },
    { id: "实例体检", shown: false, span: PluginSpan.Third, shape: PluginShape.Rich },
  ],
};
