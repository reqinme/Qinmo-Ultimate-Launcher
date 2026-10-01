/**
 * 数据层贯通示例的验收测试（**M4 门禁第 ④ 项**）
 * ============================================================================
 *
 * ## 它证明"一条数据从桩到界面"的每一段都在位
 *
 * ```text
 *   api/index.ts 的边界层（含契约校验）
 *      ↓
 *   api/query.ts 的查询键
 *      ↓
 *   TanStack Query 的三种状态
 *      ↓
 *   组件只渲染
 * ```
 *
 * ## 🔴 而它测的重点是**两条容易被跳过的路**
 *
 * 一个只测"成功时渲染出了什么"的测试会让**加载态与错误态**变成
 * 没有地方验证的东西 —— 而那两条恰好是最容易在真实环境里出问题的。
 * 所以下面**三条路都有**，且错误态那条会核对**原文被显示出来**
 *（一个把错误归纳成"加载失败"的实现会让诊断少掉最关键的一行）。
 */

import { describe, expect, it, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { CapabilitiesQueryPage } from "./CapabilitiesQueryPage.tsx";
import { setBackend, stubBackend, fetchCapabilities } from "../api/index.ts";
import { CapabilityContractError, parseCapabilities } from "../api/contract.ts";
import { qk } from "../api/query.ts";

/**
 * ⚠️ **每个测试用一个新的 `QueryClient`。**
 *
 * 共用一个会让上一个测试的缓存**在下一次断言里出现**，
 * 而那类失败的症状是"单独跑通过、一起跑失败"。
 */
function wrap(ui: ReactElement): ReactElement {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return <QueryClientProvider client={client}>{ui}</QueryClientProvider>;
}

afterEach(() => {
  // 把后端恢复成默认桩，免得一个测试的注入影响下一个。
  setBackend(stubBackend());
});

describe("门禁④：数据层贯通", () => {
  it("成功态：能力表的结论被画出来了（含**带原因**的禁用态）", async () => {
    setBackend(stubBackend());
    render(wrap(<CapabilitiesQueryPage />));

    // 加载态**先**出现 —— 它必须是一个真实存在的分支
    expect(await screen.findByRole("heading", { name: /能力表/ })).toBeTruthy();

    await waitFor(() => {
      expect(screen.getByText(/该形态不支持光影/)).toBeTruthy();
    });
    // 而另一条禁用原因也在（两条，验证它不是偶然画出来的一个）
    expect(screen.getByText(/与官方启动器共用账户与数据/)).toBeTruthy();
  });

  it("错误态：**原文**被显示出来，而不是被归纳成一句「加载失败」", async () => {
    setBackend(stubBackend({ fail: true }));
    render(wrap(<CapabilitiesQueryPage />));

    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeTruthy();
    });
    // ⚠️ 断言的是**原文**。一个把错误归纳掉的实现会让诊断
    // 少掉最关键的那一行，而"少一行"在界面上看不出来。
    expect(screen.getByText(/后端不可用（桩：这是被注入的失败）/)).toBeTruthy();
    // 而它必须提供重试 —— 一个只显示错误不给出路的界面是死路
    expect(screen.getByRole("button", { name: "重试" })).toBeTruthy();
  });

  it("加载态：有一个 `aria-busy` 的区域（读屏也要能知道它在等）", async () => {
    setBackend(stubBackend({ delayMs: 50 }));
    render(wrap(<CapabilitiesQueryPage />));

    // 立刻查：此时应当还在 pending
    const busy = document.querySelector('[aria-busy="true"]');
    expect(busy, "加载态应当有一个 aria-busy 的区域").not.toBeNull();

    // 而它最终会变成成功态（证明延迟只是延迟，不是卡死）
    await waitFor(
      () => {
        expect(screen.getByText(/该形态不支持光影/)).toBeTruthy();
      },
      { timeout: 3000 },
    );
  });
});

describe("边界层的不变式（**两侧都守**）", () => {
  it("禁用态缺少原因时抛 —— 前端也守一次", () => {
    // Rust 的类型系统管不到穿越 IPC 的 JSON。这是边界校验，不是重复劳动。
    expect(() =>
      parseCapabilities({ mods: { enabled: false } }),
    ).toThrow(CapabilityContractError);
  });

  it("空原因（只有空白）也算缺原因", () => {
    // 一个用 `reason !== undefined` 判定的实现会让 `"   "` 通过 ——
    // 而那在界面上是一个**看起来像没写原因的**禁用态。
    expect(() => parseCapabilities({ mods: { enabled: false, reason: "   " } })).toThrow(
      CapabilityContractError,
    );
  });

  it("可用态**不需要**原因", () => {
    expect(() => parseCapabilities({ mods: { enabled: true } })).not.toThrow();
  });

  it("非对象（数组 / null / 标量）被拒", () => {
    for (const bad of [[], null, "x", 3] as const) {
      expect(() => parseCapabilities(bad), `${String(bad)} 应当被拒`).toThrow(
        CapabilityContractError,
      );
    }
  });

  it("`fetchCapabilities` 走的是边界层（于是校验一定被执行）", async () => {
    // ⚠️ 注入一个"契约被破坏"的后端 —— 校验**必须**把它拦住。
    // 一个绕过 `parseCapabilities` 的取数路径会在这里静默通过。
    setBackend({
      capabilitiesOf: async () =>
        // 故意绕过类型：模拟"后端给了脏 JSON"
        ({ mods: { enabled: false } }) as unknown as Awaited<
          ReturnType<typeof fetchCapabilities>
        >,
      instanceSummary: async (id: string) => ({ id, name: id }),
    });
    await expect(fetchCapabilities()).rejects.toThrow(CapabilityContractError);
  });
});

describe("查询键（**唯一构造处**）", () => {
  it("键里用的是内核的标识，而不是界面编的字符串", () => {
    // 规格 §4.5："缓存 key 直接用实例 id / 版本 id，与内核的实例模型一一对应"
    expect(qk.instance("abc")).toEqual(["instance", "abc"]);
    expect(qk.instanceCapabilities("abc")).toEqual(["instance", "abc", "capabilities"]);
    // ⚠️ 能力表**按实例**算 —— 两个实例的键必须不同
    expect(qk.instanceCapabilities("a")).not.toEqual(qk.instanceCapabilities("b"));
    // 而版本清单**与实例无关**，所以它的键里没有实例 id
    expect(qk.versionManifest()).toEqual(["versions", "manifest"]);
  });

  it("同一个 id 两次得到**结构相等**的键（否则缓存永远不命中）", () => {
    expect(qk.instance("x")).toEqual(qk.instance("x"));
  });
});
