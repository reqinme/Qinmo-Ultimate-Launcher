/**
 * 与 Rust 侧的**唯一**取数入口（方案 §2 的"前端零业务逻辑"的边界）
 * ============================================================================
 *
 * ## 🔴 为什么所有取数都必须经过这个目录
 *
 * `eslint.config.js` 里有一条 S4 护栏（M0 补），它把 `web/src/api/`
 * 当作**唯一允许碰外部世界的地方**：
 *
 * | 规则 | 拦什么 |
 * |---|---|
 * | `fetch` 调用 | 组件里不许直接发网络请求 |
 * | `invoke` 调用 | 组件里不许直接调 IPC |
 * | `@tauri-apps/api/core` 导入 | 组件里连拿到 `invoke` 的机会都不该有 |
 *
 * **理由**（那条注释的原文）：*"否则组件不可测（要起网络）、数据来源不唯一
 * （两处各发一次）、错误处理会散落在各处。"*
 *
 * ## ⚠️ 而本文件现在**还没有 Tauri 可调**
 *
 * `src-tauri` 不存在 —— 它是 M1 的剩余项（Tauri 安全基线 + 前端命令层门禁），
 * 而那一项**硬依赖 M4 的前端存在**。所以现在这里是**明确的桩**，
 * 而桩的形状就是真实调用的形状：
 *
 * ```ts
 * // 接线的位置（M1 剩余项）：把下面每个 `return` 换成
 * //   const raw: unknown = await invoke("<命令名>", { ... });
 * //   return parseXxx(raw);        // ← 边界校验，先验后用
 * ```
 *
 * **一个"等有了后端再抽这一层"的做法会失败**：那时组件里已经到处是
 * `invoke` 了，而把它们收回来是一次全量重写。
 */

import { assertCapabilitiesValid, parseCapabilities, type Capabilities } from "./contract.ts";

/**
 * Rust 侧的调用接口。
 *
 * ⚠️ **它是一个接口而不是直接调 `invoke`**，因为：
 *
 * 1. 测试里可以注入一个假实现（**不需要起 Tauri**）；
 * 2. "调用形状"在一处可见；
 * 3. 将来 Tauri 的命令改名时，只有这一个文件要改。
 */
export interface Backend {
  /** 取某个实例的能力结论。`id` 为 `null` 表示"当前产品"的默认实例。 */
  capabilitiesOf(instanceId: string | null): Promise<Capabilities>;
  /** 取一个实例的摘要（**骨架期只要 id 与显示名**）。 */
  instanceSummary(instanceId: string): Promise<{ readonly id: string; readonly name: string }>;
}

/**
 * **骨架期的假后端。**
 *
 * 它刻意保留**真实的禁用态**（带原因）—— 于是"禁用必须带原因"
 * 这条规则在开发时就能被眼睛看到，而不是等 M1。
 *
 * 而它**有一个可观察的延迟**（默认 0）：门禁④ 要证明的是"加载态、
 * 错误态、成功态"三条路都通，所以测试里会注入延迟与失败。
 */
export function stubBackend(opts?: {
  readonly delayMs?: number;
  readonly fail?: boolean;
}): Backend {
  const delay = opts?.delayMs ?? 0;
  const sleep = async (): Promise<void> => {
    if (delay > 0) {
      await new Promise((r) => setTimeout(r, delay));
    }
  };
  return {
    async capabilitiesOf(): Promise<Capabilities> {
      await sleep();
      if (opts?.fail === true) {
        throw new Error("后端不可用（桩：这是被注入的失败）");
      }
      // ⚠️ 这里走的是**契约的解析器**，而不是直接返回对象 ——
      // 于是"返回脏数据"会在**同一条路**上被抓到，与真实后端一致。
      return parseCapabilities({
        launch: { enabled: true },
        preflight: { enabled: true },
        mods: { enabled: true },
        worlds: { enabled: true },
        configs: { enabled: true },
        crash_analysis: { enabled: true },
        log_filtering: { enabled: true },
        offline_play: { enabled: true },
        shaders: { enabled: false, reason: "该形态不支持光影" },
        isolation: {
          enabled: false,
          reason: "该形态无法隔离实例，与官方启动器共用账户与数据",
        },
      });
    },
    async instanceSummary(instanceId: string) {
      await sleep();
      if (opts?.fail === true) {
        throw new Error("后端不可用（桩：这是被注入的失败）");
      }
      return { id: instanceId, name: instanceId };
    },
  };
}

/**
 * 当前生效的后端实现。
 *
 * ⚠️ **`let` + 一个 setter 是刻意的**：M1 的接线只需要改这一处，
 * 而所有取数路径（组件、测试、未来的一段后台逻辑）立刻都用上真的那个。
 */
let current: Backend = stubBackend();

export function setBackend(b: Backend): void {
  current = b;
}

export function backend(): Backend {
  return current;
}

/**
 * **取能力表，并在边界上校验。**
 *
 * 校验失败会抛 —— 而调用方（TanStack Query）会把它变成 `error` 态。
 * **不要在这里 catch 掉**：一个"校验失败就返回空表"的实现会让
 * "后端契约坏了"在界面上表现为**一个正常但空白的页**。
 */
export async function fetchCapabilities(instanceId: string | null = null): Promise<Capabilities> {
  const raw = await backend().capabilitiesOf(instanceId);
  // 再过一遍不变式：真实后端穿越 IPC 时，Rust 的类型系统管不到 JSON。
  assertCapabilitiesValid(raw);
  return raw;
}

/** 取一个实例的摘要。 */
export async function fetchInstanceSummary(
  instanceId: string,
): Promise<{ readonly id: string; readonly name: string }> {
  return backend().instanceSummary(instanceId);
}
