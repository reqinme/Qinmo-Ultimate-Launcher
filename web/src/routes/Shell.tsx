import { useState, type ReactElement } from "react";
import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import {
  PRIMARY,
  PRODUCT_KEYS,
  TOOL_KEYS,
  primaryOf,
  secondaryOf,
  type PrimaryKey,
  type SecondaryArea,
  type SecondaryItem,
  type SecondarySection,
} from "./nav.ts";
import { Island, IslandLayer, idleContent } from "../island/Island.tsx";
import { useIsland } from "../island/IslandProvider.tsx";
import { TitleBar } from "../titlebar/TitleBar.tsx";
import { IslandView } from "../island/islandMath.ts";
import { RailIcon } from "../icons/rail.tsx";
import { APP_VERSION } from "../app/version.ts";
import {
  ProductProvider,
  PRODUCT_FIXTURE,
  activeProductOf,
  instanceCountText,
  useProduct,
  type ProductRef,
} from "../product/product.tsx";
import { RailPrefProvider, useRailPref } from "../shell/railPref.tsx";
import "./Shell.css";

/**
 * 外壳：**四段侧栏 + 状态条**（M4 门禁第 ③ 项）
 * ============================================================================
 *
 * 结构（`docs/UI设计规格.md` §4.1 / §4.6.1 / §4.6.1.1 / §4.6.2）：
 *
 * ```text
 *  ┌──────────────────────────────────────────────────────────────────────┐
 *  │ 标题栏 36 px（品牌 · 搜索 · 灵动岛 · 窗口控制）                        │
 *  ├──────────────┬───────────────────┬──────────────────────────────────┤
 *  │ 账户卡        │ 二级栏            │                                  │
 *  │ 产品段        │ （**内容由一级     │        主区                       │
 *  │ 主页          │   决定**，§4.6.1.1）│                                  │
 *  │ 实例          │                   │                                  │
 *  │ 下载          │                   │                                  │
 *  │ （有意留白）   │                   │                                  │
 *  │ 账号管理      │                   │                                  │
 *  │ 百宝箱        │                   │                                  │
 *  │ 设置          │                   │                                  │
 *  │ 关于          │                   │                                  │
 *  │ v0.0.0        │                   │                                  │
 *  ├──────────────┴───────────────────┴──────────────────────────────────┤
 *  │ 状态条 26 px（常驻四项：版本 · 当前产品 · 源 · 网速）                  │
 *  └──────────────────────────────────────────────────────────────────────┘
 * ```
 *
 * ## 🔴 四条实现纪律
 *
 * 1. **一级高亮与二级栏内容都只从路由路径推出来** —— 没有"当前选中项"的
 *    组件状态。两个真相来源会立刻不同步（例如浏览器后退之后）。
 * 2. **一级没有下级时，二级栏整条不存在**（§4.6.1.1）—— 见 `secondaryOf`。
 * 3. **界面不认识具体游戏**（门禁⑤ 的 lint 规则）—— 这里的每一项都是
 *    固定的能力名，而不是 `if (product === 'java')`。
 * 4. **不知道的数不许编**（§4.6.1.3 / §7.2 的灵动岛同一条口径）——
 *    没有来源的格子写 `—`，而不是写 `0`。
 *
 * ## ⚠️ 两份全局状态挂在这里，而不是 `main.tsx`
 *
 * 「当前产品」（§4.6.1.2）与「侧栏宽度」（§4.6.2）都是**跨页**的状态，
 * 所以它们必须是 Provider。而它们挂在**外壳自己这一层**，因为三处消费者
 * （侧栏产品段 · 状态条 · 主页横幅）**全都在外壳的子树里**
 *（主页在 `Outlet` 下）—— 挂在 `main.tsx` 那一层也行，但那样每一个
 * "只渲染外壳"的测试都要额外补两层与外壳无关的 Provider。
 *
 * ⚠️ 而**灵动岛的 Provider 仍在 `main.tsx`**（`web/src/island/IslandProvider.tsx`）：
 * 它的状态由**页面**发起（主页那个「启动」按钮），而且它渲染在整个网格之外。
 */
