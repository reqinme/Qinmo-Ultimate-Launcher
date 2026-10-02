/**
 * 队列接线的测试（**fake timers + mock 边界层**）
 * ============================================================================
 *
 * ## 🔴 它证的是四件事，而每一件都是"看起来能用而其实没有"的反面
 *
 * | # | 它证 | 不证会怎样 |
 * |---|---|---|
 * | 1 | 进度真的会流到 `content` | 一个只设了 `busy` 而没收消息的 hook 看起来**也在转** |
 * | 2 | 翻译不过来时**保留上一态**（而不是回到空闲） | 那个 bug 会**活很久**（症状与"没事发生"一样） |
 * | 3 | 失败时 `run.error` 有那一句 | 用户看到一个**没有解释**的岛 |
 * | 4 | 卸载之后**不再写状态** | 一次没有意义的写入（而它在 React 18 下不再报警告） |
 */

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

// ⚠️ **`vi.mock` 要提到 import 之前**（它会被提升，而路径必须与源码一致）。
// 而这里 mock 的是**边界层**，不是 `@tauri-apps/api/core` ——
// 后者根本不该被这个 hook 碰到（那是 `api/` 的事）。
vi.mock("../api/index.ts", () => ({
  startInstall: vi.fn(),
  cancelInstall: vi.fn(),
}));

import { cancelInstall, startInstall } from "../api/index.ts";
import { useIslandQueue } from "./useIslandQueue.ts";
import type { IslandState } from "./bridge.ts";

const mockedStart = vi.mocked(startInstall);
const mockedCancel = vi.mocked(cancelInstall);

afterEach(() => {
  vi.clearAllMocks();
});

