import type { ReactElement } from "react";
import { App } from "../App.tsx";
import { parseCapabilities, type Capabilities } from "../api/contract.ts";

/**
 * 能力表页 —— **U0 那一页没有消失，它变成了一条路由**。
 *
 * ## 为什么不删掉它
 *
 * 它验证的是三件**至今仍然重要**的事（见 `App.tsx` 的文档）：
 * 1. Rust ↔ TS 的契约形状能对上；
 * 2. `reason` 在禁用态**确实存在**（而且没有禁用态缺原因）；
 * 3. 界面不写 `if (product === ...)` 也能把差异表达清楚。
 *
 * 那三件事在 M4 之后仍然是纪律（§3.3 的能力描述符、§1.4 的"界面不认识具体游戏"），
 * 所以**把这一页保留成一个能随时打开的自检页**比删掉它更有价值。
 *
 * ## ⚠️ 而数据现在是**这一页**的
 *
 * U0 的时候能力表在 `main.tsx` 里构造（挂载时就发）。
 * 现在它是这一页的 —— 于是"**冷启动期间不等待任何网络请求**"（§8 的 M4 验收）
 * 在结构上成立：不打开这一页就不会去取它。
 *
 * `loadCapabilities` 的注释说的"M1 起改为调 Tauri 命令"就是接线的位置，
 * 而那要等 src-tauri 存在（M1 剩余项：Tauri 安全基线 + 命令层门禁）。
 */
function loadCapabilities(): Capabilities {
  // ⚠️ **仍是夹具**，而它现在**只在这一页被打开时**才构造。
  return parseCapabilities({
    launch: { enabled: true },
    preflight: { enabled: true },
    mods: { enabled: true },
    worlds: { enabled: true },
    configs: { enabled: true },
    crash_analysis: { enabled: true },
    log_filtering: { enabled: true },
    offline_play: { enabled: true },
    // ↓ 真实的禁用态：原因面向用户，不是错误码
    shaders: { enabled: false, reason: "该形态不支持光影" },
    isolation: { enabled: false, reason: "该形态无法隔离实例，与官方启动器共用账户与数据" },
  });
}

export function CapabilitiesPage(): ReactElement {
  return <App capabilities={loadCapabilities()} />;
}