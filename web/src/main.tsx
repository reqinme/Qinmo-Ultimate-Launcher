/**
 * 前端入口。
 *
 * **它只做三件事**：挂载 React、取能力表、把能力表交给 [`App`]。
 * 任何"该显示什么"的判断都不在这里——那是后端的活。
 */

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App.tsx";
import { parseCapabilities, type Capabilities } from "./api/contract.ts";
import "./tokens.css";
import "./styles.css";

/**
 * 取能力表。
 *
 * **U0 阶段返回夹具数据**，并**刻意留一个真实的禁用态**（带原因）——
 * 这样"禁用必须带原因"这条规则在开发时就能被眼睛看到，而不是等 M1。
 *
 * **M1 起改为调 Tauri 命令**：`invoke<unknown>("instance_capabilities", { id })`
 * 然后交给 [`parseCapabilities`] 校验。接线的位置就是这里，界面组件不用改。
 */
function loadCapabilities(): Capabilities {
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

const host = document.getElementById("root");
if (!host) {
  throw new Error("找不到 #root 挂载点");
}

createRoot(host).render(
  <StrictMode>
    <App capabilities={loadCapabilities()} />
  </StrictMode>,
);
