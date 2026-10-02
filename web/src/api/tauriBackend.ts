/**
 * 真的 Tauri 后端（**边界层的实现**）
 * ============================================================================
 *
 * ## 🔴 为什么这个文件在 `web/src/api/` 里
 *
 * `eslint.config.js` 有一条 S4 护栏：**`@tauri-apps/api/core` 只允许在
 * `web/src/api/` 下被 import**。而这条规则是"前端零业务逻辑"从
 * **设计意图**变成**机制**的那一步：
 *
 * > 组件里连拿到 `invoke` 的机会都不该有。
 *
 * 而 §5.8 说得更直白：
 *
 * > *"前端零业务逻辑"是**设计意图**，不是**安全边界**。WebView2 里的页面
 * > 一旦被注入内容（渲染了不可信的 mod 描述、公告、Markdown、外部图标），
 * > 攻击面就是**整个 Tauri 命令层**。*
 *
 * 所以这个文件是那条攻击面上的**第一道**（也是前端这一侧唯一的）关口。
 *
 * ## 它做三件事，而每一件都是"关口"
 *
 * | # | 它做 | 不这么做会怎样 |
 * |---|---|---|
 * | 1 | **调 `invoke`** | — |
 * | 2 | **在边界上校验契约**（`parseCapabilities`） | 一个坏的 IPC 载荷会变成"界面某个角落莫名其妙不对" |
 * | 3 | **在非 Tauri 环境下回退**（`isTauri()`） | 单元测试与浏览器预览会**抛异常**，于是"界面能不能渲染"变成一个要起 Tauri 才知道的问题 |
 *
 * ## ⚠️ 第 3 条是本文件最容易被写错的地方
 *
 * 一个"直接 `await invoke(...)`"的实现会在**单元测试里抛**
 *（`window.__TAURI_INTERNALS__` 不存在）。而那时的失败长这样：
 *
 * ```text
 *   TypeError: Cannot read properties of undefined (reading 'invoke')
 * ```
 *
 * —— 它**完全不提"这不是 Tauri 环境"**，于是很容易被误判成"命令名拼错了"。
 *
 * 所以这里显式用 `isTauri()`（Tauri 2 官方就为"浏览器或单元测试"提供了它）。
 */

import { invoke, isTauri } from "@tauri-apps/api/core";
import { parseCapabilities, type Capabilities } from "./contract.ts";
import { stubBackend, type Backend } from "./backend.ts";

/** 一个实例的摘要（**与 Rust 侧 `InstanceSummary` 一一对应**）。 */
export interface InstanceSummary {
  readonly id: string;
  readonly name: string;
}

/**
 * 真的后端。
 *
 * ## 命令名与参数名必须与 Rust 侧逐字一致
 *
 * | 这里 | `src-tauri/src/main.rs` |
 * |---|---|
 * | `invoke("capabilities")` | `fn capabilities()` |
 * | `invoke("instance_summary", { instanceId })` | `fn instance_summary(instance_id: String)` |
 *
 * ⚠️ **构造参数名是 camelCase 而 Rust 侧是 snake_case** ——
 * Tauri 2 默认把 Rust 的 `instance_id` 暴露成 JS 的 `instanceId`。
 * 一个用 `instance_id` 去调的实现在**运行期**会得到
 * "missing required key instanceId"，而那是一条只说了一半的错误。
 */
