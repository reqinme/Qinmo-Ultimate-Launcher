/**
 * 灵动岛的**消息桥**：`IslandState`（IPC）→ `IslandContent`（组件）
 * ============================================================================
 *
 * ## 🔴 这个文件解决的是"同一个状态有两份形状"这件事
 *
 * | 哪一边 | 类型 | 为什么是那个形状 |
 * |---|---|---|
 * | **内核 / IPC** | `IslandState`（Rust 枚举，`tag = "kind"`） | 它是**八态**，而每一态带自己的数据 |
 * | **组件** | `IslandContent`（`kind` + `headline` + `detail` + `fraction`） | 它是**画什么**，而不是**发生了什么** |
 *
 * 两者**不该合并**，理由有三条：
 *
 * 1. 内核那一侧不知道"主文案是什么" —— 那是**表现**，而 §2 的纪律是
 *    "前端零业务逻辑"**反向**也成立：内核不含界面文案。
 * 2. 组件那一侧不该知道"下载速度可能是 `null`"的**原因**
 *    （那原因写在 `qul-core/src/island.rs`：样本不足算不出速度）。
 * 3. 合并之后，"IPC 载荷的形状"与"界面的形状"会**一起变** ——
 *    而那时一次界面改版会牵动一次 IPC 契约变更。
 *
 * ## ⚠️ 而 `parseIslandState` 是**边界校验**，不是类型体操
 *
 * Rust 的类型系统**管不到穿越 IPC 的 JSON**（§5.8 说的那条攻击面）。
 * 所以这里逐字段核对，而**坏的载荷会抛** ——
 * 一个 `as IslandState` 会把"载荷形状不对"变成"界面上某个字段是 `undefined`"，
 * 而那条症状**指不到 IPC**。
 */

import type { IslandContent } from "./Island.tsx";
import { humanBytes, humanEta, humanSpeed } from "./islandMath.ts";

/**
 * 内核八态的**线上形状**（与 `qul-core/src/island.rs` 逐字段对应）。
 *
 * ⚠️ **字段名是 camelCase** —— serde 的默认。而 `LaunchStage` 的值是
 * snake_case（`"parse"` / `"download"` / …），因为那个枚举上有
 * `rename_all = "snake_case"`。
 */
export type IslandState =
  | { readonly kind: "idle" }
  | { readonly kind: "probe"; readonly done: number; readonly total: number }
  | {
      readonly kind: "download";
      readonly done_bytes: number;
      readonly total_bytes: number;
      readonly speed_bps: number | null;
      readonly eta_secs: number | null;
    }
  | {
      readonly kind: "install";
      readonly step: number;
      readonly steps: number;
      readonly current: string;
    }
  | { readonly kind: "launch"; readonly stage: LaunchStage }
  | {
      readonly kind: "running";
      readonly started_at_ms: number;
      readonly resident_bytes: number | null;
    }
  | { readonly kind: "error"; readonly code: string; readonly human: string }
  | { readonly kind: "update"; readonly version: string };

/**
 * ⚠️ **注意它是 snake_case** —— 因为 Rust 那个枚举上有
 * `#[serde(rename_all = "snake_case")]`，而 `IslandState` 上只有 `tag = "kind"`。
 * 一个把两者写成同一种大小写的实现会在**运行期**匹配不上，
 * 而 TypeScript 帮不了忙（它们是字符串字面量）。
 */
export type LaunchStage = "parse" | "download" | "verify" | "extract" | "launch";

const LAUNCH_STAGES: readonly LaunchStage[] = [
  "parse",
  "download",
  "verify",
  "extract",
  "launch",
];

/** 五阶段的人话（**顺序就是进度**，与内核的 `Ord` 一致）。 */
const STAGE_LABEL: Record<LaunchStage, string> = {
  parse: "解析版本",
  download: "下载文件",
  verify: "校验完整",
  extract: "解压原生产物",
  launch: "准备启动",
};

function fail(what: string, got: unknown): never {
  const shown =
    got === null ? "null" : Array.isArray(got) ? "数组" : typeof got;
  throw new TypeError(`灵动岛载荷不合法：${what}（收到 ${shown}）`);
}

function num(o: Record<string, unknown>, k: string): number {
  const v = o[k];
  if (typeof v !== "number" || !Number.isFinite(v) || v < 0) {
    fail(`${k} 必须是非负有限数`, v);
  }
  return v;
}

function numOrNull(o: Record<string, unknown>, k: string): number | null {
  const v = o[k];
  if (v === null) return null;
  if (typeof v !== "number" || !Number.isFinite(v) || v < 0) {
    fail(`${k} 必须是非负有限数或 null`, v);
  }
  return v;
}

