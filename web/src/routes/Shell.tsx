import { useState, type ReactElement } from "react";
import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import {
  PRIMARY,
  PRODUCT_KEYS,
  TOOL_KEYS,
  primaryOf,
  secondaryOf,
  type PrimaryKey,
} from "./nav.ts";
import { Island, IslandLayer, idleContent } from "../island/Island.tsx";
import { IslandView } from "../island/islandMath.ts";
import "./Shell.css";

/**
 * 外壳：**两段式导航**（M4 门禁第 ③ 项）
 * ============================================================================
 *
 * 结构（`docs/UI设计规格.md` §4.6.1）：
 *
 * ```text
 *  ┌──────────────┬───────────────────┬──────────────────────────┐
 *  │ 账户卡        │ 二级栏            │                          │
 *  │ ─────────    │ （**内容由一级     │        主区               │
 *  │ 主页          │   决定**，§4.6.1.1）│                          │
 *  │ 实例          │                   │                          │
 *  │ 下载          │                   │                          │
 *  │ ─────────    │                   │                          │
 *  │ 账号管理      │                   │                          │
 *  │ 百宝箱        │                   │                          │
 *  │ 设置          │                   │                          │
 *  │ 关于          │                   │                          │
 *  └──────────────┴───────────────────┴──────────────────────────┘
 * ```
 *
 * ## 🔴 三条实现纪律
 *
 * 1. **一级高亮与二级栏内容都只从路由路径推出来** —— 没有"当前选中项"的
 *    组件状态。两个真相来源会立刻不同步（例如浏览器后退之后）。
 * 2. **一级没有下级时，二级栏整条不存在**（§4.6.1.1）—— 见 `secondaryOf`。
 * 3. **界面不认识具体游戏**（门禁⑤ 的 lint 规则）—— 这里的每一项都是
 *    固定的能力名，而不是 `if (product === 'java')`。
 */
export function Shell(): ReactElement {
  // ⚠️ **从路由本身推出"当前一级"**，而不是从事件处理器里 setState。
  //
  // 理由：浏览器后退 / 直接输入 URL / 将来从托盘跳转 —— 这些入口
  // 都不会经过"点击侧栏"那条路。一个用 state 记当前项的实现
  // 在那些入口下会**高亮错的那一项**。
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  // ⚠️ **队列在 M4 主干里还没有真实来源**（那要等安装/启动的进度上报接进来）。
  // 所以现在挂的是**空闲态** —— 而它的形状与真实数据一样，
  // 于是接上真数据时只换这一行。
  //
  // 而"收起"是一个**本地 UI 状态**（它是用户的选择），不是队列状态 ——
  // 所以由外壳持有，而内核的队列一个字节都不受影响（§7.3 约束 4）。
  const [islandCollapsed, setIslandCollapsed] = useState(false);
  const islandContent = idleContent("离线账户", "官方源");
  const current = primaryFromPath(pathname);
  const secondary = secondaryOf(current);

  return (
    <div className="shell">
      <nav className="shell__rail" aria-label="主导航">
        {/* 账户卡：**不属于导航**，所以它不在 PRIMARY 里 */}
        <div className="shell__account">
          <span className="shell__accountBadge" aria-hidden="true" />
          <span className="shell__accountName">未登录</span>
        </div>

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
      </nav>

      {/*
        ⚠️ **`null` ⇒ 整条二级栏不存在。** 见 `nav.ts` 里
        "`null` 与 `[]` 的区别"那一段 —— 渲染一个空的二级栏会让
        "主页"看起来像"一个有二级栏但里面没东西的页"。
      */}
      {secondary === null ? null : (
        <nav className="shell__secondary" aria-label="二级导航">
          <ul className="shell__secondaryList">
            {secondary.map((s) => (
              <li key={s.key}>
                <Link
                  to={secondaryHref(current, s.key)}
                  className="shell__secondaryItem"
                  activeProps={{ className: "shell__secondaryItem shell__secondaryItem--on" }}
                >
                  {s.label}
                </Link>
              </li>
            ))}
          </ul>
        </nav>
      )}

      <main className="shell__main">
        <Outlet />
      </main>
      {/*
        ⚠️ **灵动岛挂在网格之外**（`position: fixed`）—— 它不是三段布局的一员。
        放进网格会挤出一个格子，而它是"浮在最上层"的（§5.3 使用纪律 2）。

        而它在**外壳里**而不是某一页里，正因为 §7.1 的定位：
        "把此刻唯一值得看的事**提到顶层**" —— 它的作用域是整个应用。
      */}
      <IslandLayer>
        <Island
          content={islandContent}
          onCollapse={() => setIslandCollapsed(true)}
          view={islandCollapsed ? IslandView.Collapsed : IslandView.Compact}
        />
      </IslandLayer>
    </div>
  );
}

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
  return (
    <li>
      <Link
        to={hrefOf(itemKey)}
        className={on ? "shell__railItem shell__railItem--on" : "shell__railItem"}
        aria-current={on ? "page" : undefined}
      >
        {/* 图标位：口径是"自绘 SVG 集合"（§4.5），门禁② 里补 */}
        <span className="shell__railGlyph" aria-hidden="true" />
        {item.label}
      </Link>
    </li>
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
 * 而那个规律有一个例外：**`home` 的二级是 `null`，所以它不会走到这里**。
 */
function secondaryHref(primary: PrimaryKey, secondaryKey: string): string {
  const base = hrefOf(primary);
  const sep = base === "/" ? "" : "/";
  return `${base}${sep}${secondaryKey}`;
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
