/**
 * 侧边栏宽度这一个**偏好**（`docs/UI设计规格.md` §4.6.2）
 * ============================================================================
 *
 * ## 规格要求的三件事
 *
 * | 要求 | 落点 |
 * |---|---|
 * | **窄栏（默认）56 px 仅图标 / 宽栏 200 px 图标 + 名称** | `RailWidth` 两个成员 + `web/src/routes/Shell.css` 的 `--shell-rail-w` |
 * | 切换位置 = 设置 →「外观」→ 侧边栏（分段控件） | `web/src/routes/SettingsPage.tsx` |
 * | **立即生效、不需重启** | 这里是一个 context ⇒ 一按就重渲染 |
 *
 * ## ⚠️ 为什么是 `"narrow" | "wide"` 而不是一个布尔
 *
 * `eslint.config.js` 的第三条纪律是"**禁止 toggle 式布尔状态** ——
 * 用联合类型表达状态机，这样'非法状态组合'在类型层面就不存在"。
 *
 * 宽度这件事看起来只有两态，所以很容易写成 `isWide`；但写成联合类型
 * 换来两件具体的事：① 分段控件的两个按钮与两个取值**一一对应**，
 * 于是"选中态"不需要再写一个 `!isWide` 的反向判断；
 * ② 将来加第三态（例如"跟随窗口宽度"）时，**每一个 switch 都会报错**，
 * 而不是静默地落进 `else` 那一支。
 *
 * ## 🔴 而它今天**还没有持久化**（诚实记录）
 *
 * 规格那一行还写着"**记住用户选择**并随配置备份迁移"。那件事要等
 * 设置存储接上（Rust 侧的配置 + 备份），而**今天没有那一层**
 * （`web/src/appearance/appearance.ts` 里也没有 store，只有纯函数）。
 *
 * 所以这一份状态**只活在这一次会话里**：切页不丢（Provider 在外壳那一层，
 * 见 `web/src/routes/Shell.tsx`），而**重启窗口会回到默认的窄栏**。
 * 一个用 `localStorage` 顶上的实现会**假装**那件事已经做了 ——
 * 而它不会跟着配置备份迁移，于是"备份恢复之后侧栏突然变了"。
 *
 * ⚠️ **Provider 挂在哪一层**：外壳自己那一层（`<Shell>` 的根，
 * `web/src/routes/Shell.tsx`），**不是 `main.tsx`** —— 今天唯一的消费者
 * 是外壳的根节点（它把值写进 `data-rail`），而挂在 `main.tsx` 会让每一个
 * "只渲染外壳"的测试都要多补一层 Provider。将来设置页要能切它，
 * 而设置页在 `<Outlet/>` 之下 ⇒ 也在外壳的子树里，所以这一层够用。
 */

import { createContext, useContext, useMemo, useState, type ReactElement, type ReactNode } from "react";

/** 侧边栏的两种宽度。 */
export type RailWidth = "narrow" | "wide";

/** 分段控件里两段的**顺序与文案**（§4.6.2 只允许这两个取值）。 */
export const RAIL_WIDTHS: readonly RailWidth[] = ["narrow", "wide"];

/** 取值 → 界面上的名字。 */
export const RAIL_WIDTH_LABELS: Readonly<Record<RailWidth, string>> = {
  narrow: "窄栏",
  wide: "宽栏",
};

/** 这一份偏好的样子。 */
export interface RailPrefState {
  readonly railWidth: RailWidth;
  readonly setRailWidth: (width: RailWidth) => void;
}

const RailPrefContext = createContext<RailPrefState | null>(null);

/**
 * 装上这一份偏好。
 *
 * ⚠️ 默认是 **`"narrow"`**（§4.6.2：窄栏是默认态）。
 */
export function RailPrefProvider({ children }: { readonly children: ReactNode }): ReactElement {
  const [railWidth, setRailWidth] = useState<RailWidth>("narrow");
  const value = useMemo<RailPrefState>(() => ({ railWidth, setRailWidth }), [railWidth]);
  return <RailPrefContext.Provider value={value}>{children}</RailPrefContext.Provider>;
}

/**
 * 读这一份偏好。
 *
 * ⚠️ **Provider 之外调用直接抛**，理由同 `useProduct()`：
 * 一个兜底会让"忘了装 Provider"变成"分段控件点了没反应" —— 静默的错。
 */
export function useRailPref(): RailPrefState {
  const value = useContext(RailPrefContext);
  if (value === null) {
    throw new Error("useRailPref() 必须在 <RailPrefProvider> 里调用（见 web/src/shell/railPref.tsx）");
  }
  return value;
}