function str(o: Record<string, unknown>, k: string): string {
  const v = o[k];
  if (typeof v !== "string") fail(`${k} 必须是字符串`, v);
  return v;
}

/**
 * **校验一个 IPC 载荷并给出 `IslandState`。**
 *
 * ⚠️ 它**不做兜底**：一个"认不出来就返回 Idle"的实现会让
 * "内核加了一个状态而前端没跟上"表现为**岛永远空闲** ——
 * 而那与"没事发生"在屏幕上一模一样。
 *
 * 所以在边界上抛，由调用方决定怎么处理（通常是 `console.error` +
 * 保持上一个状态）。
 */
export function parseIslandState(raw: unknown): IslandState {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    fail("顶层必须是一个对象", raw);
  }
  const o = raw as Record<string, unknown>;
  const kind = o["kind"];
  switch (kind) {
    case "idle":
      return { kind: "idle" };
    case "probe": {
      const done = num(o, "done");
      const total = num(o, "total");
      // ⚠️ `done > total` 是**不可能的**，而它会画出一根超出 100% 的进度条。
      // 在边界上拦住它比在组件里夹紧更好：后者会**掩盖**内核的一个 bug。
      if (done > total) fail(`done(${done}) 不该大于 total(${total})`, done);
      return { kind: "probe", done, total };
    }
    case "download": {
      const done = num(o, "done_bytes");
      const total = num(o, "total_bytes");
      if (total > 0 && done > total) {
        fail(`已下(${done}) 不该大于总量(${total})`, done);
      }
      return {
        kind: "download",
        done_bytes: done,
        total_bytes: total,
        speed_bps: numOrNull(o, "speed_bps"),
        eta_secs: numOrNull(o, "eta_secs"),
      };
    }
    case "install": {
      const step = num(o, "step");
      const steps = num(o, "steps");
      if (steps > 0 && step > steps) fail(`step 不该大于 steps`, step);
      return { kind: "install", step, steps, current: str(o, "current") };
    }
    case "launch": {
      const s = o["stage"];
      if (typeof s !== "string" || !LAUNCH_STAGES.includes(s as LaunchStage)) {
        fail(`stage 必须是五个阶段之一，收到 ${String(s)}`, s);
      }
      return { kind: "launch", stage: s as LaunchStage };
    }
    case "running":
      return {
        kind: "running",
        started_at_ms: num(o, "started_at_ms"),
        // ⚠️ **`null` 是契约的一部分**（基岩版没有 JVM 内存这个概念）。
        // 所以这里**不**把它转成 0 —— 见下面的 `toIslandContent`。
        resident_bytes: numOrNull(o, "resident_bytes"),
      };
    case "error":
      return { kind: "error", code: str(o, "code"), human: str(o, "human") };
    case "update":
      return { kind: "update", version: str(o, "version") };
    default:
      fail(`认不出的 kind：${String(kind)}`, kind);
  }
}

/**
 * **把内核状态翻成组件要画的东西。**
 *
 * ## ⚠️ 三条"不许编"的纪律都落在这里
 *
 * | 情形 | 不许做什么 | 为什么 |
 * |---|---|---|
 * | `speed_bps === null` | 不许显示"0 B/s" | 那与"下载卡住了"**在视觉上无法区分** |
 * | `resident_bytes === null` | 不许显示"0 MB" | **基岩版没有这个概念** —— 编一个 0 是**错的**，不是"近似" |
 * | `total_bytes === 0` | 不许算 `0/0` | 那会得到 `NaN`，而 `NaN` 画出来是一条**空**进度条 |
 */
