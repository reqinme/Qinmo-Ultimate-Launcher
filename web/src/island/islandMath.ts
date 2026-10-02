/**
 * 灵动岛的形态枚举（**与内核的 `IslandView` 一一对应**）
 * ============================================================================
 *
 * ## 🔴 为什么它是一个独立的模块，而不是写在 `Island.tsx` 里
 *
 * 因为**测试要 import 它**（证明三个形态都存在），而从一个 `.tsx`
 * 里 import 一个 `const enum` 会把 React 一起拉进测试。
 * 更重要的是：**它与 `IslandContent` 一样是"契约"，不是"视图"** ——
 * 契约要能单独被引用与核对。
 *
 * ⚠️ **取值必须与 `crates/qul-core/src/island.rs` 的 `IslandView` 一致。**
 * 有一条测试断言这三个字符串就是那三个（改 Rust 侧而忘改这里会被测出来 ——
 * 虽然它**只能**发现"少了/多了"，发现不了"改错了名"，所以那条测试的
 * 注释里写清了这一点）。
 */
export const IslandView = {
  /** 32 px 胶囊。 */
  Compact: "compact",
  /** ≤ 220 px。 */
  Expanded: "expanded",
  /** 一行状态条（用户点了 ✕）。 */
  Collapsed: "collapsed",
} as const;

export type IslandView = (typeof IslandView)[keyof typeof IslandView];

/** 全部形态，**按规格 §7.3 约束 2 的顺序**。 */
export const ALL_ISLAND_VIEWS: readonly IslandView[] = [
  IslandView.Compact,
  IslandView.Expanded,
  IslandView.Collapsed,
];

/**
 * **进度节流**（§7.3 约束 5）
 * ============================================================================
 *
 * 规格原文：*"进度节流：**≥100 ms 或变化 ≥1% 才刷新**（引擎侧 200 ms +
 * 宿主侧 200 ms 双层，对齐"≤10 次/s"）"*。
 *
 * ## 🔴 它是纯函数，因为"该不该刷新"是一个**判断**
 *
 * 一个把它写在组件里的实现会**没法测** —— 而"节流写错了"的症状是
 * "进度条卡住不动"或"界面掉帧"，两者都很难归因。
 *
 * ## ⚠️ 而"或"在这里是**真的或**
 *
 * 两个条件是**各自独立**能触发刷新的：
 * - 距上次 ≥100 ms ⇒ 刷新（即使进度只涨了 0.1%）
 * - 涨了 ≥1% ⇒ 刷新（即使距上次只有 5 ms）
 *
 * 一个写成 `&&` 的实现会让"下载飞快时界面卡住"（因为每次都不到 100 ms 就被拒），
 * 而那是**最需要刷新的时候**。
 */
export interface ThrottleState {
  /** 上次刷新的时刻（毫秒，单调时钟）。 */
  readonly lastMs: number;
  /** 上次刷新时的进度（0–1）。 */
  readonly lastFraction: number;
}

/** 节流的两个阈值（**它们是规格给的数，不是调优出来的**）。 */
export const THROTTLE_MIN_INTERVAL_MS = 100;
export const THROTTLE_MIN_DELTA = 0.01;

/**
 * 该不该刷新。
 *
 * 返回 `null` ⇒ **不该刷新**（调用方保留上一次的画法）。
 * 返回一个新的 [`ThrottleState`] ⇒ 该刷新了，并把新状态记下来。
 */
export function shouldRefresh(
  prev: ThrottleState | null,
  nowMs: number,
  fraction: number,
): ThrottleState | null {
  // 第一次**总是**刷新 —— 否则界面会停在"没有进度"上。
  if (prev === null) {
    return { lastMs: nowMs, lastFraction: fraction };
  }
  const elapsed = nowMs - prev.lastMs;
  const delta = Math.abs(fraction - prev.lastFraction);
  if (elapsed >= THROTTLE_MIN_INTERVAL_MS || delta >= THROTTLE_MIN_DELTA) {
    return { lastMs: nowMs, lastFraction: fraction };
  }
  return null;
}

/**
 * 时间与百分比的人类可读化（岛上的副文案用它）。
 *
 * ⚠️ **`null` 进来必须给出 `undefined`（= 不显示），而不是 `"0 B/s"`。**
 * 内核的 `speed_bps` / `eta_secs` 是 `Option` 正是这个原因 ——
 * 头几秒样本不足算不出速度，而"0 B/s"与"卡住了"在视觉上无法区分。
 */
export function humanSpeed(bps: number | null | undefined): string | undefined {
  if (bps === null || bps === undefined || bps <= 0) return undefined;
  if (bps >= 1024 * 1024) return `${(bps / (1024 * 1024)).toFixed(1)} MB/s`;
  if (bps >= 1024) return `${(bps / 1024).toFixed(0)} KB/s`;
  return `${bps} B/s`;
}

export function humanEta(secs: number | null | undefined): string | undefined {
  if (secs === null || secs === undefined || secs <= 0) return undefined;
  if (secs >= 3600) return `剩余 ${Math.floor(secs / 3600)} 时 ${Math.floor((secs % 3600) / 60)} 分`;
  if (secs >= 60) return `剩余 ${Math.floor(secs / 60)} 分`;
  return `剩余 ${Math.ceil(secs)} 秒`;
}

export function humanBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(0)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

/**
 * 下载态的副文案：`62% · 1.2 GB/1.9 GB · ↓12.4 MB/s · 剩余 2 分`
 * —— **逐段对应 §7.2 那一行的 Island 内容列**。
 *
 * 而**每一段都可能缺席**（速度与剩余时间在头几秒没有），所以它是
 * "用 `·` 拼起来的有哪些"。
 */
export function downloadDetail(
  doneBytes: number,
  totalBytes: number,
  speedBps: number | null | undefined,
  etaSecs: number | null | undefined,
): string {
  const parts: string[] = [`${humanBytes(doneBytes)}/${humanBytes(totalBytes)}`];
  const sp = humanSpeed(speedBps);
  if (sp !== undefined) parts.push(`↓${sp}`);
  const eta = humanEta(etaSecs);
  if (eta !== undefined) parts.push(eta);
  return parts.join(" · ");
}