export interface ShellProps {
  /**
   * 产品表。
   *
   * 🔴 **今天默认是夹具**（`PRODUCT_FIXTURE`）—— 真产品表要由 Rust 侧的
   * 能力描述符给，而 IPC 里现在没有那条命令（见 `web/src/product/product.tsx`）。
   * 留成参数是为了让"接真数据"这件事有一个**明确的落点**：
   * 到那时只换这里的默认值，而三处消费者一行都不用动。
   */
  readonly products?: readonly ProductRef[];
}

export function Shell({ products = PRODUCT_FIXTURE }: ShellProps = {}): ReactElement {
  return (
    <RailPrefProvider>
      <ProductProvider rows={products}>
        <Frame />
      </ProductProvider>
    </RailPrefProvider>
  );
}

/**
 * 网格本身（**两个 Provider 的消费者**）。
 *
 * ⚠️ 它单独是一个组件，因为 `data-rail` 那几个属性要读 Provider 里的值 ——
 * 而"读"必须发生在 Provider **之内**。Provider 不渲染 DOM，所以这里
 * 与"直接把那两层写在 `Shell` 里"是同一棵 DOM。
 */
function Frame(): ReactElement {
  // ⚠️ **从路由本身推出"当前一级"**，而不是从事件处理器里 setState。
  //
  // 理由：浏览器后退 / 直接输入 URL / 将来从托盘跳转 —— 这些入口
  // 都不会经过"点击侧栏"那条路。一个用 state 记当前项的实现
  // 在那些入口下会**高亮错的那一项**。
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const rail = useRailPref();
  // ⚠️ **收起是一个本地 UI 状态**（它是用户的选择），不是队列状态 ——
  // 所以由外壳持有，而内核的队列一个字节都不受影响（§7.3 约束 4）。
  //
  // ⚠️ 而它用**联合类型**而不是布尔：`eslint.config.js` 的第三条纪律
  // （"禁止 toggle 式布尔状态"）要的就是这个形状 —— 见 `web/src/shell/railPref.tsx`
  // 里那段"为什么不是 `isWide`"。**而那条规则今天是死的**（它的选择器
  // 匹配的是字符串字面量，而 `useState(false)` 的 `value` 是布尔）——
  // 那件事记在 `SESSION.md` 的"环境事实"一节里，不在这一轮的范围里。
  const [islandView, setIslandView] = useState<"compact" | "collapsed">("compact");
  // 🔴 **状态来自那一层全局接线，而不是这里的夹具。**
  //
  // ⚠️ 而 `idleContent` **仍然有用**：它在"还没开始"时给出
  // "源 + 账户"那两个**真实信息**（而那正是 §7.2 的 `Idle` 该有的样子 ——
  // "极小胶囊：源 + 账户"）。一个用一句固定的"就绪"代替它的实现
  // 会让那两条信息**永远消失**。
  const island = useIsland();
  const islandContent =
    island.content.kind === "idle"
      ? idleContent(FIXTURE_ACCOUNT, FIXTURE_SOURCE)
      : island.content;
  const current = primaryFromPath(pathname);
  const secondary = secondaryOf(current);

  return (
    // ⚠️ `has-titlebar` 这个类让网格多出标题栏那一行、`data-rail` 让两态
    // 由 CSS 变量决定 —— 而它们**写在属性上而不是内联样式上**，于是
    // "这个网格有几行、侧栏现在是哪一态"在 CSS 里读得出来。
    //
    // ⚠️ 而这一条说明**必须是 `//` 而不是 `{/* */}`**：后者是一个 JSX
    // 表达式，于是 `return (` 后面就有两个根节点 —— 那是语法错误
    //（而报错信息指向 `)`，指不到这一行）。
    <div className="shell shell--has-titlebar" data-rail={rail.railWidth}>
      {/* 🔴 自绘标题栏（§4.5）。它提供系统标题栏关掉之后失去的三样：
          拖动区、最小化/最大化/关闭、以及"这是哪个应用"。 */}
      <div className="shell__titlebar">
        <TitleBar />
      </div>
      {/*
        🔴 **侧栏四段**（§4.6.1）：账户卡 → 产品段 → 导航段 → 留白 + 下段。

        ⚠️ 而"留白"那一段是**一个 `flex: 1` 的占位符**，不是"没写东西"：
        它的职责写在 §4.6.1 的职责表里 —— **预留给产品列表的增长**。
        一个把实例列表搬进来填满它的实现**已经被否决过**
        （"与导航争夺同一件事"）。
      */}
      <nav className="shell__rail" aria-label="主导航">
        {/* 账户卡：**不属于导航**，所以它不在 PRIMARY 里。
            ⚠️ 与灵动岛的 `idleContent` 拿的是同一个夹具（`FIXTURE_ACCOUNT`）——
            等账户域接上真内核时，这两处会一起换掉。 */}
        <div className="shell__account">
          <span className="shell__accountBadge" aria-hidden="true" />
          <span className="shell__accountName">未登录</span>
        </div>

        {/* 产品段：**唯一允许增长的一段**（§4.6.1 纪律 ④） */}
        <ProductSegment />

        <ul className="shell__group">
          {PRODUCT_KEYS.map((k) => (
            <RailItem key={k} itemKey={k} current={current} />
          ))}
        </ul>

        <div className="shell__spacer" />

        <ul className="shell__group">
          {TOOL_KEYS.map((k) => (
            <RailItem key={k} itemKey={k} current={current} />
          ))}
        </ul>

        {/* 版本号：§4.6.1 的 ASCII 里它在下段的最末一行。
            ⚠️ 值是 `package.json` / `tauri.conf.json` 的真值（`0.0.0`），
            而**不是**规格稿里示意的 `v0.1.0` —— 见 `web/src/app/version.ts`。 */}
        <div className="shell__version">{`v${APP_VERSION}`}</div>
      </nav>

      {/*
        ⚠️ **`null` ⇒ 整条二级栏不存在。** 见 `nav.ts` 里
        "`null` 与 `[]` 的区别"那一段 —— 渲染一个空的二级栏会让
        "主页"看起来像"一个有二级栏但里面没东西的页"。
      */}
      {secondary === null ? null : <SecondaryRail current={current} area={secondary} />}

      <main className="shell__main">
        <Outlet />
      </main>

      <StatusBar />
      {/*
        ⚠️ **灵动岛挂在网格之外**（`position: fixed`）—— 它不是三段布局的一员。
        放进网格会挤出一个格子，而它是"浮在最上层"的（§5.3 使用纪律 2）。

        而它在**外壳里**而不是某一页里，正因为 §7.1 的定位：
        "把此刻唯一值得看的事**提到顶层**" —— 它的作用域是整个应用。
      */}
      <IslandLayer>
        <Island
          content={islandContent}
          onCollapse={() => setIslandView("collapsed")}
          view={islandView === "collapsed" ? IslandView.Collapsed : IslandView.Compact}
        />
      </IslandLayer>
    </div>
  );
}

