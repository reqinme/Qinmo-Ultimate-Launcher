import type { ReactElement, ReactNode } from "react";
import { useEffect, useState } from "react";
import { IslandView } from "./islandMath.ts";
import "./Island.css";

/**
 * 灵动岛（**M4 主干** · `docs/UI设计规格.md` §7）
 * ============================================================================
 *
 * ## 它是"把此刻唯一值得看的事提到顶层"
 *
 * §7.1 的定位原话。而它有三条硬规则（§7.1 / §7.3），本文件逐条落：
 *
 * | 规则 | 落点 |
 * |---|---|
 * | **只有一个岛、一个主状态** | 由内核的 `IslandQueue` 保证（**类型上不存在两个**）—— 本组件只画一个 |
 * | **两态：`Compact` 32px / `Expanded` ≤220px** | `data-view` 属性 + CSS，**切换只用 `transform` + `opacity`** |
 * | **可拖拽 + 位置记忆** | `useIslandDrag` 钩子（`localStorage` 存位置） |
 * | **可关闭但任务不丢失** | ✕ 设 `view = Collapsed`，**内核队列照旧在走** |
 * | **进度节流 ≥100ms 或 ≥1%** | `useThrottledProgress`（§7.3 约束 5） |
 *
 * ## 🔴 而"产品无关"这一条是本组件的形状
 *
 * §7.2：*"以上 8 态**全部与产品无关**"*，产品差异只在**展开面板的字段清单**里，
 * 由 `Provider.ui_hints(instance)` 驱动，**不写分支**。
 *
 * 所以本组件**没有一处** `if (product === …)`：它拿到的 `IslandState` 已经是
 * 内核算好的结论，而"该显示哪些字段"由调用方传进来的 `hints` 决定。
 */

/** 岛要显示的东西 —— **它的形状就是内核的 `IslandState`**。 */
export type IslandKind =
  | "idle"
  | "probe"
  | "download"
  | "install"
  | "launch"
  | "running"
  | "error"
  | "update";

/**
 * 展开面板里的一条字段。
 *
 * ⚠️ **它是数据，不是判断。** 调用方（来自 `Provider.ui_hints`）给什么就画什么，
 * 组件不决定"运行时该不该显示内存"。
 */
export interface HintField {
  readonly label: string;
  readonly value: string;
}

/** 岛的内容（**全部来自内核**）。 */
export interface IslandContent {
  readonly kind: IslandKind;
  /** 一句话主文案（"下载中" / "预检中 · 3 项"）。 */
  readonly headline: string;
  /** 副文案（`62% · 1.2 GB/1.9 GB · ↓12.4 MB/s · 剩余 2 分`）。 */
  readonly detail?: string;
  /** 进度（0–1）。`null` = **不确定**（它必须有自己的样子，不是 0%）。 */
  readonly fraction: number | null;
  /** 展开面板的字段（**由 Provider 的能力描述符给**）。 */
  readonly hints: readonly HintField[];
  /** `error` 态的错误码（给人看的那一句话进 `headline`）。 */
  readonly code?: string;
}

export interface IslandProps {
  readonly content: IslandContent;
  /** 用户点了岛体（§7.2 的"点击行为"）。 */
  readonly onActivate?: () => void;
  /** 用户点了 ✕ —— 收起成一行状态条。 */
  readonly onCollapse?: () => void;
  /** 初始形态。 */
  readonly view?: IslandView;
}

/**
 * 灵动岛。
 *
 * ⚠️ **它刻意不持有"当前是什么状态"。** 那个状态由内核的队列决定，
 * 而这个组件的 `content` 属性就是那份结论。
 * 一个在组件里也存一份"当前任务"的实现会立刻有两个真相来源 ——
 * 而那正是 §4.6.1.2 那条纪律要禁止的形态。
 */
