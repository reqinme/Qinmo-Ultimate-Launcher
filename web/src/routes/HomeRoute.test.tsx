/**
 * 主页那条**真的**路径（**"点一下真的调后端"**）
 * ============================================================================
 *
 * ## 🔴 它证的是什么
 *
 * 这一轮之前，主页那条横幅上的按钮是**纯装饰**（`actions` 里没有 `onClick`），
 * 而这一整条链（内核 → `Channel` → `bridge` → `useIslandQueue`）**没有被任何界面
 * 用到**。
 *
 * "那条链在测试里是通的"与"点一下按钮它会跑"**是两件事** ——
 * 而后者才是 M4 那句"全流程一次点通"的意思。
 *
 * 所以这个文件的三条断言是：
 *
 * | # | 它证 |
 * |---|---|
 * | 1 | 按钮点下去**真的调了** `startInstall`，而参数是那个版本号 |
 * | 2 | 忙的时候同一个位置变成"停止"，而它调 `cancelInstall` |
 * | 3 | 前端的失败**会显示出来**（而在浏览器里那是唯一的信息来源） |
 *
 * ## ⚠️ 而它**不**测那条链本身
 *
 * `bridge` / `useIslandQueue` 各有自己的测试。这里测的是**接线** ——
 * 一个"链全对而按钮没接上"的实现在那三处测试里**全绿**。
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

// ⚠️ 路径必须是源码里那一层（`api/index.ts`）—— 组件不该知道 `@tauri-apps/api`。
vi.mock("../api/index.ts", () => ({
  startInstall: vi.fn(),
  cancelInstall: vi.fn(),
}));

import { cancelInstall, startInstall } from "../api/index.ts";
import { HomeRoute } from "./HomeRoute.tsx";
import { IslandProvider } from "../island/IslandProvider.tsx";

const mockedStart = vi.mocked(startInstall);
const mockedCancel = vi.mocked(cancelInstall);

function ui() {
  return render(
    <IslandProvider>
      <HomeRoute />
    </IslandProvider>,
  );
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("主页那条真的路径", () => {
  it("🔴 点「启动 26.3」**真的**调了后端，而版本号是 26.3", async () => {
    mockedStart.mockResolvedValue({
      version: "26.3",
      needed: 76,
      present: 0,
      downloaded: 76,
      migrated: 0,
    });
    ui();

    // ⚠️ `getByRole("button", …)` 而不是 `getByText` —— 后者会把
    // 同一段文字在别处的出现也算进来（那个错误在这个仓库里犯过：
    // `LogDrawer.test.tsx` 里 `/游戏/` 匹配到两个按钮）。
    const btn = screen.getByRole("button", { name: "启动 26.3" });
    // ⚠️ `fireEvent.click` 而不是 `.click()` —— 后者**不会**刷新
    // React 的状态（本仓库踩过）。
    fireEvent.click(btn);

    await waitFor(() => {
      expect(mockedStart).toHaveBeenCalledTimes(1);
    });
    expect(mockedStart.mock.calls[0]?.[0]).toBe("26.3");
  });

  it("🔴 忙的时候**同一个位置**变成「停止」，而它调的是 `cancelInstall`", async () => {
    // 一直不结束 ⇒ 界面停在忙态。
    mockedStart.mockImplementation(() => new Promise(() => {}));
    mockedCancel.mockResolvedValue(true);
    ui();

    fireEvent.click(screen.getByRole("button", { name: "启动 26.3" }));
    // ⚠️ 而这里**必须等**：`busy` 是 `start()` 里同步设的，而 React 的
    // 重渲染是异步的 —— 一个立刻 `getByRole` 的实现会拿到**旧的那个按钮**。
    const stop = await screen.findByRole("button", { name: "停止" });
    // ⚠️ 而这两个按钮**不该同时存在** —— §4.6.8 规则 3：
    // **同一件事一个入口**。并排放两个按钮正是那条规则要禁的形状。
    expect(screen.queryByRole("button", { name: "启动 26.3" })).toBeNull();

    fireEvent.click(stop);
    await waitFor(() => {
      expect(mockedCancel).toHaveBeenCalledTimes(1);
    });
  });

  it("🔴 前端的失败**会显示出来**（浏览器里那是唯一的信息）", async () => {
    // ⚠️ 这一条对应 `useIslandQueue` 里那句注释：失败发生在**前端**时
    //（例如"没有后端"），后端**什么都没发**，于是界面上那一句是
    // **唯一**的信息来源。一个只把错误放进 `run.error` 而不显示的实现
    // 会让用户看到一个**没有任何解释**的界面。
    mockedStart.mockRejectedValue(
      new Error("安装只能从桌面应用里发起（现在是浏览器 / 测试环境，没有后端）。"),
    );
    ui();

    fireEvent.click(screen.getByRole("button", { name: "启动 26.3" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("只能从桌面应用里发起");
  });

  it("成功之后那四个数**真的显示出来**", async () => {
    mockedStart.mockResolvedValue({
      version: "26.3",
      needed: 76,
      present: 70,
      downloaded: 6,
      migrated: 70,
    });
    ui();
    fireEvent.click(screen.getByRole("button", { name: "启动 26.3" }));
    await waitFor(() => {
      expect(screen.getByText(/需要 76 个文件/)).toBeTruthy();
    });
    // ⚠️ **四个数都要出现** —— 只报"总数"会让用户看不出这次装了什么。
    const t = screen.getByText(/需要 76 个文件/).textContent ?? "";
    expect(t).toContain("已有 70");
    expect(t).toContain("下了 6");
    expect(t).toContain("迁移的");
  });
});
