/**
 * 标题栏的**事件契约**（`docs/UI设计规格.md` §4.5.1）
 * ============================================================================
 *
 * ## 为什么这张表要活在**代码**里，而不是只写在规格里
 *
 * 规格里那张表是给人读的散文表格，而它有前例证明**不够**：`faa7fa1` 那个真 bug
 * 的形状是"**排除了一种事件，而漏了另一种**" —— 三个窗口控制排除了
 * `pointerdown`、而**漏了 `dblclick`**，于是双击「关闭」**先最大化再关闭**。
 *
 * 那种 bug 的根因不是粗心，而是**每个元素上的排除是手写的、逐行分散的**：
 * 新增一个元素时，你得自己记得给每一个事件族都写一行。所以这里把它换成
 * 一张表 + 一个派生函数（[`swallowProps`]）——
 * **元素不再"记得"排除什么，它只是把表里那一行兑现成属性。**
 *
 * ## 三个读者（都在 §4.5.1 的表下面写着）
 *
 * | 读者 | 它核什么 |
 * |---|---|
 * | `tools/check-titlebar-contract.ps1` | 本文件 ↔ 规格表**逐格一致**（改一边不改另一边 ⇒ 红） |
 * | `web/src/titlebar/contract.test.ts` | 本表 ↔ **真实 DOM 行为**（逐元素逐事件族派发） |
 * | `web/src/titlebar/TitleBar.tsx` | 根部那道闸（`swallowed()`）**由本表派生**；元素只带 `data-titlebar-item` |
 *
 * ⚠️ 而 `TITLEBAR_ITEMS` 的**书写格式是硬约束**：`tools/check-titlebar-contract.ps1`
 * 按行解析它 —— 一行一个元素、字段顺序固定。重排换行会让那条检查报
 * "解析不到这一行"，而那是刻意的：它**宁可吵，也不要静默地少核一行**。
 */

/**
 * 标题栏**根部**真的处理的三个事件族。
 *
 * ⚠️ **这个数组是"表要有几列"的定义** —— 根部多处理一个事件族，
 * 这里就要多一项，而加完立刻会**编译不过**（见 [`EVERY_FAMILY_IS_COVERED`]）。
 */
export const EVENT_FAMILIES = ["pointerdown", "dblclick", "contextmenu"] as const;

/** `pointerdown` = 拖窗口 · `dblclick` = 最大化/还原 · `contextmenu` = 系统窗口菜单。 */
export type EventFamily = (typeof EVENT_FAMILIES)[number];

/**
 * 一格的期望。这四个符号与 §4.5.1 表格里的完全一一对应：
 *
 * | 这里 | 规格表 |
 * |---|---|
 * | `swallow` | `吞` |
 * | `bubble` | `冒` |
 * | `skip` | `不做` |
 * | `na` | `不适用` |
 */
export type Cell = "swallow" | "bubble" | "skip" | "na";

/**
 * | 这里 | 规格表 | 含义 |
 * |---|---|---|
 * | `done` | 已实现 | 在 `TitleBar` 里，逐格有行为测试 |
 * | `structural` | 结构性 | 靠 DOM 结构成立（岛不在标题栏子树里） |
 * | `pending` | 未实现 | 还没画出来 —— 而**它也因此不该出现在 DOM 里** |
 */
export type TitlebarItemStatus = "done" | "structural" | "pending";

/** 表里的一行。 */
export interface TitlebarItem {
  /** 同时是 DOM 上的 `data-titlebar-item`。 */
  readonly id: string;
  /** 规格表里的行名。 */
  readonly label: string;
  readonly status: TitlebarItemStatus;
  readonly cells: Readonly<Record<EventFamily, Cell>>;
}

/**
 * 🔴 **契约表本身 —— 一行一个元素。**
 *
 * ⚠️ 八行里的每一格都不是随手填的，`switch`（产品切换器）与 `island`（灵动岛）
 * 这两行尤其要看清：
 *
 * - **`switch` 是 `pending`**：§4.4 说标题栏上要有一个产品切换器，而它**还没画**。
 *   写进表里的意义是"它一旦被画出来，就必须按这三格排除" —— 而
 *   `contract.test.ts` 会断言 `pending` 的元素**此刻不在 DOM 里**
 *   （于是"悄悄加了半个元素"也会红）。
 * - **`island` 是 `structural`**：岛是 `position: fixed` 的一层
 *   （`IslandLayer`），**它不是标题栏的孩子** ⇒ `pointerdown` 根本不会冒泡到
 *   标题栏根部。所以它的三格是 `na`（不适用），而"两套拖动作用域分开"
 *   这件事由**结构**保证（§4.5.3 的灵动岛例外）。
 */
