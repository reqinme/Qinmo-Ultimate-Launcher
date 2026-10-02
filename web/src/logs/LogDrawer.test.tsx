/**
 * 日志抽屉的验收测试（M4 主干 · §5.5）
 * ============================================================================
 *
 * ## 它测的重点是那条**最容易被做丢**的规则
 *
 * §5.5 原文：
 *
 * > **日志分享为「本地生成脱敏包 → 用户预览/编辑 → 用户确认后上传」，
 * > 默认不上传**
 *
 * 而它最容易被退化成的样子是**一个"点一下就上传"的按钮** ——
 * 方便、代码更短、而它恰好违反规格。
 *
 * 所以这里把那条规则做成**状态机**，然后用一组断言证明
 * **那三个前置步骤在类型上不可跳过**。
 */

import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import {
  ALL_LOG_SOURCES,
  LogDrawer,
  LogLevel,
  LogSource,
  ShareState,
  canShare,
  type LogLine,
} from "./LogDrawer.tsx";

const LINES: readonly LogLine[] = [
  { source: LogSource.Console, level: LogLevel.Info, text: "启动器：开始部署", atMs: 1 },
  { source: LogSource.Game, level: LogLevel.Warn, text: "游戏：找不到资源包", atMs: 2 },
  { source: LogSource.Game, level: LogLevel.Error, text: "游戏：主类抛异常", atMs: 3 },
  { source: LogSource.Console, level: LogLevel.Debug, text: "启动器：校验通过", atMs: 4 },
];

// ============================================================================
// 来源过滤（§5.5 的原文要求）
// ============================================================================

describe("按来源过滤（§5.5：「否则诊断时两种日志混在一起无法阅读」）", () => {
  it("两个来源都在（`CONSOLE` 与 `LOG4J`）", () => {
    expect([...ALL_LOG_SOURCES].sort()).toEqual(["console", "log4j"]);
  });

  it("默认显示全部，且每行带 `data-source`", () => {
    const { container } = render(<LogDrawer lines={LINES} />);
    expect(container.querySelectorAll(".logdrawer__line").length).toBe(4);
    expect(container.querySelectorAll('[data-source="log4j"]').length).toBe(2);
    expect(container.querySelectorAll('[data-source="console"]').length).toBe(2);
  });

  it("点「游戏」之后**只剩游戏那两条**", () => {
    const { container } = render(<LogDrawer lines={LINES} />);
    // ⚠️ `/游戏/` 会同时命中"结束游戏"那颗钮 —— 所以按**名字与条数**
    // 精确定位筛选钮（它是唯一一个名字里带"游戏"且**带数字**的）。
    fireEvent.click(screen.getByRole("button", { name: /^游戏\s*\d+$/ }));
    expect(container.querySelectorAll(".logdrawer__line").length).toBe(2);
    expect(container.querySelectorAll('[data-source="console"]').length).toBe(0);
  });

  it("而筛选钮上**显示条数**（不然「那边有没有输出」要进去才知道）", () => {
    render(<LogDrawer lines={LINES} />);
    // "启动器" 那一颗上有 2
    const chip = screen.getByRole("button", { name: /启动器/ });
    expect(chip.textContent).toContain("2");
  });

  it("可以指定初始筛选", () => {
    const { container } = render(<LogDrawer lines={LINES} initialSource={LogSource.Console} />);
    expect(container.querySelectorAll(".logdrawer__line").length).toBe(2);
  });

  it("某个来源为空时给一句人话，而不是一片空白", () => {
    render(<LogDrawer lines={[LINES[0] as LogLine]} initialSource={LogSource.Game} />);
    expect(screen.getByText(/还没有输出/)).toBeTruthy();
  });
});

// ============================================================================
// 分级色（§5.5 的五档）
// ============================================================================

describe("五档分级色（§5.5 的令牌表）", () => {
  it("每一档都有 `data-level`（CSS 靠它上色）", () => {
    const { container } = render(<LogDrawer lines={LINES} />);
    for (const lv of ["info", "warn", "error", "debug"]) {
      expect(container.querySelector(`[data-level="${lv}"]`), `缺 ${lv} 档`).not.toBeNull();
    }
  });

  it("五档都在枚举里（多一档少一档都会红）", () => {
    expect(Object.values(LogLevel).sort()).toEqual(["debug", "default", "error", "info", "warn"]);
  });
});

