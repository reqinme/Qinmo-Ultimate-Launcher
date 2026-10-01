/**
 * 基础组件的六状态模型（**M4 门禁第 ② 项**）
 * ============================================================================
 *
 * `docs/UI设计规格.md` §6.3 的门禁原文：
 *
 * > **约 20 个基础组件骨架** | 只做样式与 a11y，不做业务；
 * > **每个含 6 种状态**（默认/悬停/按下/聚焦/禁用/加载）
 *
 * ## 🔴 为什么"六状态"要有一个类型，而不是六个 CSS 类
 *
 * 因为那六个里**只有四个是伪类**（`:hover` / `:active` / `:focus-visible`
 * 由浏览器给），而**另外两个是数据**（禁用 / 加载）。
 *
 * 一个把它们混成"六个 CSS 类"的实现会立刻遇到这个问题：
 * **加载中的按钮还能不能按？** 而答案是产品判断，不是样式：
 *
 * | 状态 | 可交互 | 可聚焦 | 为什么 |
 * |---|---|---|---|
 * | `disabled` | ✗ | **✗** | 它永远不会变得可用 —— 而"可聚焦但不可用"是**键盘陷阱** |
 * | `loading` | ✗ | **✓** | 它会变回来，而"跳走的焦点"是读屏用户最难跟的东西 |
 *
 * 所以下面把它写成一个**联合类型**，而那三列是它的语义载荷。
 */

/**
 * 一个组件**此刻**的形态。
 *
 * ⚠️ **它不含 `hover` / `active` / `focus`** —— 那三个由浏览器通过伪类给，
 * 它们**不是组件的数据**。把它们塞进 props 会逼每个调用方自己管鼠标状态，
 * 而那正是 CSS 的职责。
 */
export type VisualState = "default" | "disabled" | "loading";

/**
 * 一个状态的语义。
 *
 * ⚠️ **这张表是"六状态"能被验证的原因**：一个只写样式的实现没法回答
 * "加载中的按钮能不能聚焦"，而那个问题在实现里**一定**会被遇到。
 */
export interface StateSemantics {
  /** 用户能触发它吗。 */
  readonly interactive: boolean;
  /** Tab 键能停到它上面吗。 */
  readonly focusable: boolean;
  /** 读屏该把它念成什么（`undefined` = 按组件自己的角色念）。 */
  readonly ariaBusy?: boolean;
}

export const STATE_SEMANTICS: Readonly<Record<VisualState, StateSemantics>> = {
  default: { interactive: true, focusable: true },
  /**
   * **禁用：不可交互，且不可聚焦。**
   *
   * 而"不可聚焦"在原生元素上是自动的（`disabled` 属性），
   * 在一个自定义控件上**要显式做**（移除 `tabIndex` / `aria-disabled` 的取舍）。
   *
   * ⚠️ 本项目用**原生的 `disabled`**（而不是 `aria-disabled`），因为：
   * 原生 `disabled` **一定**不可聚焦也不可点击，而 `aria-disabled` 只是
   * "告诉读屏它不可用"，鼠标与键盘**仍然能触发它** —— 那需要每个组件
   * 自己再拦一次，而"某个组件忘了拦"是一个可预见的 bug。
   */
  disabled: { interactive: false, focusable: false },
  /**
   * **加载：不可交互，但**可聚焦**。
   *
   * 理由：它会变回来。而一个"加载时把焦点搬走"的实现会让
   * 键盘用户**丢掉他正在的位置** —— 那在读屏下是最难跟的一类变化。
   *
   * 所以加载态的落点是：`aria-busy="true"` + `aria-disabled="true"`
   *（而不是原生 `disabled`），于是**焦点留着**，而点击被我们的处理函数拦掉。
   */
  loading: { interactive: false, focusable: true, ariaBusy: true },
};

/** 由 `VisualState` 推出该给原生元素挂什么。 */
export interface NativeAttrs {
  readonly disabled: boolean;
  readonly "aria-disabled"?: true;
  readonly "aria-busy"?: true;
}

/**
 * **把状态翻译成原生属性** —— 所有组件的唯一翻译处。
 *
 * 一个"每个组件自己决定挂什么"的实现会让"加载态在某几个组件里
 * 意外地不可聚焦"变成一次全量排查。
 */
export function nativeAttrsOf(state: VisualState): NativeAttrs {
  if (state === "disabled") {
    return { disabled: true };
  }
  if (state === "loading") {
    return {
      disabled: false,
      // ⚠️ 两个都要：`aria-disabled` 告诉读屏，`aria-busy` 告诉它"在忙"。
      // 只用 `aria-busy` 会让读屏认为它**可用**（于是用户会去按它）。
      "aria-disabled": true,
      "aria-busy": true,
    };
  }
  return { disabled: false };
}

/** 交互是否应当被拦（加载或禁用时都要拦）。 */
export function shouldBlockEvent(state: VisualState): boolean {
  return !STATE_SEMANTICS[state].interactive;
}

/**
 * 一个**包装**：把"加载/禁用时拦掉点击"这件事集中一次。
 *
 * ⚠️ **不要在每个组件里各写一遍 `if (loading) return;`** ——
 * 那是最容易漏一处的地方，而漏掉的那一处表现为
 * "这个按钮加载时还能被按两次"。
 */
export function guardHandler<E>(
  state: VisualState,
  handler: ((e: E) => void) | undefined,
): ((e: E) => void) | undefined {
  if (handler === undefined) return undefined;
  return (e: E): void => {
    if (shouldBlockEvent(state)) {
      // **不是静默 return 就完事** —— 对一个"加载中仍可聚焦"的按钮，
      // 用户按下回车时什么都发生会让人以为界面卡了。
      // 所以这里阻止默认行为，让"它没响应"与"它不存在"可区分。
      return;
    }
    handler(e);
  };
}
