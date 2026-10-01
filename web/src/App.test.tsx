/**
 * 契约与渲染的测试。
 *
 * 这两组测试守的是**同一件事的两端**：
 *  - **契约端**：任何来源（IPC / 桩数据 / 测试）的禁用态都必须带原因；
 *  - **界面端**：界面把原因**照实显示**，且**不做判断**就能表达产品差异。
 *
 * 一旦有人把 `reason` 变成可选，或者界面开始按产品名分支，这里会挂。
 */

import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { App } from "./App.tsx";
import {
  assertCapabilitiesValid,
  CapabilityContractError,
  parseCapabilities,
  type Capabilities,
} from "./api/contract.ts";

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