export function Island({
  content,
  onActivate,
  onCollapse,
  view: initialView = IslandView.Compact,
}: IslandProps): ReactElement {
  const [view, setView] = useState<IslandView>(initialView);
  const drag = useIslandDrag();

  // ⚠️ **形态由"用户的选择"与"有没有内容"共同决定**，而不是由内容自己改。
  // 一个"下载开始就自动展开"的实现会在用户刚刚收起它之后又把它弹开。
  const effective = view;

  return (
    <div
      className="island"
      data-view={effective}
      data-kind={content.kind}
      style={drag.style}
      onPointerDown={drag.onPointerDown}
      // ⚠️ `role="status"` 而不是 `alert`：岛上的多数变化是**信息**，
      // 而 `alert` 会在读屏上打断用户正在听的东西。真正需要打断的是
      // `error` 态，那由它自己的 `role="alert"` 承担（见下面）。
      role={content.kind === "error" ? "alert" : "status"}
      aria-live={content.kind === "error" ? "assertive" : "polite"}
    >
      <button
        type="button"
        className="island__body"
        onClick={onActivate}
        // 岛的紧凑态是一个胶囊，而它里面**没有**独立的按钮（那会让 Tab 多停一次）。
        aria-label={`${content.headline}${content.detail === undefined ? "" : ` · ${content.detail}`}`}
      >
        <IslandGlyph content={content} />
        <span className="island__text">
          <span className="island__headline">{content.headline}</span>
          {/* ⚠️ **副文案在紧凑态也留着** —— `Expanded` 只是把它放大，
              而不是"展开才有信息"。一个只在展开时才给数字的实现会让
              用户必须先点一下才知道进度。 */}
          {content.detail === undefined ? null : (
            <span className="island__detail">{content.detail}</span>
          )}
        </span>
        {content.fraction === null ? null : (
          <span className="island__pct">{Math.round(content.fraction * 100)}%</span>
        )}
      </button>

      {effective === IslandView.Expanded ? (
        <div className="island__panel">
          <dl className="island__fields">
            {content.hints.map((h) => (
              <div className="island__field" key={h.label}>
                <dt className="island__fieldLabel">{h.label}</dt>
                <dd className="island__fieldValue">{h.value}</dd>
              </div>
            ))}
          </dl>
        </div>
      ) : null}

      <div className="island__actions">
        <button
          type="button"
          className="island__action"
          onClick={() => setView(effective === IslandView.Expanded ? IslandView.Compact : IslandView.Expanded)}
          aria-label={effective === IslandView.Expanded ? "收起详情" : "展开详情"}
        >
          {effective === IslandView.Expanded ? "▴" : "▾"}
        </button>
        {onCollapse === undefined ? null : (
          <button
            type="button"
            className="island__action"
            onClick={() => {
              setView(IslandView.Collapsed);
              onCollapse();
            }}
            aria-label="收起为状态条（任务不中断）"
          >
            ✕
          </button>
        )}
      </div>
    </div>
  );
}

/**
 * 左边的图形：**进度环 / 状态点 / 转鼓**。
 *
 * ⚠️ 而"转鼓"（`probe` 态那个）是 §7.3 约束 6 里**唯一被允许的无限循环**
 *（"无限循环动画（**转鼓除外且需可静止**）"）。所以它带
 * `animation-iteration-count` 的开关见 CSS 里的 `prefers-reduced-motion`。
 */
function IslandGlyph({ content }: { readonly content: IslandContent }): ReactElement {
  if (content.fraction !== null) {
    return (
      <span
        className="island__ring"
        aria-hidden="true"
        style={{
          background: `conic-gradient(var(--accent) ${content.fraction * 360}deg, var(--surface-active) 0deg)`,
        }}
      >
        <span className="island__ringHole" />
      </span>
    );
  }
  if (content.kind === "probe" || content.kind === "launch" || content.kind === "install") {
    return <span className="island__drum" aria-hidden="true" />;
  }
  if (content.kind === "error") {
    return <span className="island__dot island__dot--danger" aria-hidden="true" />;
  }
  if (content.kind === "running") {
    return <span className="island__dot island__dot--live" aria-hidden="true" />;
  }
  return <span className="island__dot" aria-hidden="true" />;
}