/**
 * 那两个**夹具**（真来源还没有 —— 见各处的注释）。
 *
 * ⚠️ 它们是**常量而不是散在 JSX 里的字符串**，因为同一个账户名出现在
 * 侧栏账户卡与灵动岛两处，而"两处写法不同"是一个迟早会发生的错。
 */
const FIXTURE_ACCOUNT = "离线账户";
const FIXTURE_SOURCE = "官方源";

/**
 * 没有来源的那一格写什么。
 *
 * 🔴 **它是 `—` 而不是 `0`，也不是"未知"两个字** —— §4.6.1.3 与灵动岛
 * 那条口径（`speed_bps === null` 不许显示 `0 B/s`）是同一条：
 * 把"不知道"渲染成 `0` 是**编数**，而 `—` 在界面上是一个**中性的缺项记号**。
 */
const UNKNOWN = "—";

/**
 * 状态条的一项（§4.1）。
 *
 * ⚠️ **常驻四项 / 其余进详情面板**这件事写在这张表里，而不是写在 JSX 的
 * 两个数组字面量里 —— 因为 `tools/check-shell-contract.ps1` 要拿它与
 * 规格 §4.1 那张逐格核对（"点开后进详情面板"是 v1.3 的修正：
 * 此前写"≤4 项"而示例画了 6 项，自相矛盾）。
 */
interface StatusItemSpec {
  readonly key: string;
  /** **已经本地化的**名字。 */
  readonly label: string;
  /** `true` = 常驻那一行；`false` = 点「更多」之后才出现。 */
  readonly resident: boolean;
}