export function toIslandContent(s: IslandState): IslandContent {
  switch (s.kind) {
    case "idle":
      // ⚠️ **这里刻意不编文案** —— 空闲态的主文案来自"源 + 账户"
      //（见 `Island.tsx` 的 `idleContent`），而那要调用方给。
      // 编一句"就绪"会让"哪个账户 / 哪个源"这两个信息消失。
      return {
        kind: "idle",
        headline: "就绪",
        fraction: null,
        hints: [],
      };
    case "probe":
      return {
        kind: "probe",
        headline: `预检中 · ${s.done}/${s.total}`,
        // ⚠️ **条件展开，而不是 `detail: undefined`。**
        //
        // 本仓库开了 `exactOptionalPropertyTypes: true`，而那条纪律说的是：
        // **"没有这个属性"与"这个属性是 undefined"是两件事。**
        // 前者是这里要的（没话说就别说），而后者会让
        // `"detail" in content` 变成 `true` —— 于是"有没有副文案"
        // 这个判断在别处会给出**错的答案**。
        ...(s.done === s.total ? { detail: "预检完成" } : {}),
        fraction: s.total === 0 ? null : s.done / s.total,
        hints: [],
      };
    case "download": {
      const pct =
        s.total_bytes === 0 ? null : s.done_bytes / s.total_bytes;
      const bits = [
        pct === null ? undefined : `${Math.round(pct * 100)}%`,
        `${humanBytes(s.done_bytes)}/${humanBytes(s.total_bytes)}`,
        humanSpeed(s.speed_bps),
        humanEta(s.eta_secs),
      ].filter((x): x is string => x !== undefined);
      return {
        kind: "download",
        headline: "下载中",
        // ⚠️ 一个 `null` 的字段**不出现在拼接里**（而不是出现成"—"）。
        // 因为"还不知道速度"与"速度是 0"必须看起来不同。
        detail: bits.join(" · "),
        fraction: pct,
        hints: [],
      };
    }
    case "install":
      return {
        kind: "install",
        headline: `安装中 · ${s.step}/${s.steps}`,
        detail: s.current,
        fraction: s.steps === 0 ? null : s.step / s.steps,
        hints: [],
      };
    case "launch": {
      // ⚠️ **阶段序号从 `LAUNCH_STAGES` 的**下标**来** —— 而它正是内核
      // `Ord` 的顺序。一个自己写 `<` 链的实现会在内核加阶段时**悄悄错位**。
      //
      // ⚠️ 而这里**不写 `console.assert(idx >= 0)`** —— 两个理由：
      //   ① `parseIslandState` 已经查过 `stage` 在表里，所以那个断言
      //      是**不可达**的（一个不会失败的断言只是噪音）；
      //   ② 仓库的 lint 白名单不放 `console.assert`，而那个白名单是**有意的**。
      //
      // 而"表里没有它"这件事仍然被处理：`?? null` 而不是 `!`。
      const idx = LAUNCH_STAGES.indexOf(s.stage);
      return {
        kind: "launch",
        headline: STAGE_LABEL[s.stage],
        detail: `第 ${idx + 1}/5 阶段`,
        fraction: idx < 0 ? null : idx / (LAUNCH_STAGES.length - 1),
        hints: [],
      };
    }
    case "running": {
      const hints = [];
      if (s.resident_bytes !== null) {
        hints.push({ label: "内存", value: humanBytes(s.resident_bytes) });
      }
      return {
        kind: "running",
        headline: "游戏运行中",
        // ⚠️ `resident_bytes === null` ⇒ **一个字都不说**（条件展开，
        // 而不是 `detail: undefined` —— 理由同上面 `probe` 那条）。
        // 而编一个 "0 MB" 是**错的**，不是"近似"：那个概念不存在。
        ...(s.resident_bytes === null
          ? {}
          : { detail: humanBytes(s.resident_bytes) }),
        fraction: null,
        hints,
      };
    }
    case "error":
      return {
        kind: "error",
        headline: s.human,
        code: s.code,
        // ⚠️ **错误态的 `fraction` 是 `null`** —— 不是 100%。
        // 一个停在 100% 的错误看起来像"完成了，只是有个提示"。
        fraction: null,
        hints: [{ label: "错误码", value: s.code }],
      };
    case "update":
      return {
        kind: "update",
        headline: `有新版本 ${s.version}`,
        detail: "点开了解详情",
        fraction: null,
        hints: [],
      };
    default: {
      // 🔴 **这个 `default` 是必需的，而它曾经不存在。**
      //
      // TypeScript 的类型系统让"所有 `kind` 都被处理了"看起来成立 ——
      // 而那是**编译期**的保证。运行期还有一条路：**这个函数是一个边界**，
      // 而边界上的输入可以是任何东西。
      //
      // 没有 `default` 时，一个认不出的 `kind` 会**静默地返回 `undefined`**
      //（函数体走完而没有 `return`），于是 `setContent(undefined)` ——
      // 而 React 的 `useState` **接受它**，界面随后炸在一个**指不到这里**的地方。
      //
      // 而这条**是被 `useIslandQueue.test.ts` 抓到的** ——
      // 那条测试第一版"通过"了，因为我用 `try/catch` 吞掉了校验的抛出，
      // 于是 `onState(undefined)` 走到的正是这里。**一个吞掉的异常
      // 会让下一个 bug 更难找**，而不只是少一条断言。
      const never: never = s;
      throw new TypeError(
        `灵动岛状态认不出：${JSON.stringify(never)} —— ` +
          `内核加了一个状态而这边的翻译表没跟上`,
      );
    }
  }
}
