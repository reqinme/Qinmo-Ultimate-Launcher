/**
 * 消息桥的测试（**八态 + 三条"不许编"的纪律**）
 * ============================================================================
 *
 * ⚠️ 这个文件里**最有价值的那几条**不是"能翻译"，而是"翻译不出来时**会抛**"
 * 与"缺值时**不编数**"。前者防的是"载荷坏了而界面静默"，后者防的是
 * "把 `null` 当成 0"。
 */

import { describe, expect, it } from "vitest";
import {
  parseIslandState,
  toIslandContent,
  type IslandState,
} from "./bridge.ts";

/** 八态的**合法**线上载荷（与 Rust 的 serde 输出逐字段对应）。 */
const VALID: readonly IslandState[] = [
  { kind: "idle" },
  { kind: "probe", done: 1, total: 3 },
  {
    kind: "download",
    done_bytes: 1024,
    total_bytes: 4096,
    speed_bps: 512,
    eta_secs: 6,
  },
  { kind: "install", step: 2, steps: 5, current: "解压 natives" },
  { kind: "launch", stage: "verify" },
  { kind: "running", started_at_ms: 1_000, resident_bytes: 536_870_912 },
  { kind: "error", code: "QUL-NET-0005", human: "服务器返回 503" },
  { kind: "update", version: "26.4" },
];

describe("parseIslandState：八态都认得", () => {
  it("每一态都能往返（校验之后再翻译不会抛）", () => {
    for (const s of VALID) {
      // ⚠️ 走一遍 `JSON` 是为了**模拟真实的 IPC**（它一定是 JSON 文本中转的），
      // 而不是把 TypeScript 对象直接喂进去 —— 后者会漏掉
      // "字段名大小写不对"这一类错。
      const wire: unknown = JSON.parse(JSON.stringify(s));
      expect(parseIslandState(wire)).toEqual(s);
    }
  });

  it("🔴 认不出的 kind **抛** —— 而不是兜底成空闲", () => {
    // ⚠️ 一个"认不出就返回 Idle"的实现会让"内核加了一个状态而前端没跟上"
    // 表现为**岛永远空闲** —— 而那与"没事发生"在屏幕上一模一样。
    expect(() => parseIslandState({ kind: "teleporting" })).toThrow(
      /认不出的 kind/,
    );
  });

  it("缺字段 / 类型不对时抛，而消息说清是哪个字段", () => {
    expect(() => parseIslandState({ kind: "download" })).toThrow(/done_bytes/);
    expect(() =>
      parseIslandState({ kind: "error", code: 5, human: "x" }),
    ).toThrow(/code 必须是字符串/);
    expect(() => parseIslandState([])).toThrow(/顶层必须是一个对象/);
    expect(() => parseIslandState(null)).toThrow(/顶层必须是一个对象/);
  });

  it("🔴 `done > total` 在边界上就被拦住", () => {
    // ⚠️ 它会画出一根**超出 100% 的进度条**。
    // 在边界上拦比在组件里夹紧更好：后者会**掩盖**内核的一个 bug。
    expect(() =>
      parseIslandState({ kind: "probe", done: 4, total: 3 }),
    ).toThrow(/不该大于/);
  });

  it("🔴 `stage` 必须是五个阶段之一（大小写也算）", () => {
    // ⚠️ 这条钉的是 **snake_case** —— `LaunchStage` 上有
    // `rename_all = "snake_case"`，而一个写成 `"Parse"` 的实现
    // 在 TypeScript 里**不会报错**（它是字符串字面量）。
    expect(parseIslandState({ kind: "launch", stage: "parse" })).toEqual({
      kind: "launch",
      stage: "parse",
    });
    expect(() => parseIslandState({ kind: "launch", stage: "Parse" })).toThrow(
      /五个阶段之一/,
    );
    expect(() => parseIslandState({ kind: "launch" })).toThrow(/五个阶段之一/);
  });
});

describe("🔴 三条「不许编数」的纪律", () => {
  it("速度是 null 时**不显示 0 B/s**", () => {
    // 那与"下载卡住了"在视觉上无法区分。
    const c = toIslandContent({
      kind: "download",
      done_bytes: 1024,
      total_bytes: 4096,
      speed_bps: null,
      eta_secs: null,
    });
    expect(c.detail).not.toContain("0 B/s");
    expect(c.detail).not.toContain("B/s");
    // 而"已经下了多少"仍然该显示 —— 缺的只是速度那一项。
    expect(c.detail).toContain("25%");
  });

  it("内存是 null 时**一个字都不说**（基岩版没有这个概念）", () => {
    const c = toIslandContent({
      kind: "running",
      started_at_ms: 1_000,
      resident_bytes: null,
    });
    // ⚠️ 编一个 "0 MB" 是**错的**，不是"近似" —— 那个概念不存在。
    expect(c.detail).toBeUndefined();
    expect(c.hints).toEqual([]);
  });

  it("总量为 0 时进度是 `null`（不确定），而不是 `NaN`", () => {
    // ⚠️ `0/0` 会得到 `NaN`，而 `NaN` 画出来是一条**空**进度条 ——
    // 那与"不确定"该有的样子**恰好相反**（§7.2：不确定必须有自己的样子）。
    const c = toIslandContent({
      kind: "download",
      done_bytes: 0,
      total_bytes: 0,
      speed_bps: null,
      eta_secs: null,
    });
    expect(c.fraction).toBeNull();
  });

  it("错误态的进度是 `null`，不是 100%", () => {
    // ⚠️ 一个停在 100% 的错误看起来像"完成了，只是有个提示"。
    const c = toIslandContent({
      kind: "error",
      code: "QUL-NET-0005",
      human: "服务器返回 503",
    });
    expect(c.fraction).toBeNull();
    expect(c.headline).toBe("服务器返回 503");
  });
});

describe("五阶段的下标来自那张表（与内核的 Ord 一致）", () => {
  it("`parse` 是第 1 阶段，而 `launch` 是最后", () => {
    const a = toIslandContent({ kind: "launch", stage: "parse" });
    const z = toIslandContent({ kind: "launch", stage: "launch" });
    expect(a.detail).toContain("1/5");
    expect(z.detail).toContain("5/5");
    // ⚠️ 而**序号必须单调** —— 一个自己写 `<` 链的实现会在内核加阶段时错位。
    expect(a.fraction).toBe(0);
    expect(z.fraction).toBe(1);
  });

  it("中间三段的序号是 2/3/4", () => {
    const got = (["download", "verify", "extract"] as const).map(
      (s) => toIslandContent({ kind: "launch", stage: s }).detail,
    );
    expect(got[0]).toContain("2/5");
    expect(got[1]).toContain("3/5");
    expect(got[2]).toContain("4/5");
  });
});
