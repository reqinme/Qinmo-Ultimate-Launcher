/**
 * 后端的**契约与桩**（边界层的公共部分）
 * ============================================================================
 *
 * ## ⚠️ 这个文件是从 `api/index.ts` 拆出来的，而拆的理由是**一个真实的循环**
 *
 * 第一版把 `Backend` 接口与 `stubBackend` 放在 `api/index.ts` 里，
 * 而真的 Tauri 后端需要它们 —— 于是 `api/tauriBackend.ts` 从
 * `api/index.ts` import，而 `api/index.ts` 又要从它 import。
 *
 * **循环 import 在 TypeScript 里不报错** —— 它会在运行期表现为
 * "某个绑定是 `undefined`"，而那条错误**指不到循环本身**。
 *
 * 所以拆成三层：
 *
 * ```text
 *   api/contract.ts      契约的形状与校验（零依赖）
 *   api/backend.ts       Backend 接口 + 桩        ← 本文件（不 import 任何兄弟）
 *   api/tauriBackend.ts  真的后端（import backend + contract）
 *   api/index.ts         取数入口（import tauriBackend + backend）
 * ```
 *
 * **依赖方向是单向的**，而这就是它不循环的原因。
 */

import { parseCapabilities, type Capabilities } from "./contract.ts";

/**
 * Rust 侧的调用接口。
 *
 * ⚠️ **它是一个接口而不是直接调 `invoke`**，因为：
 *
 * 1. 测试里可以注入一个假实现（**不需要起 Tauri**）；
 * 2. "调用形状"在一处可见；
 * 3. 将来 Tauri 的命令改名时，只有实现那一处要改。
 */
export interface Backend {
  /** 取某个实例的能力结论。`instanceId` 为 `null` 表示"当前产品"的默认实例。 */
  capabilitiesOf(instanceId: string | null): Promise<Capabilities>;
  /** 取一个实例的摘要。 */
  instanceSummary(
    instanceId: string,
  ): Promise<{ readonly id: string; readonly name: string }>;
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
