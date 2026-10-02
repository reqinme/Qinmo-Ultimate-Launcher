/**
 * 「当前产品」这一份**全局状态**（`docs/UI设计规格.md` §4.6.1.2 方案 A）
 * ============================================================================
 *
 * ## 🔴 为什么它必须是一个 Provider，而不是每一页各存一份 `useState`
 *
 * 规格 §4.6.1.2 的原话是 **"三处都能切、三处联动"**：
 *
 * | 位置 | 形态 |
 * |---|---|
 * | 主页 | 大横幅顶端的胶囊 |
 * | 实例页 | 二级栏顶部的**产品区**（各带实例数） |
 * | 下载页 | 二级栏的**「装到」区**（选了目标实例，产品随之确定） |
 *
 * 而规格紧接着写了一句 **"三处必须是同一个状态源"**。
 * 一个"每页自己 useState"的实现在**单页内部**看不出问题 ——
 * 只有"在实例页切了产品、回主页发现横幅还写着另一个"时才看得见，
 * 而那正是 §4.6.6 抱怨过的那类缺陷（"产品维度在界面上不可见"）。
 *
 * ## ⚠️ 而它为什么**不**放在 `routes/nav.ts` 里
 *
 * `nav.ts` 是**结构**（一级七项、二级的区），它刻意不导出任何"当前产品"
 * 的变量（§4.6.1.2）：导航不参与产品判断，否则会出现第二个真相来源。
 * 所以那一区的 `source` 只写 `"products"` —— 一个**指向这里**的名字。
 *
 * ## ⚠️ 今天这一份数据还是**夹具**（诚实记录）
 *
 * `PRODUCT_FIXTURE` 的两行是占位：真产品表要由 Rust 侧的能力描述符给
 * （`Capabilities` 里每个产品的能力与 `reason`），而 **IPC 里现在
 * 没有那条命令**（`web/src/api/index.ts` 只导出 `fetchCapabilities` /
 * `fetchInstanceSummary`）。
 *
 * 所以：
 *
 * - `id` 是 `"a" / "b"` 这种中性占位（`eslint.config.js` 的 `PRODUCT_NAME`
 *   规则**禁止界面里写产品名**，于是"叫什么"这件事只能由数据给）；
 * - `instanceCount` 是 **`null`（未知）**，而不是 `0` ——
 *   §4.6.1.3 与灵动岛那条口径是同一条：**不知道的数不许编**。
 *   界面拿到 `null` 就**不渲染计数**（不是渲染 `0 个实例`）。
 */

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";

/** 产品表里的一行。 */
export interface ProductRef {
  readonly id: string;
  /** **已经本地化的**展示名。界面不做翻译。 */
  readonly label: string;
  /**
   * 这个产品下有几个实例。
   *
   * 🔴 **`null` 是"未知"，不是 `0`。** 界面必须把两者分开渲染：
   * `0` 显示"0 个实例"，`null` **一个字都不显示**。
   */
  readonly instanceCount: number | null;
}

/** 这一份全局状态的样子。 */
export interface ProductState {
  readonly rows: readonly ProductRef[];
  /** 当前产品。`rows` 为空时是 `null`。 */
  readonly activeId: string | null;
  readonly setActiveId: (id: string) => void;
}

const ProductContext = createContext<ProductState | null>(null);

/**
 * 产品表的**夹具**（真数据要等 Rust 侧把产品表接上 —— 见文件头）。
 *
 * ⚠️ 两个 id 用 `"a" / "b"` 而不是任何产品名：`eslint.config.js` 那条
 * `PRODUCT_NAME` 规则会在**字面量**上把它拦下来，而它拦得对 ——
 * 产品名只该出现在 Provider 的适配代码与面向用户的 `reason` 文案里（§4.6.4）。
 */
export const PRODUCT_FIXTURE: readonly ProductRef[] = [
  { id: "a", label: "原生版", instanceCount: null },
  { id: "b", label: "另一形态", instanceCount: null },
];

