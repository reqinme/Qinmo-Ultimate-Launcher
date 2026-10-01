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
  // 相对路径。**理由是 Tauri**：它用自定义协议（tauri://localhost）加载前端，
  // 绝对路径 '/assets/..' 在那个协议下会 404。
  //
  // ⚠️ **不要**把它当成'因此就能双击 dist/index.html 打开'：
  // file:// 的 origin 是 null，ES module 会被 CORS 一律拦掉（与路径是否相对无关）。
  // 看界面只能走 HTTP：dev 用 http://localhost:5173/。
  base: "./",
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
     * ⚠️ **这里刻意不设 `headers`。教训记在下面。**
     *
     * 我一度为了"显式声明 charset"加了：
     * ```js
     * headers: { "Content-Type": "text/html; charset=utf-8" }
     * ```
     * **这一行把整个页面干成了白屏。** 因为 Vite dev server 的 `headers`
     * 会作用于**所有**响应（包括 `/src/*.tsx`），于是 JS 模块被标成 `text/html`，
     * 而浏览器对 ES module **强制校验 MIME 类型** → 拒绝执行 → React 不挂载。
     *
     * **修法不是"给 JS 也配一个头"，而是根本不设这个头**：
     * Vite 对每种文件本就给出正确的 Content-Type，
     * 而中文编码的问题 HTML 里的 `<meta charset="UTF-8">` 已经解决
     * （实测 `document.title` 取到的就是「秦墨」，不是乱码）。
     *
     * **通用教训**：想在"传输层"给整个 server 加一条通用规则时，
     * 先问它会不会盖掉框架**按文件类型**做的正确判断。
     */
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
      /**
       * ⚠️ **M4 起是 `jsdom`，而 U0 时是 `node`。**
       *
       * 原来的注释写着"不引 jsdom / testing-library：那要等 M4 的组件测试
       * 再引，现在引进来只会多两个依赖和一套没用的配置"——那个判断当时是对的，
       * 而现在已经到了那一步：门禁② 有约 20 个基础组件、门禁④ 要验证
       * TanStack Query 的三态渲染，**两者都必须在 DOM 里跑**。
       *
       * 代价：依赖多了三个（`jsdom` + `@testing-library/react` +
       * `@testing-library/dom`），而**它们的 postinstall 全是空**
       * ——所以 `pnpm.onlyBuiltDependencies` 那条白名单不需要放宽。
       */
      environment: "jsdom",
      /**
       * ⚠️ **一个 setup 文件，而它只修一件跨 realm 的事** ——
       * 见 `src/test-setup.ts` 的文档。
       */
      setupFiles: ["src/test-setup.ts"],
      include: ["src/**/*.test.{ts,tsx}"],
      /**
       * `globals: false` —— **测试里显式 import `describe` / `it` / `expect`。**
       *
       * 注入全局会让"这个文件里 `expect` 从哪来"变成一个隐式事实，
       * 而显式 import 让每个测试文件的依赖自洽（也便于将来搬文件）。
       */
      globals: false,
    },
});
