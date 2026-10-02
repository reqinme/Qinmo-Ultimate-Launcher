/**
 * 自绘标题栏（`UI设计规格.md` §4.5 · 高 **36 px**）
 * ============================================================================
 *
 * ## 🔴 它为什么必需：`decorations: false`
 *
 * 关掉系统标题栏是为了让**灵动岛浮在顶部**（§4.5 的 A 计划）。
 * 而系统标题栏同时提供了三样东西：
 *
 * | 它提供的 | 关掉之后 |
 * |---|---|
 * | **拖动区** | 窗口**移不动** |
 * | **最小化 / 最大化 / 关闭** | 只能靠任务栏或 Alt+F4 |
 * | 品牌与标题 | 用户不知道这是哪个应用 |
 *
 * 所以这个组件不是装饰 —— **它是那三样东西的替代品**。
 *
 * ## 它画什么（照 §4.5 那张图标）
 *
 * ```text
 *  │  ◈ 秦墨      ⌘ 搜索                ╭──岛──╮     ─   ▢   ✕        │ 36px
 * ```
 *
 * ⚠️ **而岛不在这个组件里** —— `IslandLayer` 是 `position: fixed` 的，
 * 它**浮在标题栏之上**（§5.3 使用纪律 2："浮在最上层"）。
 * 把岛塞进标题栏的 flex 流里会让它**挤走**搜索框，而那是布局上的错。
 * 所以标题栏在**中间留出那段空位**（`titlebar__slot`），而岛浮在上面。
 *
 * ## ⚠️ 而"Snap Layouts"那一档**做不到**，而规格里预留了降级线
 *
 * §4.5.4 的原文（`docs/UI设计规格.md` §4.5.4）：
 *
 * > | 悬停最大化按钮 | **必须弹出系统的 Snap Layouts 面板**（Win11 的贴靠布局） |
 * > | 实现 | 窗口需保留 `WS_THICKFRAME` / `WS_MAXIMIZEBOX` 风格；**最大化按钮的
 * >   命中区需向 Tauri 暴露系统的"标题栏按钮"语义**（具体做法列入 M4 门禁验证项
 * >   —— 这是 Tauri 自绘标题栏的已知难点） |
 * > | 若做不到 | **降级**：至少支持 `Win + ←/→/↑` 的键盘贴靠 ——
 * >   **这不能也不该被自绘窗口破坏** |
 *
 * **我们走降级档**，而理由要写清：那条"命中区语义"要用
 * `WM_NCHITTEST` 返回 `HTMAXBUTTON`，而在 WM_NCHITTEST 阶段
 * **窗口的过程函数归 WebView2 与 Tauri 共同持有** —— 在那里横插一段
 * 需要子类化（`SetWindowSubclass`），而那是一处**能弄坏输入**的改动。
 *
 * ⚠️ **而这是一条"没做到"，不是"不用做"** —— 所以它记在这里，
 * 而 `Win + ←/→/↑` 那一条**必须验**（它是降级线的验收）。
 */

import { useEffect, useState, type ReactElement } from "react";
import {
  windowClose,
  windowIsMaximized,
  windowMinimize,
  windowStartDragging,
  windowToggleMaximize,
} from "../api/window.ts";
import { swallows, type EventFamily, type TitlebarId } from "./contract.ts";
import { WindowGlyph, type WindowGlyphName } from "./glyphs.tsx";
import "./TitleBar.css";

