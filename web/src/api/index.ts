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
 * 而 §5.8 说得更直白：*"'前端零业务逻辑'是**设计意图**，不是**安全边界**"* ——
 * 所以这条边界不只是"整洁"，它是**攻击面上的关口**。
 *
 * ## 四层的依赖方向（**单向**，见 `backend.ts` 里那段）
 *
 * ```text
 *   api/contract.ts      契约的形状与校验（零依赖）
 *   api/backend.ts       Backend 接口 + 桩
 *   api/tauriBackend.ts  真的后端（invoke + 边界校验）
 *   api/index.ts         本文件：取数入口
 * ```
 */

import { assertCapabilitiesValid, type Capabilities } from "./contract.ts";
import {
  activeBackend,
  cancelInstall,
  startInstall,
  type InstallSummary,
  type InstanceSummary,
} from "./tauriBackend.ts";
import { stubBackend, type Backend } from "./backend.ts";

// 让调用点只 import `api/index.ts`（一条入口），而接口与桩的**定义**在
// `backend.ts` —— 于是"谁依赖谁"是单向的，不会循环。
export { stubBackend };
export type { Backend };
export type { InstanceSummary, InstallSummary };
// ⚠️ **流式那两条也走这个入口** —— 于是"组件只 import `api/index.ts`"
// 这条纪律在"进度"这条路上同样成立（`boundary.test.ts` 会核对它）。
export { cancelInstall, startInstall };

/**
 * **一个显式的测试覆盖。`undefined` = 按环境判断。**
 *
 * ## ⚠️ 它为什么存在，以及它为什么**不是**一个可变全局
 *
 * 第一版是 `let current: Backend = stubBackend()` —— 一个**默认就有值**的
 * 可变全局，而它有一个真实的问题：生产代码也依赖它，于是
 * **"我改了 Rust 侧而界面没变"时没有人知道现在用的是哪一个。**
 *
 * 现在默认是 `undefined`，而 `backend()` 在那时问 `activeBackend()` ——
 * 那个函数的答案**确定的**（`isTauri()` 的 `true` / `false`）。
 *
 * 而测试需要一个"注入假后端"的缝（三条状态各要一种桩），
 * 所以这个 `setBackend` **留着**，而它的语义明确是**测试用**：
 * 设了它，环境判断就被跳过；而**生产代码里没有任何地方设它**。
 */
let testOverride: Backend | undefined;

/** **测试用**：注入一个假后端。传 `undefined` 恢复环境判断。 */
export function setBackend(b: Backend | undefined): void {
  testOverride = b;
}

/**
 * **当前生效的后端。**
 *
 * 优先用测试注入的那个（若设了），否则按环境判断。
 */
export function backend(): Backend {
  return testOverride ?? activeBackend();
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
  // ⚠️ 而"真的后端"自己也会过一遍 `parseCapabilities`
  //（见 `tauriBackend.ts`）—— **两道不是重复**：
  // 那一道证明"载荷能被解析成契约"，这一道证明"解析结果满足不变式"。
  assertCapabilitiesValid(raw);
  return raw;
}

/** 取一个实例的摘要。 */
export async function fetchInstanceSummary(instanceId: string): Promise<InstanceSummary> {
  return backend().instanceSummary(instanceId);
}