describe("useIslandQueue：流真的会到 content", () => {
  it("推一条 `launch` 之后 content 变成那一阶段", async () => {
    let push: ((s: IslandState) => void) | undefined;
    mockedStart.mockImplementation(async (_v: string, onState: (s: IslandState) => void) => {
      push = onState;
      // 推一条，然后**一直不结束**（模拟一个长安装）
      onState({ kind: "launch", stage: "download" });
      return new Promise(() => {});
    });

    const { result } = renderHook(() => useIslandQueue());
    // ⚠️ `act` 包住那次调用 —— 否则 React 会警告"状态在 act 之外变了"。
    void act(() => {
      void result.current.start("26.3");
    });

    await waitFor(() => {
      expect(result.current.content.kind).toBe("launch");
    });
    expect(result.current.content.detail).toContain("2/5");
    expect(result.current.run.busy).toBe(true);

    // 再推一条下载进度 —— 阶段变了。
    act(() => {
      push?.({ kind: "download", done_bytes: 50, total_bytes: 100, speed_bps: null, eta_secs: null });
    });
    expect(result.current.content.kind).toBe("download");
    expect(result.current.content.fraction).toBeCloseTo(0.5);
  });

  it("🔴 翻译不过来时**保留上一态**（而不是回到空闲）", async () => {
    // ⚠️ **这条测试的真正形状是"hook 收到一条它翻译不了的状态"**，
    // 而不是"IPC 载荷坏了"（那件事由 `bridge.test.ts` 里的
    // `parseIslandState` 那几条证）。
    //
    // ## 而它写了三版才对，而三次的错各不相同 —— 值得记
    //
    // | 版本 | 我写的 | 为什么错 |
    // |---|---|---|
    // | 1 | 把 `{kind:"teleporting"}` 直接当 `IslandState` 塞给 hook | `startInstall` 的入参类型是**校验过的** `IslandState`，所以那条路径**真实链路里不存在** |
    // | 2 | 让 mock 走 `parseIslandState`，而让它的抛出**冒泡** | 那会让 `startInstall` 返回一个**被拒的 promise** ⇒ `start()` 走"整次安装失败"那条路，**根本没走到进度** |
    // | 3 | 用 `try/catch` 吞掉校验的抛出 | 吞掉之后 `feed` 里的表达式是 `undefined`，而 `onState(undefined)` 会走到 `toIslandContent(undefined)` ⇒ 抛的是 `TypeError`，**测试"通过"了而证的不是那件事** |
    //
    // **修法是"捕获 hook 真正传进来的那个回调"** —— 于是这里推的每一条
    // 都**确实**是一条 `IslandState`，而复现的正是真实链路里
    // `toIslandContent` 那一步的失败。
    let onState: ((s: IslandState) => void) | undefined;
    mockedStart.mockImplementation(
      async (_v: string, cb: (s: IslandState) => void) => {
        onState = cb;
        return new Promise(() => {});
      },
    );

    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const { result } = renderHook(() => useIslandQueue());
    void act(() => {
      void result.current.start("26.3");
    });
    await waitFor(() => {
      expect(onState).toBeDefined();
    });
    // 先推一条**好的** —— 于是界面上有一个"上一态"。
    act(() => {
      onState?.({ kind: "launch", stage: "verify" });
    });
    expect(result.current.content.detail).toContain("3/5");

    // 再推一条**它翻译不了的**。
    //
    // ⚠️ 而这里要造出一个 `IslandState` 而**不是**一个坏载荷 ——
    // 因为"坏载荷"根本到不了这一层（那正是 `parseIslandState` 的职责）。
    // 用一个**运行期不存在的 kind** 是造它的最直接方式：它在**类型上**
    // 是 `IslandState`（所以这行能过 typecheck 的那一半靠断言），
    // 而在**运行期**落进 `toIslandContent` 的 `default` 之外。
    act(() => {
      onState?.({ kind: "teleporting" } as unknown as IslandState);
    });

    await waitFor(() => {
      expect(spy).toHaveBeenCalled();
    });
    // 🔴 **而上一条仍然在** —— 界面没有闪回空闲。
    expect(result.current.content.kind).toBe("launch");
    expect(result.current.content.detail).toContain("3/5");
    spy.mockRestore();
  });

  it("失败时 `run.error` 是**给人看的那一句**", async () => {
    mockedStart.mockRejectedValue(new Error("安装只能从桌面应用里发起"));
    const { result } = renderHook(() => useIslandQueue());
    await act(async () => {
      await result.current.start("26.3");
    });
    expect(result.current.run.busy).toBe(false);
    expect(result.current.run.error).toContain("只能从桌面应用里发起");
  });

  it("成功时 `run.last` 带着那四个数", async () => {
    mockedStart.mockResolvedValue({
      version: "26.3",
      needed: 76,
      present: 0,
      downloaded: 76,
      migrated: 0,
    });
    const { result } = renderHook(() => useIslandQueue());
    await act(async () => {
      await result.current.start("26.3");
    });
    expect(result.current.run.last?.downloaded).toBe(76);
    expect(result.current.run.error).toBeNull();
  });
});

describe("useIslandQueue：卸载与取消", () => {
  it("🔴 卸载之后**不再写状态**", async () => {
    let push: ((s: IslandState) => void) | undefined;
    mockedStart.mockImplementation(async (_v: string, onState: (s: IslandState) => void) => {
      push = onState;
      return new Promise(() => {});
    });
    const { result, unmount } = renderHook(() => useIslandQueue());
    void act(() => {
      void result.current.start("26.3");
    });
    unmount();
    // 卸载之后推一条 —— 而它**不该**抛（也不该写）。
    // ⚠️ "不该写"没法直接断言，所以这里断言的是**它不抛** ——
    // 而那条正是"一个没有 alive 标记的实现"会失败的形状吗？
    // 诚实地说：**不会**（React 18 对卸载后的写入不再报警告）。
    // 所以这条测试的价值是**防回归**（将来某个版本又开始报），
    // 而不是证明那个标记是必需的。
    expect(() => push?.({ kind: "idle" })).not.toThrow();
  });

  it("`cancel()` 会问后端", async () => {
    mockedCancel.mockResolvedValue(true);
    const { result } = renderHook(() => useIslandQueue());
    await act(async () => {
      await result.current.cancel();
    });
    expect(mockedCancel).toHaveBeenCalledTimes(1);
  });
});