/**
 * 三个窗口按钮。
 *
 * ⚠️ **而它们的 `aria-label` 是有意写成中文动作的** ——
 * 读屏用户听到的是"最小化"，而不是"─"。
 *
 * 🔴 **`glyph` 是 `glyphs.tsx` 里的图形名，不是字形字符。**
 *
 * ⚠️ 而这里曾经有两个错，它们长在同一个字段上：
 *
 * | 错 | 症状 | 谁看见的 |
 * |---|---|---|
 * | 最大化那一格画的是 `MorphGlyph`（**菜单 ⇄ 关闭**） | `─ ☰ ✕`，最大化后 `─ ✕ ✕` | **用户**（规格 §4.5.2 要的是 `─ ▢ ✕`） |
 * | 三个 id 只靠 `as const` "对得上契约表" | 写错一个字母**不是**编译错误 | 没有人 |
 *
 * 所以现在是 `satisfies`：`key` 必须是契约表的 `TitlebarId`，
 * `glyph` 必须是 `glyphs.tsx` 的 `WindowGlyphName` —— **拼错即编译错误**。
 */
const CONTROLS = [
  { key: "min", glyph: "minimize", label: "最小化" },
  { key: "max", glyph: "maximize", label: "最大化" },
  { key: "close", glyph: "close", label: "关闭" },
] as const satisfies readonly {
  readonly key: TitlebarId;
  readonly glyph: WindowGlyphName;
  readonly label: string;
}[];

/**
 * 🔴 **"这一格该吞吗"** —— 标题栏根部那道闸。
 *
 * ⚠️ **它在根部，而不是在每个元素上挂 `stopPropagation`** —— 这不是风格选择，
 * 是一个实测出来的事实：`disabled` 的搜索框**收不到 React 的合成
 * `onDoubleClick`**（React 对"实际禁用"的表单元素不派发鼠标类合成事件），
 * 于是"把排除挂在那个按钮上"那一格**静默失效** —— 双击搜索框冒到根部，
 * 窗口被最大化。而 `<header>` 从不被禁用，所以判断放在这里。
 *
 * 而"事件落在哪个元素上"由 `data-titlebar-item` 反查（§4.5.1 的契约表是
 * 唯一数据源，见 `contract.ts`）。
 */
function swallowed(target: EventTarget | null, family: EventFamily): boolean {
  if (!(target instanceof Element)) return false;
  const owner = target
    .closest("[data-titlebar-item]")
    ?.getAttribute("data-titlebar-item");
  return owner !== null && owner !== undefined && swallows(owner, family);
}

