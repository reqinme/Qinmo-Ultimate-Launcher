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
 * §4.5 的原文（`UI设计规格.md:849`）：
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

import { useState, type ReactElement } from "react";
import {
  windowClose,
  windowMinimize,
  windowStartDragging,
  windowToggleMaximize,
} from "../api/window.ts";
import "./TitleBar.css";

/**
 * 三个窗口按钮。
 *
 * ⚠️ **而它们的 `aria-label` 是有意写成中文动作的** ——
 * 读屏用户听到的是"最小化"，而不是"─"。
 */
const CONTROLS = [
  { key: "min", glyph: "─", label: "最小化" },
  { key: "max", glyph: "▢", label: "最大化" },
  { key: "close", glyph: "✕", label: "关闭" },
] as const;

export function TitleBar(): ReactElement {
  const [maximized, setMaximized] = useState(false);

  return (
    <header
      className="titlebar"
      // ⚠️ **`onPointerDown` 而不是 `onClick`。**
      //
      // 拖动必须在**按下的那一刻**开始（那是系统标题栏的行为）——
      // 一个 `onClick` 的实现要等用户抬手，于是"按住拖动"会**完全无效**，
      // 而"点一下"倒会莫名开始拖动。
      //
      // 而它**在捕获阶段**（`onPointerDownCapture`）不必要：三个按钮
      // 的 `onPointerDown` 会先 `stopPropagation`，于是按按钮不会拖窗口。
      onPointerDown={(e) => {
        // 只响应主键（左键）—— 右键拖动是另一件事。
        if (e.button !== 0) return;
        void windowStartDragging();
      }}
    >
      <span className="titlebar__brand">
        <span className="titlebar__mark" aria-hidden="true" />
        秦墨
      </span>

      {/* ⚠️ 搜索现在是**禁用态**，而它带原因。
          一个"画一个假搜索框"的实现会让用户点它 —— 而点了没反应
          比"看得到它还没好"更糟（§6.3 的六态里有"禁用必须带原因"）。 */}
      <button
        type="button"
        className="titlebar__search"
        disabled
        title="全局搜索要等内容索引接上来（M6 之后）"
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
            aria-label={c.label}
            // ⚠️ **按钮上的按下不该拖窗口** —— 见上面 `onPointerDown` 那段。
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => {
              if (c.key === "min") void windowMinimize();
              else if (c.key === "close") void windowClose();
              else {
                void windowToggleMaximize().then(setMaximized);
              }
            }}
          >
            {/* ⚠️ 最大化的图标要跟着**真相**走（`maximized` 来自返回值）。
                一个永远画 ▢ 的实现会让"已经是最大化"看不出来。 */}
            <span aria-hidden="true">
              {c.key === "max" && maximized ? "❐" : c.glyph}
            </span>
          </button>
        ))}
      </div>
    </header>
  );
}
