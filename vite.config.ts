import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

/**
 * 前端构建配置。
 *
 * **Tauri 的两个关键点**：
 * - `clearScreen: false` —— 不要擦掉 Rust 侧的编译错误。
 * - `envPrefix: ["VITE_", "TAURI_"]` —— 允许 Tauri 注入环境变量。
 *
 * **端口固定为 5173** 且 `strictPort: true`：Tauri 的 devUrl 写死了这个端口，
 * 若被占用就让 Vite 直接失败，而不是悄悄换端口（那会让 `tauri dev` 白屏，
 * 且报错信息完全不指向真正的原因）。
 */
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    // WebView2 跟随 Edge 更新，主流版本早已支持 ES2022
    target: "es2022",
    sourcemap: true,
  },
  test: {
    // U0 只做**纯逻辑测试**（契约校验、渲染输出）。
    // 不引 jsdom / testing-library：那要等 M4 的组件测试再引，
    // 现在引进来只会多两个依赖和一套没用的配置。
    environment: "node",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