const STATUS_ITEMS: readonly StatusItemSpec[] = [
  { key: "version", label: "版本", resident: true },
  { key: "product", label: "当前产品", resident: true },
  { key: "source", label: "源", resident: true },
  { key: "speed", label: "网速", resident: true },
  // ⚠️ 这一项的**名字是「运行时」而不是规格 §4.1 里写的「Java 版本」**：
  // 导航那一处（`nav.ts` 的 `runtime`）已经把"Java 与运行时"改成了语言中立的
  // "运行时"，理由是同一条（界面不许出现产品名，§4.6.4 纪律 1）。
  // 两处该同名，所以规格那一行在 v2.4 里跟着改了。
  { key: "runtime", label: "运行时", resident: false },
  { key: "memory", label: "内存占用", resident: false },
];

/** 侧栏的一项。 */
function RailItem({
  itemKey,
  current,
}: {
  readonly itemKey: PrimaryKey;
  readonly current: PrimaryKey;
}): ReactElement {
  const item = primaryOf(itemKey);
  const on = itemKey === current;
  // 🔴 §4.6.2 ④ + §7.5 G8：**窄栏里文字是 `display: none`**，
  // 于是它从**可访问树里也消失了** —— 一个只有图形的链接会让读屏
  // 用户听到一排"link"。所以名字必须由 `aria-label` 兜住，
  // 而它在两态下**是同一个名字**（测试钉着这件事）。
  //
  // ⚠️ 规格 §4.6.2 的原话是"窄栏下文字是 `display:none` 而不是缺失，
  // 屏幕阅读器仍要能读到" —— 那句话的**机制**就是下面这一行：
  // `display:none` 本身读不到，能读到的是 `aria-label`。
  return (
    <li>
      <Link
        to={hrefOf(itemKey)}
        className={on ? "shell__railItem shell__railItem--on" : "shell__railItem"}
        aria-current={on ? "page" : undefined}
        aria-label={item.label}
        // 窄栏悬停/聚焦时浮出的那个名字（§4.6.2 ①）。值放在属性里，
        // CSS 用 `attr()` 取 —— 于是"气泡里写的是什么"在源码里读得出来。
        data-tip={item.label}
      >
        <span className="shell__railIcon" aria-hidden="true">
          {/* ⚠️ 图标名与一级 key **同名同序**（`RAIL_ICON_NAMES`）——
              少一个就是编译错，而不是一个空图标。 */}
          <RailIcon name={itemKey} />
        </span>
        <span className="shell__railLabel">{item.label}</span>
      </Link>
    </li>
  );
}

/**
 * 侧栏的产品段（§4.6.1 的第二段，**唯一允许增长的一段**）。
 *
 * 内容来自 §4.6.1.2 那份全局状态 —— 而**同一个状态**同时驱动状态条的
 *「当前产品」与主页横幅的胶囊。所以这里点一下，那两处会跟着变
 *（`Shell.test.tsx` 里有一条正是这么踩的）。
 */
