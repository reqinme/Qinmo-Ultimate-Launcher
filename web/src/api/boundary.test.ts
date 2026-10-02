/**
 * 边界层与**命令层门禁**的测试（**M1 剩余项的运行时那一半**）
 * ============================================================================
 *
 * ## 🔴 它证的是什么
 *
 * §5.8 说：*"'前端零业务逻辑'是**设计意图**，不是**安全边界**。
 * WebView2 里的页面一旦被注入内容，攻击面就是**整个 Tauri 命令层**。
 * 所以必须有强制手段。"*
 *
 * 而"强制手段"在这里是**三件**，本文件逐件证明：
 *
 * | # | 手段 | 本文件怎么证 |
 * |---|---|---|
 * | 1 | `@tauri-apps/api/core` **只允许在 `api/` 下被 import** | 遍历 `web/src` 的每个文件，核对 |
 * | 2 | **IPC 载荷必须在边界上校验** | 注入坏载荷，断言它抛 |
 * | 3 | **非 Tauri 环境下不抛**（而是明确回退） | 断言 `activeBackend()` 给出桩 |
 *
 * ⚠️ **这一条文件跑在 `jsdom` 下** —— 因为 `isTauri()` 要读
 * `window.__TAURI_INTERNALS__`，而 `node` 环境下没有 `window`。
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { activeBackend, parseInstanceSummary } from "./tauriBackend.ts";
import { backend, fetchCapabilities, setBackend, stubBackend } from "./index.ts";
import { CapabilityContractError, parseCapabilities } from "./contract.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const SRC = join(HERE, "..");

/** 递归列出 `web/src` 下所有 .ts / .tsx。 */
function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) {
      out.push(...sourceFiles(p));
    } else if (name.endsWith(".ts") || name.endsWith(".tsx")) {
      out.push(p);
    }
  }
  return out;
}

// ============================================================================
// 🔴 门禁 1：「组件里连拿到 `invoke` 的机会都不该有」
// ============================================================================

describe("命令层门禁：`@tauri-apps/api/core` 只允许在 `api/` 下被 import", () => {
  it("🔴 除 `api/` 之外**一个文件都不 import 它**", () => {
    // ⚠️ **这是"前端零业务逻辑"从设计意图变成机制的那一步。**
    //
    // `eslint.config.js` 里有同一条规则（S4 护栏），而**这里再证一次**，
    // 因为两件事不同：
    //   - eslint 那条在**开发时**拦（有人写错的当下）
    //   - 这条在**测试时**拦（有人改了 eslint 配置或加了豁免之后）
    //
    // 一条只在 lint 里的规则，会在"给某个文件加一行 eslint-disable"时**静默失效**。
    const offenders: string[] = [];
    for (const f of sourceFiles(SRC)) {
      const rel = relative(SRC, f).replace(/\\/g, "/");
      // 测试文件自身允许（它要 import 被测对象）
      if (rel.endsWith(".test.ts") || rel.endsWith(".test.tsx")) continue;
      const text = readFileSync(f, "utf8");
      if (text.includes("@tauri-apps/api/core") && !rel.startsWith("api/")) {
        offenders.push(rel);
      }
    }
    expect(
      offenders,
      `这些文件 import 了 @tauri-apps/api/core，而它们不在 api/ 下：${offenders.join(", ")} —— ` +
        `组件里连拿到 invoke 的机会都不该有（方案 §5.8）`,
    ).toEqual([]);
  });

  it("而 `api/` 下**确实有**那个 import（否则这条门禁是空的）", () => {
    // ⚠️ **没有这一条，上面那条就是"零个文件命中"的空断言。**
    // 一个把所有 import 都删掉的实现会让上面那条**通过**。
    const withImport = sourceFiles(SRC).filter(
      (f) => !f.endsWith(".test.ts") && readFileSync(f, "utf8").includes("@tauri-apps/api/core"),
    );
    expect(withImport.length).toBeGreaterThan(0);
  });
});