// ============================================================================
// 四个操作
// ============================================================================

describe("四个操作（§5.5：`Clear` / `Copy Log` / `Upload Log` / `Kill Minecraft`）", () => {
  it("清空：调用方拿到回调（组件自己不删数据）", () => {
    let cleared = 0;
    render(<LogDrawer lines={LINES} onClear={() => (cleared += 1)} />);
    fireEvent.click(screen.getByRole("button", { name: "清空" }));
    expect(cleared).toBe(1);
  });

  it("复制：**只复制当前筛选下的那些行**（不是全部）", () => {
    // ⚠️ 一个"复制全部"的实现会让"我筛了游戏那两条，复制出来却混着启动器的"，
    // 而那在贴给别人看的时候是一种隐私问题。
    let copied = "";
    render(<LogDrawer lines={LINES} initialSource={LogSource.Game} onCopy={(t) => (copied = t)} />);
    fireEvent.click(screen.getByRole("button", { name: "复制" }));
    expect(copied).toContain("主类抛异常");
    expect(copied).not.toContain("开始部署");
  });

  it("🔴 结束游戏**要二次确认**，而那一次确认说明了后果", () => {
    let killed = 0;
    render(<LogDrawer lines={LINES} onKill={() => (killed += 1)} />);

    // 第一次点击**不杀**，只是进入确认态
    fireEvent.click(screen.getByRole("button", { name: "结束游戏" }));
    expect(killed, "第一次点击不该真的结束进程").toBe(0);

    // 而确认态里**说明了后果**（不是一句"确定吗"）
    expect(screen.getByText(/未保存的进度会丢/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "确认结束" }));
    expect(killed).toBe(1);
  });

  it("二次确认可以取消，而取消**不杀**", () => {
    let killed = 0;
    render(<LogDrawer lines={LINES} onKill={() => (killed += 1)} />);
    fireEvent.click(screen.getByRole("button", { name: "结束游戏" }));
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(killed).toBe(0);
    // 而它回到了未确认的那一态
    expect(screen.getByRole("button", { name: "结束游戏" })).toBeTruthy();
  });

  it("没有 `onKill` 时那颗钮**是禁用的**（不给一个点了没反应的按钮）", () => {
    render(<LogDrawer lines={LINES} />);
    expect((screen.getByRole("button", { name: "结束游戏" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });
});

// ============================================================================
// 🔴 分享：三个前置步骤**在类型上不可跳过**
// ============================================================================

describe("分享状态机（§5.5：「默认不上传」）", () => {
  it("🔴 **只有 `Confirmed` 能上传** —— 这是那条规格的类型级落点", () => {
    // ⚠️ **这条测试是本文件存在的理由。**
    //
    // 最容易退化成"点一下就上传"，而这里断言的是：
    // `Idle` / `Generated` / `Previewing` 三个状态下**都不能**上传。
    expect(canShare(ShareState.Idle, "upload")).toBe(false);
    expect(canShare(ShareState.Generated, "upload")).toBe(false);
    expect(canShare(ShareState.Previewing, "upload")).toBe(false);
    expect(canShare(ShareState.Confirmed, "upload")).toBe(true);
  });

  it("六个状态都在（而**没有**「直接上传」那一个）", () => {
    const states = Object.values(ShareState);
    // ⚠️ **不断言顺序**（`Object.values` 的顺序不是契约），而断言
    // "**恰好这六个**" —— 一个多出 `direct`（"直接上传"）的版本会在这里红。
    expect([...states].sort()).toEqual([
      "confirmed",
      "done",
      "generated",
      "idle",
      "previewing",
      "uploading",
    ]);
    expect(new Set(states).size, "六个状态应当互不相同").toBe(6);
  });

  it("生成只在 `Idle` 允许（不能重复生成覆盖用户改过的内容）", () => {
    expect(canShare(ShareState.Idle, "generate")).toBe(true);
    for (const s of [ShareState.Generated, ShareState.Previewing, ShareState.Confirmed]) {
      expect(canShare(s, "generate"), `${s} 不该能再生成`).toBe(false);
    }
  });

  it("🔴 **上传中不能编辑**（否则上传的不是他确认过的那一份）", () => {
    expect(canShare(ShareState.Generated, "edit")).toBe(true);
    expect(canShare(ShareState.Previewing, "edit")).toBe(true);
    expect(canShare(ShareState.Confirmed, "edit")).toBe(false);
    expect(canShare(ShareState.Uploading, "edit")).toBe(false);
  });
});

describe("分享面板的界面行为", () => {
  it("🔴 **初始不显示上传**（`Idle` 时整个分享面板都不存在）", () => {
    // 一个"默认就把上传按钮摆在那"的实现会让用户以为那是常规动作。
    render(<LogDrawer lines={LINES} />);
    expect(screen.queryByRole("button", { name: "上传" })).toBeNull();
  });

  it("点生成 ⇒ 出现可编辑的内容 + 两个按钮，而**上传那时是禁用的**", () => {
    render(<LogDrawer lines={LINES} onGenerateShare={() => "脱敏后的内容"} />);
    fireEvent.click(screen.getByRole("button", { name: "生成诊断包" }));

    const ta = screen.getByRole("textbox", { name: /诊断包内容/ }) as HTMLTextAreaElement;
    expect(ta.value).toBe("脱敏后的内容");
    expect(ta.readOnly).toBe(false);
    // ⚠️ **这时上传必须是禁用的** —— 用户还没确认。
    expect((screen.getByRole("button", { name: "上传" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("确认之后上传才可用", () => {
    render(<LogDrawer lines={LINES} onGenerateShare={() => "x"} />);
    fireEvent.click(screen.getByRole("button", { name: "生成诊断包" }));
    fireEvent.click(screen.getByRole("button", { name: /我已检查/ }));
    expect((screen.getByRole("button", { name: "上传" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("用户编辑过之后**又回到「未确认」**（改了就得更一次确认）", () => {
    // ⚠️ 这条是"上传的是他确认过的那一份"的实质：改过内容之后
    // 上一次的确认**不再有效**。
    render(<LogDrawer lines={LINES} onGenerateShare={() => "x"} />);
    fireEvent.click(screen.getByRole("button", { name: "生成诊断包" }));
    // ⚠️ **先改、再确认、再断言。**
    //
    // 而我第一版写的是"先确认再改" —— 那**测不出来**，因为
    // **确认之后 textarea 变成只读**（那是刻意的：一个"确认之后还能改"
    // 的实现会让「上传的是他确认过的那一份」这句话不成立）。
    fireEvent.change(screen.getByRole("textbox", { name: /诊断包内容/ }), {
      target: { value: "第一次改" },
    });
    // 改过之后**还没确认** ⇒ 上传不可用
    expect((screen.getByRole("button", { name: "上传" }) as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: /我已检查/ }));
    // 确认了 ⇒ 上传可用
    expect((screen.getByRole("button", { name: "上传" }) as HTMLButtonElement).disabled).toBe(false);

    // **而确认之后 textarea 应当是只读的** —— 否则「确认」没有约束力。
    expect(
      (screen.getByRole("textbox", { name: /诊断包内容/ }) as HTMLTextAreaElement).readOnly,
    ).toBe(true);
    });

  it("放弃之后回到初始（分享面板消失）", () => {
    render(<LogDrawer lines={LINES} onGenerateShare={() => "x"} />);
    fireEvent.click(screen.getByRole("button", { name: "生成诊断包" }));
    fireEvent.click(screen.getByRole("button", { name: "放弃" }));
    expect(screen.queryByRole("button", { name: "上传" })).toBeNull();
  });

  it("面板上**明说**「默认不会上传」", () => {
    // 一句话的成本很低，而它是"用户知道自己没在上传"的唯一来源。
    render(<LogDrawer lines={LINES} onGenerateShare={() => "x"} />);
    fireEvent.click(screen.getByRole("button", { name: "生成诊断包" }));
    expect(screen.getByText(/默认不会上传/)).toBeTruthy();
  });
});