export const tauriBackend: Backend = {
  async capabilitiesOf(instanceId: string | null): Promise<Capabilities> {
    // ⚠️ **参数现在没用，而它不该被删掉。**
    //
    // `Backend` 的签名是 `capabilitiesOf(instanceId)` —— 而它是**必需的**：
    // 能力是**按实例**算的（§3.3），于是"哪个实例"这条命令将来要收。
    // 一个为了消除 lint 警告而把它从接口里删掉的实现，会在接上多实例时
    // **改一遍所有调用点**（而那时正是最忙的时候）。
    //
    // 所以这里把它交给一个显式的 no-op，而不是改名成 `_instanceId` ——
    // 后者会让下一个人以为"这个参数本来就不需要"。
    void instanceId;
    // ⚠️ 参数 `_instanceId` 现在是**未使用**的 —— 而它**不是**多余的：
    // 能力是**按实例**算的（§3.3），而"哪个实例"这条命令将来要收。
    // 一个现在就把它从接口里删掉的实现，会在接上多实例时改一遍所有调用点。
    const raw: unknown = await invoke("capabilities");
    // 🔴 **在这一侧再校验一次。**
    // Rust 的类型系统管不到穿越 IPC 的 JSON —— 而那正是 §5.8 说的攻击面。
    return parseCapabilities(raw);
  },

  async instanceSummary(instanceId: string): Promise<InstanceSummary> {
    const raw: unknown = await invoke("instance_summary", { instanceId });
    // ⚠️ 这里**不用** `as`：一个 `as InstanceSummary` 会把"载荷形状不对"
    // 变成"运行时某处读到 undefined"。所以逐字段核对。
    return parseInstanceSummary(raw);
  },
};

/**
 * 校验一个实例摘要的载荷。
 *
 * ⚠️ 它与 `contract.ts` 的 `parseCapabilities` 是**同一种东西** ——
 * 边界校验。而它单独一个函数而不是内联，是为了能被**单独测**：
 * "载荷形状不对时抛"这件事只能在边界上验。
 */
export function parseInstanceSummary(raw: unknown): InstanceSummary {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    throw new TypeError(`实例摘要应当是一个对象，收到 ${describe(raw)}`);
  }
  const o = raw as Record<string, unknown>;
  const id = o["id"];
  const name = o["name"];
  if (typeof id !== "string" || id === "") {
    throw new TypeError(`实例摘要的 id 必须是非空字符串，收到 ${describe(id)}`);
  }
  if (typeof name !== "string" || name === "") {
    throw new TypeError(`实例摘要的 name 必须是非空字符串，收到 ${describe(name)}`);
  }
  return { id, name };
}

function describe(v: unknown): string {
  if (v === null) return "null";
  if (Array.isArray(v)) return "数组";
  return typeof v;
}

/**
 * **当前该用哪个后端。**
 *
 * | 环境 | 用它 | 为什么 |
 * |---|---|---|
 * | 在 Tauri 里 | `tauriBackend` | 它才是真的 |
 * | 别处（浏览器 / 单元测试） | `stubBackend()` | 否则一次 `invoke` 就会抛，而错误**不提环境** |
 *
 * ⚠️ **它不是"自动降级"，而是"环境判断"** —— 两者的区别在于
 * **降级会掩盖问题**（"我以为在用真的，其实在用桩"），
 * 而环境判断是**确定的**：`isTauri()` 的答案是 `true` 或 `false`，
 * 不依赖"上一次调用成没成功"。
 */
export function activeBackend(): Backend {
  if (isTauri()) {
    return tauriBackend;
  }
  // ⚠️ 而**这里刻意会说一句**（而不是静默）——
  // 一个静默用桩的实现在"我改了 Rust 侧而界面没变"时让人无从下手。
  // 而 `console.info` 而不是 `warn`：在浏览器里预览界面是**正常用法**。
  // ⚠️ **`warn` 而不是 `info`** —— 而两个理由都成立：
  //   ① 仓库的 lint 白名单只允许 `warn` / `error`（那是一条有意的约束：
  //      剩下的 console 方法在"忘了删的调试输出"与"真的想让人看到"之间
  //      分不清）；
  //   ② 而**这条消息确实值得被看到** —— 它说的是"你在看夹具数据"。
  console.warn("[qinmo] 不在 Tauri 环境里 —— 用桩后端。界面照旧可用，而数据是夹具。");
  return stubBackend();
}