function ProductSegment(): ReactElement {
  const product = useProduct();
  const active = activeProductOf(product);
  return (
    <div className="shell__segment">
      {/* ⚠️ 这一段没有可见的标题（规格的 ASCII 里就是一排产品行）——
          而一个 `aria-hidden` 的"产品"字样会让读屏用户也能分辨这一段是什么。 */}
      <div className="shell__segmentTitle" aria-hidden="true">
        产品
      </div>
      {product.rows.length === 0 ? (
        <p className="shell__segmentNote">还没有产品表。</p>
      ) : (
        <ul className="shell__group">
          {product.rows.map((row) => (
            <li key={row.id}>
              <ProductRowButton
                row={row}
                on={active !== null && row.id === active.id}
                onPick={() => product.setActiveId(row.id)}
                variant="rail"
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * 产品表里的一行（**两个地方用它**：侧栏的产品段、二级栏的「产品」区）。
 *
 * ## 🔴 而"实例数"是这一轮里唯一一个**会有数字**的地方
 *
 * `instanceCount` 是 `number | null`，而 `null` 是**未知** ——
 * `instanceCountText` 把两者分开（`null` ⇒ `undefined` ⇒ 一个字都不渲染）。
 * 一个把它渲染成 `0 个实例` 的实现是**编数**，而"0 个实例"与"不知道有几个"
 * 在屏幕上长得一样、意思完全相反。
 *
 * ## 而它就是 §4.6.2 ② 说的那个**角标**
 *
 * > 状态用右上角数字角标（如「下载 3」），**必须同时有 `aria-label`**
 *
 * 宽栏里它是行尾的一段小字，窄栏里同一个元素变成图标右上角的小圆点数字
 *（那是 CSS 的事，见 `Shell.css` 的 `[data-rail="narrow"]` 那一段）——
 * **一个数字，两处形态，不是两份数据**。
 */
function ProductRowButton({
  row,
  on,
  onPick,
  variant,
}: {
  readonly row: ProductRef;
  readonly on: boolean;
  readonly onPick: () => void;
  /**
   * 画在哪一层 —— 于是两个地方**只差类名，不差行为**。
   *
   * ⚠️ 这是一个联合类型而不是布尔，且**不靠"类名等于什么"来判断分支**：
   * 后者会让"我改一个类名"静默改成另一个形态。
   */
  readonly variant: "rail" | "secondary";
}): ReactElement {
  const count = instanceCountText(row.instanceCount);
  // 可访问名里**带上实例数**（"下载 3"那条要求的"同时有 aria-label"）。
  const name = count === undefined ? row.label : `${row.label} · ${count}`;
  const base = variant === "rail" ? "shell__railItem" : "shell__secondaryItem";
  return (
    <button
      type="button"
      className={on ? `${base} ${base}--on` : base}
      aria-pressed={on}
      aria-label={name}
      data-tip={name}
      onClick={onPick}
    >
      {variant === "rail" ? (
        <span className="shell__railIcon" aria-hidden="true">
          <span className={on ? "shell__productDot shell__productDot--on" : "shell__productDot"} />
        </span>
      ) : null}
      <span className={variant === "rail" ? "shell__railLabel" : "shell__secondaryLabel"}>{row.label}</span>
      {count === undefined ? null : (
        <span className={variant === "rail" ? "shell__railCount" : "shell__secondaryCount"} aria-hidden="true">
          {count}
        </span>
      )}
    </button>
  );
}

/**
 * 二级栏（§4.6.1.1）。
 *
 * 它的内容**只由当前一级决定** —— 而"区"这件事（产品 / 分组 / 装到 / 按产品）
 * 来自 `nav.ts`，其中两个区的**内容不在那里**：
 *
 * | `source` | 内容从哪来 | 今天 |
 * |---|---|---|
 * | `static` | `nav.ts` 的项表 | 六页可用 |
 * | `products` | §4.6.1.2 的全局状态 | 有状态源（夹具数据） |
 * | `instances` | 实例列表 | **没有那份数据** ⇒ 只给一句事实说明 |
 */
function SecondaryRail({
  current,
  area,
}: {
  readonly current: PrimaryKey;
  readonly area: SecondaryArea;
}): ReactElement {
  return (
    <nav className="shell__secondary" aria-label="二级导航">
      {area.sections.map((section) => (
        <section
          key={section.key}
          className="shell__secondarySection"
          aria-labelledby={`shell-section-${section.key}`}
        >
          {/* ⚠️ `h2` 而不是 `div`：二级栏的区在**大纲**里是主区标题的下级，
              而"能用标题跳转"是读屏用户唯一能快速越过这一段的手段。
              视觉上它是 12 px 的大写字距标题，不是大标题 —— 见 CSS。 */}
          <h2 className="shell__secondaryTitle" id={`shell-section-${section.key}`}>
            {section.label}
          </h2>
          {renderSection(section, area, current)}
        </section>
      ))}
    </nav>
  );
}

/** 一个区的三种渲染方式（**穷尽 `SectionSource` 的三支**）。 */
function renderSection(
  section: SecondarySection,
  area: SecondaryArea,
  current: PrimaryKey,
): ReactElement {
  switch (section.source) {
    case "static":
      return <StaticSection current={current} section={section} items={itemsOf(area, section)} />;
    case "products":
      return <ProductsSection />;
    case "instances":
      return <InstancesSection />;
  }
}

/** 某一个区里的项（**项自带 `section`**，所以这是一次过滤）。 */
function itemsOf(area: SecondaryArea, section: SecondarySection): readonly SecondaryItem[] {
  return area.items.filter((item) => item.section === section.key);
}

/** 「分组」「类型」「账户」「工具」「类别」这些**写死的**区。 */
function StaticSection({
  current,
  section,
  items,
}: {
  readonly current: PrimaryKey;
  readonly section: SecondarySection;
  readonly items: readonly SecondaryItem[];
}): ReactElement {
  return (
    <ul className="shell__secondaryList">
      {items.map((s) => (
        <li key={s.key}>
          <Link
            to={secondaryHref(current, section, s)}
            className="shell__secondaryItem"
            activeProps={{ className: "shell__secondaryItem shell__secondaryItem--on" }}
          >
            {s.label}
          </Link>
        </li>
      ))}
    </ul>
  );
}

/**
 * 「产品」区（实例页）与「按产品」区（设置页）—— **同一份全局状态**。
 *
 * ⚠️ 它与侧栏的产品段**是同一个控件、同一个状态**，只是形式上分成两处：
 * §4.6.1.2 的方案 A 要的就是"三处都能切、三处联动"，而"能切"这件事
 * 必须在**用户正在看那件事的那一页**上做得到 —— 在实例页里，
 * 用户看的是"这个产品下的实例"，所以产品列表也在这一页的二级栏里。
 */
function ProductsSection(): ReactElement {
  const product = useProduct();
  const active = activeProductOf(product);
  if (product.rows.length === 0) {
    return <p className="shell__secondaryNote">还没有产品表 —— 等内核把能力描述符给上来。</p>;
  }
  return (
    <ul className="shell__secondaryList">
      {product.rows.map((row) => (
        <li key={row.id}>
          <ProductRowButton
            row={row}
            on={active !== null && row.id === active.id}
            onPick={() => product.setActiveId(row.id)}
            variant="secondary"
          />
        </li>
      ))}
    </ul>
  );
}

/**
 * 「装到」区（下载页，§4.6.1.1 / §4.6.7）。
 *
 * 🔴 **今天这里一个假的行都不画**，因为"有哪些实例"这件事**没有数据源**：
 * `web/src/api/index.ts` 只有 `fetchCapabilities` / `fetchInstanceSummary`，
 * 没有"列出实例"那条命令（`InstanceSummary` 甚至只有 `id` 与 `name`）。
 *
 * ⚠️ 而"不画"不等于"不出现"：§4.2 的纪律是**不适用的项永不隐藏**
 *（"不适用 = 禁用态 + 一句原因"）。所以这一区在，而里面是一句事实说明。
 * 一个用 `instanceCount === 1` 编一个假实例名的实现会让这一页
 * **看起来已经能用**，而那正是 §4.6.1.3 要拦的东西。
 */
function InstancesSection(): ReactElement {
  return (
    <p className="shell__secondaryNote">
      还没有实例 —— 装好一个实例之后，这里才会出现「装到哪儿」。
    </p>
  );
}

/**
 * 状态条（§4.1：高 26 px，常驻四项）。
 *
 * ## 四项里只有两项今天有真值
 *
 * | 项 | 真值来自 | 今天 |
 * |---|---|---|
 * | 版本 | `web/src/app/version.ts` | ✅ `0.0.0`（与 `package.json` 逐字相同） |
 * | 当前产品 | §4.6.1.2 的全局状态 | ✅ 夹具数据 |
 * | 源 | 内核的源配置 | ⚠️ 夹具（`官方源`，与灵动岛的 `idleContent` 同源） |
 * | 网速 | 下载时的实时速率 | ❌ **没有来源** ⇒ `—` |
 *
 * 而"点开后进详情面板"那两项（运行时 / 内存占用）同样**没有来源**。
 * 规格 v1.3 专门修正过这一条：常驻**恰好四项**，其余进详情 ——
 * 因为此前写"≤4 项"而示例画了 6 项，自相矛盾。
 */
function StatusBar(): ReactElement {
  const product = useProduct();
  const active = activeProductOf(product);
  // ⚠️ 联合类型而不是布尔（同 `Frame` 里那条注释）。
  const [details, setDetails] = useState<"closed" | "open">("closed");
  const open = details === "open";
  const values: Readonly<Record<string, string>> = {
    version: `v${APP_VERSION}`,
    product: active === null ? UNKNOWN : active.label,
    source: FIXTURE_SOURCE,
    speed: UNKNOWN,
    runtime: UNKNOWN,
    memory: UNKNOWN,
  };
  return (
    <footer className="shell__status" aria-label="状态条">
      <ul className="shell__statusList">
        {STATUS_ITEMS.filter((i) => i.resident).map((i) => (
          <li key={i.key} className="shell__statusItem">
            <span className="shell__statusLabel">{i.label}</span>
            <span className="shell__statusValue">{values[i.key] ?? UNKNOWN}</span>
          </li>
        ))}
      </ul>
      <button
        type="button"
        className="shell__statusMore"
        aria-expanded={open}
        aria-controls="shell-status-details"
        onClick={() => setDetails(open ? "closed" : "open")}
      >
        {open ? "收起详情" : "更多"}
      </button>
      {open ? (
        <ul className="shell__statusList shell__statusList--details" id="shell-status-details">
          {STATUS_ITEMS.filter((i) => !i.resident).map((i) => (
            <li key={i.key} className="shell__statusItem">
              <span className="shell__statusLabel">{i.label}</span>
              <span className="shell__statusValue">{values[i.key] ?? UNKNOWN}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </footer>
  );
}

/**
 * 一级 → 它的默认落地路径。
 *
 * ⚠️ **它是显式的映射，不是字符串拼接。** 一个 `${key}` 拼出来的路径
 * 会让"改了一级的 key 却忘了改路径"变成运行时 404，而这里改 key
 * 会让 **TypeScript 报错**（`hrefOf` 的返回类型是路由的合法路径联合）。
 */
function hrefOf(k: PrimaryKey): string {
  switch (k) {
    case "home":
      return "/";
    case "instances":
      return "/instances";
    case "downloads":
      return "/downloads";
    case "accounts":
      return "/accounts";
    case "toolbox":
      return "/toolbox";
    case "settings":
      return "/settings";
    case "about":
      return "/about";
  }
}

/**
 * 二级项 → 路径。
 *
 * ⚠️ 这里**允许**字符串拼接，因为二级项多且规律（`<一级>/<二级键>`）。
 *
 * 而那个规律有**两类**例外：
 *
 * 1. **`home` 的二级是 `null`** ⇒ 它不会走到这里。
 * 2. **`section.rootItem` 那一项落在一级页本身** —— 「账户列表」不是
 *    `/accounts/list`，它就是 `/accounts`；「内建帮助」就是 `/toolbox`；
 *    「外观与材质」就是 `/settings`；「全部实例」就是 `/instances`。
 *    这四项此前各自指着一条不存在的路由（点下去是 `Not Found`）。
 *    见 `nav.ts` 里 `SecondarySection.rootItem` 与检查 G。
 */
function secondaryHref(
  primary: PrimaryKey,
  section: SecondarySection,
  item: SecondaryItem,
): string {
  const base = hrefOf(primary);
  if (section.rootItem === item.key) return base;
  const sep = base === "/" ? "" : "/";
  return `${base}${sep}${item.key}`;
}

/**
 * 从路径推出当前一级。
 *
 * ⚠️ **顺序敏感**：`/downloads/loaders` 必须匹配 `downloads` 而不是任何
 * 以 `d` 开头的其他项。所以这里用**显式的最长前缀**，而不是
 * `startsWith` 的遍历（那会在前缀互为前缀时出错）。
 */
export function primaryFromPath(pathname: string): PrimaryKey {
  // 段数最多的一级在最前 —— 目前都是一段，但**这个顺序不该靠"现在恰好如此"**。
  const byLength = [...PRIMARY].sort((a, b) => b.key.length - a.key.length);
  for (const p of byLength) {
    const href = hrefOf(p.key);
    if (href === "/") {
      // 根路径**只匹配恰好是 `/`** —— 否则它会吞掉所有路径。
      if (pathname === "/") return "home";
    } else if (pathname === href || pathname.startsWith(`${href}/`)) {
      return p.key;
    }
  }
  // 兜底：未知路径归到主页（而不是抛错）—— 一个 404 应当由路由自己处理，
  // 而导航高亮不该因此崩掉。
  return "home";
}
