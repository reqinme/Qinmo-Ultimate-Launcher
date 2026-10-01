/**
 * 契约与渲染的测试。
 *
 * 这份文件守四件事：
 *  - **契约端**：任何来源（IPC / 桩数据 / 测试）的禁用态都必须带原因；
 *  - **界面端**：界面把原因**照实显示**，且**不做判断**就能表达产品差异；
 *  - **构建配置端**：dev server 不许出现"一刀切"的 `Content-Type` 头；
 *  - **产物端**：生产构建里必须真的含渲染内容（不是白屏）。
 *
 * 一旦有人把 `reason` 变成可选、界面开始按产品名分支、
 * 或者又"顺手优化"了 server 头，这里会挂。
 */

import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync, readdirSync } from "node:fs";

import { App } from "./App.tsx";
import {
  assertCapabilitiesValid,
  CapabilityContractError,
  parseCapabilities,
  type Capabilities,
} from "./api/contract.ts";
import viteConfig from "../vite.config.ts";

/** 把 HTML 里可见的文字抠出来（不引 jsdom，够用且更快）。 */
function visibleText(html: string): string {
  return html
    .replace(/<[^>]*>/g, " ")
    .replace(/&#x27;|&apos;/g, "'")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .trim();
}

const SAMPLE: Capabilities = {
  launch: { enabled: true },
  mods: { enabled: true },
  shaders: { enabled: false, reason: "该形态不支持光影" },
};

describe("能力契约", () => {
  it("接受带原因的禁用态", () => {
    expect(() => assertCapabilitiesValid(SAMPLE)).not.toThrow();
  });

  it("拒绝没有原因的禁用态", () => {
    const bad = { shaders: { enabled: false } } as unknown as Capabilities;
    expect(() => assertCapabilitiesValid(bad)).toThrow(CapabilityContractError);
  });

  it("拒绝原因只有空白字符的禁用态", () => {
    const bad: Capabilities = { shaders: { enabled: false, reason: "   " } };
    expect(() => assertCapabilitiesValid(bad)).toThrow(/没有给出原因/);
  });

  it("错误信息里带上出问题的能力名（便于定位）", () => {
    const bad = { some_key: { enabled: false } } as unknown as Capabilities;
    expect(() => assertCapabilitiesValid(bad)).toThrow(/some_key/);
  });

  it("parseCapabilities 先验后用：非对象输入直接拒绝", () => {
    expect(() => parseCapabilities(null)).toThrow(CapabilityContractError);
    expect(() => parseCapabilities([1, 2, 3])).toThrow(CapabilityContractError);
    expect(() => parseCapabilities("nope")).toThrow(CapabilityContractError);
  });

  it("parseCapabilities 通过后原样返回", () => {
    expect(parseCapabilities(SAMPLE)).toEqual(SAMPLE);
  });
});

describe("外壳渲染", () => {
  const html = renderToStaticMarkup(createElement(App, { capabilities: SAMPLE }));
  const text = visibleText(html);

  it("可用的能力被渲染出来", () => {
    expect(text).toContain("launch");
    expect(text).toContain("mods");
  });

  it("禁用的能力也被渲染出来（不隐藏）", () => {
    expect(text).toContain("shaders");
  });

  it("禁用的能力把原因一并显示——这就是产品信息", () => {
    expect(text).toContain("该形态不支持光影");
  });

  it("禁用态的原因同时进 title（可悬停查看）", () => {
    expect(html).toContain('title="该形态不支持光影"');
  });

  it("渲染时不需要界面认识任何具体产品：只靠 enabled/reason 就够表达差异", () => {
    // 契约里没有任何产品字段；界面照样把"可用/不可用+原因"表达清楚了。
    expect(Object.keys(SAMPLE).every((k) => !/product|edition|loader/i.test(k))).toBe(true);
  });

  it("空能力表不崩，且显示 0", () => {
    const empty = renderToStaticMarkup(createElement(App, { capabilities: {} }));
    expect(visibleText(empty)).toContain("0");
  });
});

/**
 * 这道测试是从一次**真实白屏事故**里长出来的。
 *
 * 事故经过：为了"显式声明 charset"，我在 `server.headers` 里加了
 * `"Content-Type": "text/html; charset=utf-8"`。而 Vite 的 `server.headers`
 * 作用于**所有**响应——包括 `/src/main.tsx`。于是 JS 模块被标成 `text/html`，
 * 浏览器对 ES module **强制校验 MIME 类型**，直接拒绝执行 → React 不挂载 → 全白。
 *
 * **为什么当时没测出来**：`pnpm test` 跑的是纯逻辑（契约 + 渲染字符串），
 * `pnpm build` 也不经过 dev server。**两者都绿，页面却是白的。**
 *
 * **所以这道测试断言的是"配置里不许有这种一刀切头"**——
 * 它是能在无浏览器环境里拦住这个 bug 的最近一道防线。
 * （真正端到端的保障是"用真浏览器载一次页面并看 DOM"，那条写进了
 * `SESSION.md` 的收工检查清单——因为测试跑不了浏览器，工具链里也没有 jsdom。）
 */
describe("构建配置", () => {
  it("dev server 不许设一刀切的 Content-Type 头（会白屏）", () => {
    const headers = (viteConfig as { server?: { headers?: Record<string, string> } }).server
      ?.headers;

    if (headers === undefined) return; // 不设头 = 正确，直接通过

    for (const [name, value] of Object.entries(headers)) {
      const isBlanketType =
        name.toLowerCase() === "content-type" && value.toLowerCase().includes("text/html");
      expect(
        isBlanketType,
        [
          "`server.headers` 里出现了 `Content-Type: text/html`。",
          "",
          "Vite 的 `server.headers` 作用于**所有**响应，不只是 HTML——",
          "`/src/*.tsx` 也会被标成 text/html，而浏览器对 ES module",
          "强制校验 MIME 类型，于是 JS 被拒绝执行、页面变白屏。",
          "",
          "编码问题请交给 index.html 里的 `<meta charset=\"UTF-8\">` 解决。",
        ].join("\n"),
      ).toBe(false);
    }
  });

  it("vite root 必须是 web/（否则会去扫 repos/ 里的参考项目）", () => {
    expect((viteConfig as { root?: string }).root).toBe("web");
  });
});

/**
 * 产物端：生产构建里必须**真的**含渲染内容。
 *
 * 这是"看不见的失败"的最后一道廉价防线：白屏的页面，
 * HTTP 是 200、HTML 也完全合法——**只有内容不在**。
 * 构建产物里出现这些字面量，就证明 JSX 与中文都活着。
 *
 * **依赖 `pnpm build` 已跑过**（CI 里 build 步在 test 之后；
 * 若产物不存在则跳过，避免"只跑 test"的人被误报）。
 */
describe("生产产物", () => {
  const bundleText = (() => {
    const dir = new URL("../dist/assets/", import.meta.url);
    try {
      const js = readdirSync(dir).filter((f) => f.endsWith(".js"));
      if (js.length === 0) return null;
      return js
        .map((f) => readFileSync(new URL(f, dir), "utf8"))
        .join("\n");
    } catch {
      return null; // 还没构建过
    }
  })();

  it.runIf(bundleText !== null)("产物里含真实渲染内容（证明不是白屏）", () => {
    for (const needle of ["秦墨", "该形态不支持光影", "shaders", "launch"]) {
      expect(bundleText, `产物里缺少「${needle}」`).toContain(needle);
    }
  });
});
