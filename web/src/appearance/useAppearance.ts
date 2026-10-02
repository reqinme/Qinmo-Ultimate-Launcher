/**
 * 把外观解析接到 React 上（**M4 主干**）
 * ============================================================================
 *
 * ## 它做三件事，而每一件都必须是**响应式**的
 *
 * 1. **读系统能力**（`prefers-color-scheme` / `prefers-contrast` /
 *    `prefers-reduced-motion`）—— 而它们**会在运行中变化**（用户切了系统主题、
 *    插上电源、切换到远程桌面）。一个"只在启动时读一次"的实现会让
 *    那些变化**不生效**，而用户会以为是我们坏了。
 * 2. 读省电与远程桌面 —— ⚠️ **Web 平台没有可靠的 API**（见下）。
 * 3. 解析并写 `data-*`。
 *
 * ## 🔴 省电与远程桌面：`null` 是诚实的答案
 *
 * `navigator.getBattery()` 在 **Firefox 里没有**，而"远程桌面"在浏览器里
 * **完全没有** API。而 Tauri 里它们将来可以经由 Rust 侧的能力描述符拿到
 *（§3.3：产品差异由后端表达）。
 *
 * **所以这里把它们当成"未知 = 不触发降级"，而不是"false"。**
 * 一个把它们写死成 `false` 的实现会在"将来接上真值"时**看不出**
 * 哪些地方漏了 —— 而 `null` 会让类型系统提醒那个接线点。
 */

import { useEffect, useState } from "react";
import {
  IntensityPref,
  MaterialPref,
  ThemePref,
  applyAppearance,
  resolveAppearance,
  type ResolvedAppearance,
  type SystemCapabilities,
  type UserPrefs,
} from "./appearance.ts";

/** 用 `matchMedia` 订阅一个媒体查询。**它会随变化更新。** */
function useMediaQuery(query: string, fallback = false): boolean {
  const [matches, setMatches] = useState<boolean>(() => {
    // ⚠️ `matchMedia` 在 jsdom 里**存在但功能有限**，而在极老的 WebView 里
    // 可能不存在 —— 所以这里是 `?.` 加兜底，**不抛**。
    try {
      return globalThis.matchMedia?.(query).matches ?? fallback;
    } catch {
      return fallback;
    }
  });

  useEffect(() => {
    let mq: MediaQueryList | undefined;
    try {
      mq = globalThis.matchMedia?.(query);
    } catch {
      return;
    }
    if (mq === undefined) return;

    // ⚠️ **不用 `addEventListener` 的旧写法兜底** —— Tauri 2 的 WebView2 与
    // 所有现代浏览器都支持 `addEventListener`，而写两套会让"到底哪一套在跑"
    // 变成一个问题。
    const onChange = (e: MediaQueryListEvent): void => setMatches(e.matches);
    mq.addEventListener("change", onChange);
    setMatches(mq.matches);
    return () => mq.removeEventListener("change", onChange);
  }, [query]);

  return matches;
}

/** 系统能力的现状（**响应式**）。 */
export function useSystemCapabilities(): SystemCapabilities {
  const prefersDark = useMediaQuery("(prefers-color-scheme: dark)", true);
  const highContrast = useMediaQuery("(prefers-contrast: more)", false);
  const reducedMotion = useMediaQuery("(prefers-reduced-motion: reduce)", false);

  // ⚠️ **这两个现在恒为 `false`，而那是已知的缺口** ——
  // 见文件头"省电与远程桌面"那一段。它们将来由 Rust 侧的能力描述符给。
  return {
    prefersDark,
    highContrast,
    reducedMotion,
    batterySaver: false,
    remoteSession: false,
  };
}

/** 默认偏好（**M4 主干里还没有设置页**，所以这是一组保守的默认值）。 */
export const DEFAULT_PREFS: UserPrefs = {
  // §3.4 原文："**默认跟随系统**"
  theme: ThemePref.System,
  material: MaterialPref.Full,
  // §5.4.4 纪律 3："`Visual.Enhanced` **默认关闭** ——
  // 默认值是给所有人用的，而'更丰富'是少数人在少数时候想要的"
  intensity: IntensityPref.Standard,
  instanceAccent: undefined,
};

/**
 * 把外观接到 DOM 上。
 *
 * ⚠️ **`useEffect` 而不是在 render 里写** —— render 期间的副作用会在
 * React 严格模式的**双调用**下执行两次，而那对 DOM 是幂等的、对
 * "读一次就缓存"的逻辑不是。
 */
export function useAppearance(prefs: UserPrefs = DEFAULT_PREFS): ResolvedAppearance {
  const sys = useSystemCapabilities();
  const resolved = resolveAppearance(prefs, sys);

  useEffect(() => {
    applyAppearance(resolved);
  }, [resolved.theme, resolved.material, resolved.intensity, resolved.accent]);

  return resolved;
}
