import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

/**
 * 前端构建配置。
 *
 * ## ⚠️ 关键：`root` 必须是 `web/`，不能让 Vite 以项目根为根
 *
 * **这条是从一次真实崩溃里学来的。** 前端最初放在项目根（`src/` + `index.html`），
 * 而项目根下住着 `repos/`（17 个参考项目，587 MB）。
 * `pnpm dev` 因此把 `repos/XMCL/**`、`repos/LeviLauncher/**` 全当成待处理模块：
 * 上千条 HMR 日志 + 一堆 `Failed to resolve import "@/i18n"` 报错 + dev server 直接崩。
 *
 * **修法不是"把 repos 加进忽略名单"，而是让前端有一个自己的根。**
 * 忽略名单是黑名单（来一个新的参考仓库就得再补一条），
 * 而换根是白名单——**根之外的东西 Vite 根本不会去看**。
 *
 * 这也正是 Tauri 官方推荐的前端目录布局。
 */
export default defineConfig({
  root: "web",
  plugins: [react()],
  clearScreen: false,
  // 允许 Tauri 注入的环境变量（Tauri 2 用 TAURI_ 前缀）
  envPrefix: ["VITE_", "TAURI_"],
  server: {
    // Tauri 的 devUrl 会写死这个端口；被占用就失败，而不是悄悄换端口
    // （换端口会让 `tauri dev` 白屏，且报错完全不指向真正原因）
    port: 5173,
    strictPort: true,
    /**
     * **显式声明 charset。**
     *
     * 实测 Vite dev server 返回 `Content-Type: text/html`（**不带 charset**），
     * 那次是靠 HTML 里的 `<meta charset="UTF-8">` 兜住的。
     * 但界面里到处是中文（以及那个最要紧的"禁用原因"）
     * ——**编码只能靠猜的时候，中文是最先坏的那一类内容。**
     * 显式声明不花任何代价，何必留一个"只要 meta 忘了就全乱"的隐患。
     */
    headers: {
      "Content-Type": "text/html; charset=utf-8",
    },
    // 只监听 web/ 子树，避免把 repos/ 的变动也当成"项目源文件变了"
    watch: {
      ignored: ["**/repos/**", "**/_archive/**", "**/target/**"],
    },
  },
  build: {
    // 产物固定到 web/dist，供 Tauri 的 frontendDist 引用
    outDir: "dist",
    emptyOutDir: true,
    // WebView2 跟随 Edge 更新，主流版本早已支持 ES2022
    target: "es2022",
    sourcemap: true,
  },
  test: {
    // U0 只做纯逻辑测试（契约校验、渲染输出）。
    // 不引 jsdom / testing-library：那要等 M4 的组件测试再引，
    // 现在引进来只会多两个依赖和一套没用的配置。
    environment: "node",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