export function TitleBar(): ReactElement {
  const [maximized, setMaximized] = useState(false);

  // 🔴 **"最大化"不只由我们那三条命令改变。**
  //
  // `Win + ↑`、把窗口拖到屏幕顶端、以及系统那套贴靠都会最大化窗口 ——
  // 而那些动作**不经过** `onClick`，于是 `maximized` 会变成一个**谎**：
  // 窗口已经最大化，而按钮画的还是那个"点了能最大化"的 `▢`。
  //
  // 所以状态**在挂载时问一次，并在每次尺寸变化之后再问一次**。
  // `resize` 在 WebView2 里由宿主尺寸变化触发（最大化 / 还原 / 贴靠都会），
  // 而真相来自窗口那边（`window_is_maximized`），不是前端猜的。
  //
  // ⚠️ 两个细节都是刻意的：
  //
  // 1. **不是"只在挂载时问一次"** —— 那比不修更坏：它把"点两下之后一致"
  //    变成了"只有我们自己点的时候才一致"。
  // 2. **拖边框会在一次拖动里发几十个 `resize`**，所以那一路用 150 ms 的
  //    去抖（§7.3 的节流纪律：一次拖动不该变成几十次 IPC）。
  useEffect(() => {
    let pending = 0;
    const ask = (): void => {
      void windowIsMaximized().then(setMaximized);
    };
    const later = (): void => {
      window.clearTimeout(pending);
      pending = window.setTimeout(ask, 150);
    };
    ask();
    window.addEventListener("resize", later);
    return () => {
      window.clearTimeout(pending);
      window.removeEventListener("resize", later);
    };
  }, []);

  return (
    <header
      className="titlebar"
      // ⚠️ 根部自己那一行（§4.5.1 契约表里的 `blank`）：它的 `pointerdown`
      // 与 `dblclick` 都是 **冒** —— 因为"标题栏空白处"就是它自己。
      data-titlebar-item="blank"
      // ⚠️ **`onPointerDown` 而不是 `onClick`。**
      //
      // 拖动必须在**按下的那一刻**开始（那是系统标题栏的行为）——
      // 一个 `onClick` 的实现要等用户抬手，于是"按住拖动"会**完全无效**，
      // 而"点一下"倒会莫名开始拖动。
      //
      // ⚠️ 而它**在捕获阶段**（`onPointerDownCapture`）不必要：排除由下面
      // 那道闸做，而闸问的是"事件落在哪个元素上"（§4.5.1 的契约表）。
      onPointerDown={(e) => {
        // 只响应主键（左键）—— 右键拖动是另一件事。
        if (e.button !== 0) return;
        // 🔴 **先问契约表**：这一格是 `吞` 的元素（品牌 / 搜索框 / 三个控制）
        // 上按下，**不该拖窗口**。
        if (swallowed(e.target, "pointerdown")) return;
        void windowStartDragging();
      }}
      // 🔴 **双击空白处 = 最大化 / 还原。**
      //
      // 而 §4.5.3 把它标成"**必须支持**"：
      //
      // > | 双击 | 标题栏空白处双击 = **最大化/还原**（系统行为，必须支持） |
      //
      // ⚠️ **"必须支持"那几个字在这里有一层具体的含义**：
      // 它是**用户对标题栏的肌肉记忆里最强的一条** —— 比拖动还强。
      // 一个只有拖动而没有双击的标题栏会让人**反复试而不得**，
      // 而那不会有人报成 bug（他们会以为"这个应用就是不能双击"）。
      //
      // ⚠️ **而"空白处"由那道闸保证** —— 品牌、搜索框、三个控制的那一格
      // 都是 `吞`。一个"在标题栏上无条件处理双击"的实现会让**双击关闭按钮**
      // 变成"最大化"，而那是用户最难理解的一类行为（`faa7fa1` 抓到的正是它）。
      onDoubleClick={(e) => {
        // 🔴 **先问契约表** —— 否则双击搜索框会最大化窗口（实测漏过的那一格）。
        if (swallowed(e.target, "dblclick")) return;
        void windowToggleMaximize().then(setMaximized);
      }}
    >
      <span
        className="titlebar__brand"
        data-titlebar-item="brand"
        // ⚠️ **品牌也必须排除交互** —— 而这是 §4.5.3 的原文：
        //
        // > **排除交互元素**：**品牌、搜索框、产品切换器、灵动岛、
        // > 窗口控制按钮都必须排除拖动**（否则点不动）
        //
        // 而"点不动"在这里有一个更具体的后果：**双击品牌会最大化窗口** ——
        // 因为双击由上面那个 `onDoubleClick` 处理。
        // 而用户双击品牌多半是想**选中它**，或者**什么都不做**。
        //
        // 🔴 **而这两格不再手写、也不再挂在这个元素上**：它们由 §4.5.1 的
        // 契约表派生（`contract.ts` 的 `brand` 行 —— `pointerdown` 与
        // `dblclick` 都是 `吞`），而执行那道闸的是上面的 `<header>`。
        //
        // ⚠️ **为什么排除不在这个元素上做**：`disabled` 的搜索框收不到
        // React 的合成 `onDoubleClick`，于是"挂在元素上"会**静默漏掉一格**。
        // 见 `contract.ts` 里 `swallows()` 那段。
      >
        <span className="titlebar__mark" aria-hidden="true" />
        秦墨
      </span>

      {/* ⚠️ 搜索现在是**禁用态**，而它带原因。
          一个"画一个假搜索框"的实现会让用户点它 —— 而点了没反应
          比"看得到它还没好"更糟（§6.3 的六态里有"禁用必须带原因"）。 */}
      <button
        type="button"
        className="titlebar__search"
        data-titlebar-item="search"
        disabled
        title="全局搜索要等内容索引接上来（M6 之后）"
        // ⚠️ 而它**也要排除拖动与双击**（见品牌那一段）。
        // 一个禁用的按钮**仍然会收到 pointerdown** —— 所以那两格 `吞` 不是多余的。
        // 🔴 而这里**不再挂处理函数**：禁用元素收不到 React 的合成事件，
        // 于是排除由根部那道闸做（这一格是 `contract.test.tsx` 抓出来的）。
      >
        <span aria-hidden="true">⌘</span> 搜索
      </button>

      {/*
        🔴 **这一段是给岛留的位置。**
        ⚠️ 而它是**空**的 —— 岛由 `IslandLayer` 浮在它上面
        （`position: fixed`）。见本文件顶部那段：把岛放进 flex 流里
        会挤走搜索框，而那是布局上的错。
      */}
      <span className="titlebar__slot" aria-hidden="true" />

      <div className="titlebar__controls">
        {CONTROLS.map((c) => (
          <button
            key={c.key}
            type="button"
            className={`titlebar__btn titlebar__btn--${c.key}`}
            // ⚠️ **最大化之后这个名字要变成"还原"** —— 因为那才是那个按钮
            // **那时**做的事（§4.5.2 的"图标换为还原"是给眼睛的，
            // 这一行是同一件事给读屏）。
            aria-label={c.key === "max" && maximized ? "还原" : c.label}
            // ⚠️ **它的 id 就是 `c.key`**（`min` / `max` / `close`）——
            // 而 `CONTROLS` 是 `as const`，于是这三个 id 与契约表的
            // `TitlebarId` 对得上：**写错一个字母是编译错误**。
            data-titlebar-item={c.key}
            // ⚠️ **按钮上的按下不该拖窗口** —— 见上面 `onPointerDown` 那段。
            //
            // 🔴 **而这一组属性现在由契约表派生**，包括那个曾经缺失的
            // `dblclick` 格：
            //
            // 缺了它的后果：**双击「关闭」会先最大化，再关闭** ——
            // 因为双击事件冒泡到标题栏那个 `onDoubleClick`。
            //
            // ⚠️ 而那是**被 `TitleBar.test.tsx` 抓到的一个真 bug**：
            // 那条测试断言"双击关闭按钮只关闭、不最大化"，而它第一版红了。
            //
            // > 而这一类 bug 的共同形状是：**排除了一种事件，而漏了另一种**。
            // > 拖动只排除 `pointerdown`；而双击是**另一个事件类型**。
            //
            // 现在"漏一格"要么改契约表（于是 `check-titlebar-contract.ps1`
            // 与规格对不上 ⇒ 红），要么**什么都不会发生**：根部那道闸读的就是表。
            onClick={() => {
              if (c.key === "min") void windowMinimize();
              else if (c.key === "close") void windowClose();
              else {
                void windowToggleMaximize().then(setMaximized);
              }
            }}
          >
            {/*
              ⚠️ **图标跟着真相走**（`maximized` = 窗口那边的返回值，
              加上挂载时与每次 `resize` 之后的那次查询）。

              🔴 **而这一格曾经是 `MorphGlyph`（`morphicons` 尖刺的产物）** ——
              它画的是"菜单 ⇄ 关闭"，于是最大化按钮画成了**汉堡三横线**。
              规格 §4.5.2 要的是 `─ ▢ ✕`，而那个 `▢` **从来没有被画过**；
              看见它的是用户的眼睛，而不是任何一条测试（见 `glyphs.tsx`）。

              ⚠️ 所以现在这一格在"未最大化 / 已最大化"之间切**两个不同的图形**
              （`maximize` ⇄ `restore`），而四个图形全部自绘。
            */}
            {c.key === "max" ? (
              <WindowGlyph name={maximized ? "restore" : "maximize"} />
            ) : (
              <WindowGlyph name={c.glyph} />
            )}
          </button>
        ))}
      </div>
    </header>
  );
}
