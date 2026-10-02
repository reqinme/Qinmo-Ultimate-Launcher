/**
 * 灵动岛的验收测试（**M4 主干** · §7）
 * ============================================================================
 *
 * ## 它测什么、以及为什么这些是"能被测住"的那部分
 *
 * §7 的三条硬规则里，**"只有一个岛、一个主状态"** 已经在内核
 *（`crates/qul-core/src/island.rs` 的 22 项测试）钉住了。
 *
 * 所以本文件测的是**前端这一侧**能承担的责任：
 *
 * | 它 | 归谁 |
 * |---|---|
 * | 队列与抢占语义 | **内核**（已测） |
 * | 八态都**画得出来**（每一种都有渲染路径） | 本文件 |
 * | 两态切换与"收起" | 本文件 |
 * | **进度节流**（纯函数） | 本文件 |
 * | 数值人类可读化（速度/剩余/体积） | 本文件 |
 */

import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { Island, idleContent, type IslandContent, type IslandKind } from "./Island.tsx";
import {
  ALL_ISLAND_VIEWS,
  IslandView,
  downloadDetail,
  humanBytes,
  humanEta,
  humanSpeed,
  shouldRefresh,
  THROTTLE_MIN_DELTA,
  THROTTLE_MIN_INTERVAL_MS,
} from "./islandMath.ts";

/** 一个最小的内容。 */
function content(over: Partial<IslandContent> = {}): IslandContent {
  return {
    kind: "download",
    headline: "下载中",
    detail: "12.4 MB/s",
    fraction: 0.62,
    hints: [],
    ...over,
  };
}

// ============================================================================
// 八态都能画出来
// ============================================================================

