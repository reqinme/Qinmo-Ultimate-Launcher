import { useState, type ReactElement } from "react";
import { HomePage, type HeroSummary } from "../home/HomePage.tsx";
import { PluginShape, PluginSpan, type PluginCatalogue } from "../home/plugins.ts";
import { useIsland } from "../island/IslandProvider.tsx";

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
  // 🔴 **那一个真的按钮。** 它推到 `Shell` 里渲染的那个岛
  //（见 `IslandProvider.tsx`：状态在路由之外，所以两层都看得见）。
  const island = useIsland();

  const hero: HeroSummary = {
    headline: island.run.busy ? "正在准备 26.3" : "还没有实例",
    detail: island.run.busy
      ? "进度在右上角的灵动岛上。"
      : "点「启动」会装好 26.3 并把它拉起来 —— 进度都在岛上。",
    actions: [
      // ⚠️ **同一个位置在忙与闲时做不同的事**（启动 / 停止），
      // 而不是并排放两个按钮。
      //
      // 理由是 §4.6.8 的规则 3：**同一件事一个入口**。
      // 而"停止"与"启动"是**同一件事的两个方向**（都是"让这次运行
      // 开始 / 结束"），所以它们共用一个位置。
      island.run.busy
        ? {
            label: "停止",
            primary: true,
            onClick: () => {
              void island.cancel();
            },
          }
        : {
            label: "启动 26.3",
            primary: true,
            onClick: () => {
              void island.start("26.3");
            },
          },
      { label: "打开数据目录" },
      { label: "导入整合包" },
    ],
  };

  return (
    <>
      <p className="page__note" data-fixture="true">
        ⚠️ 横幅文案与插件正文是<strong>夹具</strong>；而<strong>结构、三条硬规则、
        一键预设、拖动与就地操作</strong>现在就是真的。
        {/* ⚠️ **而这一条说的是"哪个部分是真的"** —— 一个"整页都是夹具"
            的提示会让那一个真的按钮也被当成摆设。 */}
        <br />
        ✅ <strong>「{island.run.busy ? "停止" : "启动 26.3"}」是真的</strong>
        ：它会调内核走完五阶段，而进度显示在右上角的灵动岛上。
      </p>
      {/* 🔴 **失败要说出来。**
          ⚠️ 而它**不一定**在岛上：失败发生在**前端**时（例如在浏览器里
          没有后端），后端**什么都没发**，于是这里那一句是唯一的信息。
          —— 那正是 `useIslandQueue` 里"失败时只设 `run.error`"那条注释说的。 */}
      {island.run.error !== null && (
        <p className="page__error" role="alert">
          启动没能进行：{island.run.error}
        </p>
      )}
      {island.run.last !== null && (
        <p className="page__note">
          ✅ 上一步完成：需要 {island.run.last.needed} 个文件，
          本机已有 {island.run.last.present} 个，
          这次下了 {island.run.last.downloaded} 个，
          其中 {island.run.last.migrated} 个是从已有安装迁移的。
        </p>
      )}
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
 * 插件清单 —— **§4.6.8 的七件齐了**。
 *
 * | 插件 | 占宽 | 为什么是这个档 |
 * |---|---|---|
 * | 最近运行 | 1/2（`span 6`） | 它有四张卡要横排（规格的表） |
 * | 快速启动 | 1/4（`span 3`） | 小件 |
 * | 下载队列 | 1/4（`span 3`） | 小件 |
 * | 游玩统计 | 1/4（`span 3`） | 小件（**当前未显示**） |
 * | 实例体检 | 1/3（`span 4`） | **默认档**（它要写"为什么这个实例不能用"） |
 * | 我的分组 | 1/3（`span 4`） | 默认档 |
 * | 产品状态 | 1/4（`span 3`） | 规格的表把它归在"小件"那一列 |
 *
 * ⚠️ **注意那两块 `shown: false`** —— 它们刻意留着不显示，
 * 而它们是规则 1 的活证据：**它们仍然在这份清单里**，所以
 * `⚙插件设置` 列得出它们、`＋添加插件` 也放得回去。
 * 一个"关掉就从数组里删掉"的夹具会让这一页看起来是对的，
 * 而把那个最该防的错法演示成正确做法。
 */
const FIXTURE_CATALOGUE: PluginCatalogue = {
  all: [
    { id: "最近运行", shown: true, span: PluginSpan.Half, shape: PluginShape.Rich },
    { id: "快速启动", shown: true, span: PluginSpan.Quarter, shape: PluginShape.Rich },
    { id: "下载队列", shown: true, span: PluginSpan.Quarter, shape: PluginShape.Rich },
    { id: "游玩统计", shown: false, span: PluginSpan.Quarter, shape: PluginShape.Rich },
    { id: "实例体检", shown: false, span: PluginSpan.Third, shape: PluginShape.Rich },
    { id: "我的分组", shown: true, span: PluginSpan.Third, shape: PluginShape.Rich },
    { id: "产品状态", shown: true, span: PluginSpan.Quarter, shape: PluginShape.Rich },
  ],
};