export const TITLEBAR_ITEMS = [
  { id: "blank", label: "标题栏空白处", status: "done", cells: { pointerdown: "bubble", dblclick: "bubble", contextmenu: "skip" } },
  { id: "brand", label: "品牌", status: "done", cells: { pointerdown: "swallow", dblclick: "swallow", contextmenu: "skip" } },
  { id: "search", label: "搜索框", status: "done", cells: { pointerdown: "swallow", dblclick: "swallow", contextmenu: "skip" } },
  { id: "switch", label: "产品切换器", status: "pending", cells: { pointerdown: "swallow", dblclick: "swallow", contextmenu: "skip" } },
  { id: "island", label: "灵动岛", status: "structural", cells: { pointerdown: "na", dblclick: "na", contextmenu: "na" } },
  { id: "min", label: "最小化", status: "done", cells: { pointerdown: "swallow", dblclick: "swallow", contextmenu: "skip" } },
  { id: "max", label: "最大化", status: "done", cells: { pointerdown: "swallow", dblclick: "swallow", contextmenu: "skip" } },
  { id: "close", label: "关闭", status: "done", cells: { pointerdown: "swallow", dblclick: "swallow", contextmenu: "skip" } },
] as const satisfies readonly TitlebarItem[];

/** 表里出现过的 id（而 `as const` 让它成为一个**字面量联合**，拼错是编译错误）。 */
export type TitlebarId = (typeof TITLEBAR_ITEMS)[number]["id"];

/**
 * ⚠️ **这一行是编译期的闩，不是装饰。**
 *
 * `Record<EventFamily, true>` 要求**每一个**事件族都被列出来 ⇒
 * 往 [`EVENT_FAMILIES`] 里加一项，这里立刻编译不过。
 *
 * 而它防的正是那个真 bug 的形状：**"漏了另一个事件族"**。
 * （导出是为了不让 lint 把它当成未使用的常量。）
 */
export const EVERY_FAMILY_IS_COVERED: Record<EventFamily, true> = {
  pointerdown: true,
  dblclick: true,
  contextmenu: true,
};

/** 按 id 取一行。取不到是**编程错误**（id 由类型约束过），所以它抛。 */
export function itemOf(id: TitlebarId): TitlebarItem {
  const found = TITLEBAR_ITEMS.find((item) => item.id === id);
  if (found === undefined) {
    throw new TypeError(`事件契约表里没有这个元素：${id}`);
  }
  return found;
}

/** 按 id 取一行；**不是表里的 id ⇒ `undefined`（不抛）** —— 这是给 DOM 反查用的那道闸。 */
export function itemById(id: string): TitlebarItem | undefined {
  return TITLEBAR_ITEMS.find((item) => item.id === id);
}

/**
 * 🔴 **"这一格该吞吗"** —— 标题栏根部那道闸问的就是这一句。
 *
 * ## ⚠️ 为什么判断在**根部**，而不是在元素上挂 `stopPropagation`
 *
 * 这是一个**实测**出来的事实：`disabled` 的搜索框**收不到 React 的合成
 * `onDoubleClick`**（React 对"实际禁用"的表单元素不派发鼠标类合成事件）。
 * 于是"把排除挂在那个按钮上"这一格**静默失效** —— 双击搜索框一路冒到根部，
 * 把窗口最大化了。而 `<header>` **从不被禁用**，所以判断放在那里，
 * 用 `data-titlebar-item` 反查事件落在哪个元素上。
 *
 * ⚠️ 而那个 bug 的形状与 `faa7fa1` 那次**一模一样**（"看着排除了，实际没有"），
 * 只是这次机制藏在 React 的事件系统里；`TitleBar.test.tsx` 当初只测了
 * 搜索框的 `pointerdown`，所以它一直绿着。抓到它的是 `contract.test.tsx`
 * 那张**逐格遍历**的测试。
 *
 * ⚠️ 表外的 id ⇒ `false`（不吞）：一个"多出来的元素"应当在测试里红，
 * 而不是在这里被静默吞掉。
 */
export function swallows(id: string, family: EventFamily): boolean {
  return itemById(id)?.cells[family] === "swallow";
}