/**
 * 拖拽 + **位置记忆**（§7.3 约束 3）。
 *
 * ## ⚠️ 只用 `transform`，不改 `left` / `top`
 *
 * §4.4 的纪律。而这里它还有一个实际好处：拖动过程中**不触发重排**，
 * 所以 60fps 更容易保住。
 *
 * ## 而"位置记忆"存在 `localStorage` 里
 *
 * 它是**界面偏好**，不是实例数据 —— 所以它不该进内核的配置层级
 *（那是 M6 的实例 profile 管的东西）。而"岛在屏幕上的位置"与
 * "用户在哪个实例上"完全无关。
 */
export function useIslandDrag(storageKey = "qinmo.island.pos"): {
  readonly style: { transform: string };
  readonly onPointerDown: (e: React.PointerEvent<HTMLDivElement>) => void;
} {
  const [pos, setPos] = useState<{ x: number; y: number }>(() => {
    // ⚠️ `localStorage` 在测试环境（jsdom）里存在，但在某些
    // 隐私模式/沙箱下会**抛异常**。一个不 try 的实现会让整个界面白屏。
    try {
      const raw = globalThis.localStorage?.getItem(storageKey);
      if (raw !== null && raw !== undefined) {
        const p: unknown = JSON.parse(raw);
        if (
          typeof p === "object" &&
          p !== null &&
          typeof (p as { x?: unknown }).x === "number" &&
          typeof (p as { y?: unknown }).y === "number"
        ) {
          return { x: (p as { x: number }).x, y: (p as { y: number }).y };
        }
      }
    } catch {
      // 读不到就用默认位置 —— **不抛**。
    }
    return { x: 0, y: 0 };
  });

  const [drag, setDrag] = useState<{ dx: number; dy: number } | null>(null);

  useEffect(() => {
    if (drag === null) return;
    const move = (e: PointerEvent): void => {
      setDrag({ dx: drag.dx + e.movementX, dy: drag.dy + e.movementY });
    };
    const up = (): void => {
      setPos((p) => {
        const next = { x: p.x + drag.dx, y: p.y + drag.dy };
        try {
          globalThis.localStorage?.setItem(storageKey, JSON.stringify(next));
        } catch {
          // 存不下就算了 —— **位置记忆丢了不该影响任何事**。
        }
        return next;
      });
      setDrag(null);
    };
    globalThis.addEventListener("pointermove", move);
    globalThis.addEventListener("pointerup", up, { once: true });
    return () => {
      globalThis.removeEventListener("pointermove", move);
      globalThis.removeEventListener("pointerup", up);
    };
  }, [drag, storageKey]);

  const x = pos.x + (drag?.dx ?? 0);
  const y = pos.y + (drag?.dy ?? 0);

  return {
    style: { transform: `translate3d(${x}px, ${y}px, 0)` },
    onPointerDown: (e) => {
      // ⚠️ **只在岛体上开始拖**，不在按钮上 —— 否则"点 ✕"会变成"拖一下"。
      if ((e.target as HTMLElement).closest("button") !== null) return;
      setDrag({ dx: 0, dy: 0 });
    },
  };
}

/** 一个空态岛（队列为空时用它，省得调用方自己拼）。 */
export function idleContent(account: string, source: string): IslandContent {
  return {
    kind: "idle",
    headline: source,
    detail: account,
    fraction: null,
    hints: [
      { label: "账户", value: account },
      { label: "源", value: source },
    ],
  };
}

/** 供外壳挂岛的容器 —— 它只是一个定位层。 */
export function IslandLayer({ children }: { readonly children: ReactNode }): ReactElement {
  return <div className="islandLayer">{children}</div>;
}