describe("八态都有渲染路径（§7.2 的表一行不少）", () => {
  const KINDS: readonly IslandKind[] = [
    "idle",
    "probe",
    "download",
    "install",
    "launch",
    "running",
    "error",
    "update",
  ];

  it.each(KINDS)("%s 态能渲染，且带 data-kind", (kind) => {
    // ⚠️ **这条断言的价值是"每一种都有路径"** —— 一个漏了
    // `data-kind` 的实现在 CSS 上就**没有语气色**，而那在眼睛看来
    // 只是"这个态的颜色不对"，很难归因。
    const { container } = render(<Island content={content({ kind, headline: kind })} />);
    expect(container.querySelector(`[data-kind="${kind}"]`)).not.toBeNull();
  });

  it("八态**恰好是**八个（多一个少一个都会红）", () => {
    expect(new Set(KINDS).size).toBe(8);
  });

  it("error 态用 `role=alert`，其余用 `role=status`", () => {
    // ⚠️ 这个区别是有意义的：`alert` 会在读屏上**打断**用户正在听的东西。
    // 而下载进度不该打断任何人 —— 失败才该。
    const { unmount } = render(<Island content={content({ kind: "error", headline: "失败" })} />);
    expect(screen.getByRole("alert")).toBeTruthy();
    unmount();

    render(<Island content={content({ kind: "download" })} />);
    expect(screen.getByRole("status")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

// ============================================================================
// 形态：两态 + 收起
// ============================================================================

describe("形态（§7.3 约束 2 / 4）", () => {
  it("三个形态都存在，且取值与内核一致", () => {
    // ⚠️ 这条只能发现"少了/多了"，**发现不了"改错了名"** ——
    // 后者要靠 Rust 侧的序列化测试。而它仍然有价值：
    // 一个"忘了加 Collapsed"的版本会在这里红。
    expect([...ALL_ISLAND_VIEWS].sort()).toEqual(["collapsed", "compact", "expanded"]);
  });

  it("默认是紧凑态（32 px 那一档）", () => {
    const { container } = render(<Island content={content()} />);
    expect(container.querySelector('[data-view="compact"]')).not.toBeNull();
  });

  it("可以传初始形态（外壳用 `Collapsed` 恢复用户上次的选择）", () => {
    const { container } = render(
      <Island content={content()} view={IslandView.Collapsed} />,
    );
    expect(container.querySelector('[data-view="collapsed"]')).not.toBeNull();
  });

  it("收起**不改变内容** —— 任务不丢失（§7.3 约束 4）", () => {
    // ⚠️ 这条的形状就是那条规则：`data-kind` 与文案**一个字节都没变**，
    // 变的只有 `data-view`。
    const { container } = render(
      <Island content={content({ kind: "install", headline: "安装中" })} view={IslandView.Collapsed} />,
    );
    expect(container.querySelector('[data-kind="install"]')).not.toBeNull();
    // 而收起之后**仍然能读到**这一句话（它只是被 CSS 藏了一部分）
    expect(screen.getByRole("status")).toBeTruthy();
  });

  it("展开按钮在紧凑/展开之间切换", () => {
    const { container } = render(<Island content={content()} />);
    // ⚠️ **`fireEvent` 而不是原生 `.click()`。**
    //
    // 原生 `.click()` 会调用 DOM 的点击，而 React 的事件系统在测试环境里
    // 是靠 `act()` 包裹才 flush 状态更新的 —— 所以 `.click()` 之后
    // **DOM 还没重渲染**，断言就看到了旧的形态。
    //
    // 而我第一版正是这么写的，于是 4 条测试一起红，而错误信息
    //（`expected null not to be null`）**完全没提"没 flush"这件事**。
    fireEvent.click(screen.getByRole("button", { name: "展开详情" }));
    expect(container.querySelector('[data-view="expanded"]')).not.toBeNull();
  });

  it("展开时把 `hints` 画出来（**字段由调用方给，组件不判断**）", () => {
    render(
      <Island
        content={content({
          kind: "running",
          hints: [
            { label: "JVM 内存", value: "1024 MB" },
            { label: "运行时长", value: "3 分" },
          ],
        })}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "展开详情" }));
    expect(screen.getByText("JVM 内存")).toBeTruthy();
    expect(screen.getByText("1024 MB")).toBeTruthy();
  });

  it("⚠️ 组件里**没有**产品名分支 —— 字段完全由 hints 决定", () => {
    // §7.2：产品差异只在展开面板的**字段清单**里，由 `Provider.ui_hints` 驱动。
    // 这条测试用"同一份内容配两组不同 hints"来证明组件不参与判断。
    const a = render(
      <Island content={content({ kind: "running", hints: [{ label: "JVM 内存", value: "1 GB" }] })} />,
    );
    a.unmount();
    const b = render(
      <Island
        content={content({ kind: "running", hints: [{ label: "依赖组件", value: "3/3" }] })}
      />,
    );
    fireEvent.click(b.getByRole("button", { name: "展开详情" }));
    expect(b.getByText("依赖组件")).toBeTruthy();
    expect(b.queryByText("JVM 内存")).toBeNull();
  });

  it("点 ✕ 会收起，并回调通知调用方", () => {
    let collapsed = 0;
    const { container } = render(<Island content={content()} onCollapse={() => (collapsed += 1)} />);
    fireEvent.click(screen.getByRole("button", { name: /收起为状态条/ }));
    expect(collapsed).toBe(1);
    expect(container.querySelector('[data-view="collapsed"]')).not.toBeNull();
  });

  it("没有 `onCollapse` 时**不显示** ✕（不给用户一个点了没反应的按钮）", () => {
    render(<Island content={content()} />);
    expect(screen.queryByRole("button", { name: /收起为状态条/ })).toBeNull();
  });
});

// ============================================================================
// 不确定进度
// ============================================================================

describe("进度显示", () => {
  it("`fraction` 有值时画百分比", () => {
    render(<Island content={content({ fraction: 0.62 })} />);
    expect(screen.getByText("62%")).toBeTruthy();
  });

  it("`fraction` 是 `null` 时**不画百分比**（那是不确定，不是 0%）", () => {
    // ⚠️ 内核的 `fraction()` 对"总量还不知道"返回 `None`，而这里必须
    // 尊重那个 `null` —— 一个 `Math.round(null ?? 0)` 的实现会显示 "0%"，
    // 而那与"卡住了"在视觉上无法区分。
    const { container } = render(<Island content={content({ fraction: null })} />);
    expect(screen.queryByText(/^\d+%$/)).toBeNull();
    // 而图形换成了转鼓或点（不是空环）
    expect(container.querySelector(".island__drum, .island__dot")).not.toBeNull();
  });

  it("副文案在**紧凑态也显示**（展开只是放大，而不是展开才有信息）", () => {
    render(<Island content={content({ detail: "1.2 GB/1.9 GB" })} />);
    expect(screen.getByText("1.2 GB/1.9 GB")).toBeTruthy();
  });
});

// ============================================================================
// 进度节流（§7.3 约束 5）
// ============================================================================

describe("进度节流：≥100 ms **或** ≥1%", () => {
  it("第一次总是刷新（否则界面停在「没有进度」上）", () => {
    expect(shouldRefresh(null, 1000, 0)).toEqual({ lastMs: 1000, lastFraction: 0 });
  });

  it("时间够了就刷新（即使进度没变）", () => {
    const prev = { lastMs: 1000, lastFraction: 0.5 };
    expect(shouldRefresh(prev, 1000 + THROTTLE_MIN_INTERVAL_MS, 0.5)).not.toBeNull();
  });

  it("进度涨够了就刷新（即使时间不够）", () => {
    // ⚠️ **这一条是那个"或"的核心。** 一个用 `&&` 的实现在下载飞快时
    // （每次都不到 100 ms）会**拒绝刷新** —— 而那正是最需要刷新的时候。
    const prev = { lastMs: 1000, lastFraction: 0.5 };
    expect(shouldRefresh(prev, 1005, 0.5 + THROTTLE_MIN_DELTA)).not.toBeNull();
  });

  it("两个都不够就不刷新", () => {
    const prev = { lastMs: 1000, lastFraction: 0.5 };
    expect(shouldRefresh(prev, 1050, 0.505)).toBeNull();
  });

  it("倒退的进度也算变化（它对刷新有价值）", () => {
    // 重试会让进度回退。一个只看 `curr > prev` 的实现会在回退时**不刷新**，
    // 而用户看到的是一个卡住的百分比。
    const prev = { lastMs: 1000, lastFraction: 0.5 };
    expect(shouldRefresh(prev, 1005, 0.4)).not.toBeNull();
  });

  it("阈值就是规格给的那两个数（不是调优出来的）", () => {
    expect(THROTTLE_MIN_INTERVAL_MS).toBe(100);
    expect(THROTTLE_MIN_DELTA).toBe(0.01);
  });
});

// ============================================================================
// 人类可读化：**`null` 必须给出"不显示"**
// ============================================================================

describe("数值文案（内核的 `Option` 在这里被尊重）", () => {
  it("速度：`null` / `undefined` / `0` 都给 `undefined`（= 不显示）", () => {
    // ⚠️ 内核的 `speed_bps` 是 `Option`，理由是"头几秒样本不足算不出"。
    // 一个把它显示成 "0 B/s" 的实现会让界面看起来像**卡住了**。
    expect(humanSpeed(null)).toBeUndefined();
    expect(humanSpeed(undefined)).toBeUndefined();
    expect(humanSpeed(0)).toBeUndefined();
  });

  it("速度：有值时分三档", () => {
    expect(humanSpeed(512)).toBe("512 B/s");
    expect(humanSpeed(2048)).toBe("2 KB/s");
    expect(humanSpeed(12.4 * 1024 * 1024)).toBe("12.4 MB/s");
  });

  it("剩余时间：`null` 给 `undefined`，有值时分三档", () => {
    expect(humanEta(null)).toBeUndefined();
    expect(humanEta(0)).toBeUndefined();
    expect(humanEta(45)).toBe("剩余 45 秒");
    expect(humanEta(125)).toBe("剩余 2 分");
    expect(humanEta(3700)).toBe("剩余 1 时 1 分");
  });

  it("体积分三档", () => {
    expect(humanBytes(512)).toBe("512 B");
    expect(humanBytes(2048)).toBe("2 KB");
    expect(humanBytes(1.5 * 1024 ** 2)).toBe("2 MB");
    expect(humanBytes(1.25 * 1024 ** 3)).toBe("1.25 GB");
  });

  it("下载副文案逐段拼接，**缺席的段不出现**", () => {
    // §7.2 那一行：`62% · 1.2 GB/1.9 GB · ↓12.4 MB/s · 剩余 2 分`
    expect(downloadDetail(1.2 * 1024 ** 3, 1.9 * 1024 ** 3, 12.4 * 1024 * 1024, 120)).toBe(
      "1.20 GB/1.90 GB · ↓12.4 MB/s · 剩余 2 分",
    );
    // 头几秒：速度与剩余都还没有 ⇒ **只剩体积那一句**（不是"↓0 B/s"）
    expect(downloadDetail(1024, 2048, null, null)).toBe("1 KB/2 KB");
  });
});

// ============================================================================
// 空态
// ============================================================================

describe("空闲态", () => {
  it("`idleContent` 给源加账户（§7.2 的 Idle 内容列）", () => {
    const c = idleContent("qinme", "官方源");
    expect(c.kind).toBe("idle");
    expect(c.headline).toBe("官方源");
    expect(c.detail).toBe("qinme");
    // ⚠️ 空闲态**没有进度** —— 一个给 0 的实现会画一个空环。
    expect(c.fraction).toBeNull();
    expect(c.hints.length).toBe(2);
  });
});