describe("门禁 1 的孪生兄弟：`fetch` / `invoke` 不许散落在组件里", () => {
  it("`fetch(` 在 `api/` 之外**一处都没有**", () => {
    // §8 的 M4 验收有一条"冷启动期间不等待任何网络请求" ——
    // 而那条成立的**结构前提**就是"组件里没有 fetch"。
    const offenders: string[] = [];
    for (const f of sourceFiles(SRC)) {
      const rel = relative(SRC, f).replace(/\\/g, "/");
      if (rel.endsWith(".test.ts") || rel.endsWith(".test.tsx")) continue;
      if (rel.startsWith("api/")) continue;
      // 去掉注释再看，免得把"说明文字"当成调用
      const code = readFileSync(f, "utf8")
        .split("\n")
        .filter((l) => !l.trimStart().startsWith("//") && !l.trimStart().startsWith("*"))
        .join("\n");
      if (/\bfetch\s*\(/.test(code) || /\binvoke\s*\(/.test(code)) {
        offenders.push(rel);
      }
    }
    expect(offenders, `这些文件在 api/ 之外调了 fetch/invoke：${offenders.join(", ")}`).toEqual([]);
  });
});

// ============================================================================
// 🔴 门禁 2：IPC 载荷必须在边界上校验
// ============================================================================

describe("边界校验：坏载荷必须在边界上抛（不是漏到界面里）", () => {
  it("实例摘要：形状不对时抛，而消息**说清哪里不对**", () => {
    // ⚠️ 一个用 `as InstanceSummary` 的实现会把"载荷形状不对"
    // 变成"运行时某处读到 undefined" —— 而那条错误**指不到 IPC**。
    expect(() => parseInstanceSummary(null)).toThrow(/应当是一个对象/);
    expect(() => parseInstanceSummary([])).toThrow(/应当是一个对象/);
    expect(() => parseInstanceSummary("x")).toThrow(/应当是一个对象/);
    expect(() => parseInstanceSummary({ name: "a" })).toThrow(/id 必须是非空字符串/);
    expect(() => parseInstanceSummary({ id: "", name: "a" })).toThrow(/id 必须是非空字符串/);
    expect(() => parseInstanceSummary({ id: "a", name: "" })).toThrow(/name 必须是非空字符串/);
  });

  it("实例摘要：对的载荷原样通过", () => {
    expect(parseInstanceSummary({ id: "26.3", name: "我的实例" })).toEqual({
      id: "26.3",
      name: "我的实例",
    });
  });

  it("能力表：坏载荷也会抛（与实例摘要同一条纪律）", () => {
    expect(() => parseCapabilities({ mods: { enabled: false } })).toThrow(
      CapabilityContractError,
    );
  });
});

// ============================================================================
// 🔴 门禁 3：非 Tauri 环境下**不抛**（而是明确回退）
// ============================================================================

describe("环境判断：不在 Tauri 里时不抛，而是给出桩", () => {
  it("jsdom 下 `activeBackend()` 给出**能用的**桩", async () => {
    // ⚠️ 一个"直接 await invoke(...)"的实现会在这里抛
    // `Cannot read properties of undefined (reading 'invoke')` ——
    // 而那条错误**完全不提"这不是 Tauri 环境"**，于是很容易被误判成
    // "命令名拼错了"。
    const b = activeBackend();
    const caps = await b.capabilitiesOf(null);
    // 而桩里**保留了真实的禁用态**（带原因）—— 于是"禁用必须带原因"
    // 这条规则在开发时就能被眼睛看到。
    expect(caps["shaders"]).toEqual({ enabled: false, reason: "该形态不支持光影" });
  });

  it("测试覆盖优先于环境判断（而那是**显式**的）", async () => {
    try {
      setBackend(stubBackend({ fail: true }));
      await expect(backend().capabilitiesOf(null)).rejects.toThrow(/被注入的失败/);
    } finally {
      // ⚠️ **必须恢复** —— 一个泄漏的覆盖会让后面的测试用错后端，
      // 而症状是"单独跑通过、一起跑失败"。
      setBackend(undefined);
    }
  });

  it("而 `fetchCapabilities` 走的是**当前生效的那个**后端", async () => {
    try {
      setBackend({
        capabilitiesOf: async () =>
          ({ mods: { enabled: false } }) as unknown as Awaited<
            ReturnType<typeof fetchCapabilities>
          >,
        instanceSummary: async (id: string) => ({ id, name: id }),
      });
      // 契约被破坏 ⇒ **必须**在边界上抛
      await expect(fetchCapabilities()).rejects.toThrow(CapabilityContractError);
    } finally {
      setBackend(undefined);
    }
  });

  it("`setBackend(undefined)` 真的恢复环境判断", async () => {
    setBackend(stubBackend({ fail: true }));
    setBackend(undefined);
    const caps = await fetchCapabilities();
    expect(caps["launch"]).toEqual({ enabled: true });
  });
});