/**
 * 装上「当前产品」这一份状态。
 *
 * ⚠️ **它由 `Shell` 自己挂**（`web/src/routes/Shell.tsx`：`<ProductProvider>`
 * 就在外壳那一层），**不是 `main.tsx`** —— 理由是"谁用谁挂"：
 * 这三处消费者（侧栏产品段、二级栏的「产品 / 按产品」区、主页横幅）
 * **全部在外壳的子树里**，而挂在 `main.tsx` 会让每一个"只渲染外壳"的测试
 * （例如 `Shell.a11y.test.tsx`）都要额外补两层与本测试无关的 Provider。
 *
 * ⚠️ 而它仍然必须在**页面的子树之上** —— 主页横幅在 `<Outlet/>` 之下，
 * 于是 `useProduct()` 在那一层拿到的就是同一份状态（§4.6.1.2 要的
 * "三处都能切、三处联动"）。挂在 `Outlet` 里面就变成三份互不相干的副本。
 *
 * ⚠️ 而**灵动岛的 Provider 仍在 `main.tsx`**（`web/src/island/IslandProvider.tsx`）——
 * 它的理由是 `RouterProvider` 的子树**不继承**外层 context（那里写着），
 * 而外壳本身**就在** `RouterProvider` 的子树里，所以这里没有那个问题。
 */
export function ProductProvider({
  rows,
  children,
}: {
  readonly rows: readonly ProductRef[];
  readonly children: ReactNode;
}): ReactElement {
  // ⚠️ **初始值是"第一行"，而不是 `null`** —— 一个开着界面却"没有当前产品"
  // 的启动器会让产品段、状态条、横幅三处同时空着，而那不是一个真实的状态
  // （真实状态是"用户还没切过，所以是第一个"）。
  //
  // ⚠️ 而 `rows` 为空时它**真的是 `null`** —— 那是"一个产品都没有"，
  // 与"用第一个当默认"是两件事。
  const first = rows[0];
  const [activeId, setActive] = useState<string | null>(first === undefined ? null : first.id);

  const setActiveId = useCallback(
    (id: string) => {
      const found = rows.find((r) => r.id === id);
      if (found === undefined) {
        // 与 `nav.ts` 的 `primaryOf` 同一条纪律：这是**开发期的错**
        // （界面只该传表里的 id），不是运行时的分支 ⇒ 直接抛。
        throw new Error(`产品表里没有 ${id} —— 界面只能选表里的产品`);
      }
      setActive(id);
    },
    [rows],
  );

  const value = useMemo<ProductState>(
    () => ({ rows, activeId, setActiveId }),
    [rows, activeId, setActiveId],
  );

  return <ProductContext.Provider value={value}>{children}</ProductContext.Provider>;
}

/**
 * 读这一份全局状态。
 *
 * ⚠️ **Provider 之外调用直接抛**（不给"没有就用默认值"的兜底）：
 * 一个兜底会让"忘了装 Provider"变成"界面上那一处永远是默认产品" ——
 * 而那是一个**静默**的错，正好是这一份状态要消灭的东西。
 */
export function useProduct(): ProductState {
  const value = useContext(ProductContext);
  if (value === null) {
    throw new Error("useProduct() 必须在 <ProductProvider> 里调用（见 web/src/product/product.tsx）");
  }
  return value;
}

/** 当前产品那一行；`rows` 为空或状态里没有它时是 `null`。 */
export function activeProductOf(state: ProductState): ProductRef | null {
  if (state.activeId === null) return null;
  return state.rows.find((r) => r.id === state.activeId) ?? null;
}

/**
 * 一行产品的**实例数**该怎么写。
 *
 * 🔴 这就是"`null` 不许渲染成 `0`"那条口径的**唯一落点** ——
 * 于是它可测（`product.test.tsx`），而不是散在三个组件的 JSX 里。
 */
export function instanceCountText(count: number | null): string | undefined {
  return count === null ? undefined : `${count} 个实例`;
}
