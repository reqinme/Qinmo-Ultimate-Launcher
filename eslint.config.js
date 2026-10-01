/**
 * ESLint 配置（flat config）。
 *
 * **这里的规则不是"风格偏好"，是设计纪律的机器版。**
 * 方案 §2 与 §5.3 把"界面永远拿不到十六进制色值""界面不认识具体游戏"
 * 写成纪律；纪律只在能自动拦截时才算数，所以它们以 `no-restricted-syntax`
 * 的形式落在这里。
 *
 * 三条规则：
 *  1. **禁止字面颜色值** —— 只能用 `var(--token)` 语义令牌。
 *  2. **禁止用产品名做判断** —— 界面不许认识 java / bedrock / forge 这些词；
 *     差异必须由后端的**能力描述符**表达（方案 §3.3）。
 *  3. **禁止 toggle 式布尔状态** —— 用联合类型表达状态机，
 *     这样"非法状态组合"在类型层面就不存在（灵动岛 8 态就是这么做的）。
 */

import js from "@eslint/js";
import tseslint from "typescript-eslint";
import globals from "globals";

const HEX_COLOR = "/^#(?:[0-9a-fA-F]{3,4}){1,2}$/";
const CSS_FN = "/^(?:rgb|rgba|hsl|hsla)\\(/";
const PRODUCT_NAME = "/^(java|bedrock|forge|fabric|neoforge|quilt|optifine|mojang|minecraft)$/i";

export default tseslint.config(
  {
    ignores: [
      "web/dist/**",
      "node_modules/**",
      "coverage/**",
      "src-tauri/**",
      "crates/**",
      "repos/**",
      "_archive/**",
      "spikes/**",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["web/src/**/*.{ts,tsx}"],
    languageOptions: {
      globals: { ...globals.browser },
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    rules: {
      "no-restricted-syntax": [
        "error",
        {
          selector: `Literal[value=${HEX_COLOR}]`,
          message:
            "界面里禁止字面颜色值。请用语义令牌，例如 var(--surface-panel) / var(--text-primary)。（方案 §5.3）",
        },
        {
          selector: `Literal[value=${CSS_FN}]`,
          message:
            "界面里禁止字面颜色函数。若确需在样式里用 rgba()，请把它定义在 CSS 变量层（tokens.css），不要在组件里写。（方案 §5.3）",
        },
        {
          selector: `Literal[value=${PRODUCT_NAME}]`,
          message:
            "界面不许用产品名做判断。产品差异必须由后端的能力描述符表达（{ key, enabled, reason }），不要写 if (product === 'java')。（方案 §3.3、§1.4）",
        },
        {
          selector: "CallExpression[callee.name='useState'][arguments.0.type='Literal'][arguments.0.value=/^(true|false)$/]",
          message:
            "不要用 useState(false) 这类 toggle 布尔状态。状态机请用联合类型（'idle' | 'running' | ...），让非法状态在类型层面不存在。",
        },
      ],
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/consistent-type-imports": "error",
      eqeqeq: ["error", "always"],
      "no-console": ["warn", { allow: ["warn", "error"] }],
    },
  },
  {
    // 测试文件：允许字面量样本（测试就是要用具体值验证规则）
    files: ["web/src/**/*.test.{ts,tsx}", "web/src/**/__tests__/**"],
    rules: {
      "no-restricted-syntax": "off",
    },
  },
);
