/**
 * 主题四态 · 材质三档 · 视觉强度两档的解析（**M4 主干** · §3.4 / §5.2 / §5.4）
 * ============================================================================
 *
 * ## 🔴 它是纯函数，因为"谁覆盖谁"是一个**判断**
 *
 * §5.4.1 给了四条自上而下的**阶梯**：
 *
 * | 层 | 来源 | 可调范围 |
 * |---|---|---|
 * | **1. 系统能力** | 高对比度 / 省电 / 远程桌面 / `prefers-*` | **只能收紧，不能放开** |
 * | **2. 材质档位** | `Full` / `Reduced` / `None` | **自动、只降不升**；用户可**手动降低** |
 * | **3. 视觉强度** | `Standard` / `Enhanced` | 用户 |
 * | **4. 实例强调色** | 每个实例一个 | 用户（作用域最窄） |
 *
 * 而**纪律 ①** 是那一段的实质：
 *
 * > **向下只累加收紧，不叠加放开。** 第 1 层说不行，第 2/3/4 层**都不得放开**。
 * > 高对比度模式下，`Visual.Enhanced` **不生效，且不报错** —— 它只是被上层收紧了。
 *
 * **"且不报错"那五个字是本模块形状的来源**：解析的结果里**没有"冲突"这个字段** ——
 * 被上层收紧**不是错误**，它是正常结果。一个在界面上弹"你的增强视觉被忽略了"
 * 的实现会违反那一条。
 *
 * ## 而主题四态与材质三档**是正交的**
 *
 * §3.4：*"材质有独立的开关档位 ……**与主题四态正交**"*。
 * 所以它们是**两个独立的轴**，不是"六个组合"。
 */

/** 主题四态（§5.2）。 */
export const ThemePref = {
  Dark: "dark",
  Light: "light",
  /** 跟随系统（`prefers-color-scheme`）。 */
  System: "system",
  /** **跟随实例** —— 每个实例可以指定强调色（§5.2）。 */
  Instance: "instance",
} as const;
export type ThemePref = (typeof ThemePref)[keyof typeof ThemePref];

/** 解析后的明暗（**只有两个值** —— `System` 与 `Instance` 都要落到其中之一）。 */
export const ResolvedScheme = { Dark: "dark", Light: "light" } as const;
export type ResolvedScheme = (typeof ResolvedScheme)[keyof typeof ResolvedScheme];

/**
 * 材质档位（§3.4）。
 *
 * ⚠️ **它们不是平权的三选一** —— §5.4.1 第 2 层说的是"**自动、只降不升**"。
 * 见 [`resolveMaterial`]。
 */
export const MaterialPref = {
  /** 窗口取 DWM 材质（Mica / Acrylic）。 */
  Full: "full",
  /** 降一级。 */
  Reduced: "reduced",
  /** 纯色兜底。 */
  None: "none",
} as const;
export type MaterialPref = (typeof MaterialPref)[keyof typeof MaterialPref];

/** 视觉强度（§5.4.1 第 3 层）。 */
export const IntensityPref = { Standard: "standard", Enhanced: "enhanced" } as const;
export type IntensityPref = (typeof IntensityPref)[keyof typeof IntensityPref];

/** 系统能力的现状（**第 1 层**，界面改不了它）。 */
export interface SystemCapabilities {
  /** `prefers-color-scheme: dark`。 */
  readonly prefersDark: boolean;
  /** `prefers-contrast: more`。 */
  readonly highContrast: boolean;
  /** `prefers-reduced-motion: reduce`。 */
  readonly reducedMotion: boolean;
  /** 省电模式（§3.4 的"降一级"）。 */
  readonly batterySaver: boolean;
  /** 远程桌面（§3.4 的"纯色"）。 */
  readonly remoteSession: boolean;
}

/** 用户的偏好（**第 2/3/4 层**）。 */
export interface UserPrefs {
  readonly theme: ThemePref;
  readonly material: MaterialPref;
  readonly intensity: IntensityPref;
  /** **实例指定的强调色 ID**（`Instance` 那一态用它；见 §5.3 的"预设色板"）。 */
  readonly instanceAccent?: string | undefined;
}

/** 解析结果 —— **它就是挂到 DOM 上的那三个 `data-*`**。 */
export interface ResolvedAppearance {
  /** `data-theme` */
  readonly theme: ResolvedScheme;
  /** `data-material` */
  readonly material: MaterialPref;
  /** `data-intensity` */
  readonly intensity: IntensityPref;
  /**
   * **被第 1 层收紧过吗。**
   *
   * ⚠️ 它**不是"错误"** —— 界面**不该**为它报错或提示（§5.4.1 纪律 ①：
   * "不生效，**且不报错**"）。它存在的唯一用途是**设置页显示当前生效值**
   *（让用户知道"我选了增强，而现在因为高对比度没生效"）。
   */
  readonly tightenedBySystem: boolean;
  /** 实例强调色 ID（`theme === instance` 时有意义）。 */
  readonly accent: string | undefined;
}

/**
 * **明暗解析**。
 *
 * ⚠️ `Instance` 这一态**在明暗上等同于 `System`** —— 因为"跟随实例"只影响
 * **强调色**，不影响明暗。一个把 `Instance` 当成"第三种明暗"的实现会
 * 需要第三套色板，而那与 §5.2 的"主题只提供调色板"直接冲突。
 */
export function resolveScheme(pref: ThemePref, sys: SystemCapabilities): ResolvedScheme {
  if (pref === ThemePref.Dark) return ResolvedScheme.Dark;
  if (pref === ThemePref.Light) return ResolvedScheme.Light;
  // `System` 与 `Instance` 都看系统。
  return sys.prefersDark ? ResolvedScheme.Dark : ResolvedScheme.Light;
}

/**
 * **材质解析** —— 而它是"**只降不升**"的落点（§5.4.1 第 2 层）。
 *
 * | 触发 | 动作（§3.4 的表） |
 * |---|---|
 * | 高对比度 | **强制纯色** |
 * | 远程桌面 | **纯色** |
 * | 省电模式 | **降一级** |
 *
 * ## 🔴 而"降一级"不是"设成 Reduced"
 *
 * 一个把省电模式写死成 `Reduced` 的实现会在**用户已经选了 `None`** 时
 * 把它**升回** `Reduced` —— 而那是"放开"，正是纪律 ① 禁止的。
 *
 * 所以它是 `min(用户的选择, 系统允许的上限)` —— **取更严的那一个**。
 */
export function resolveMaterial(pref: MaterialPref, sys: SystemCapabilities): MaterialPref {
  // ⚠️ **一个数组，不是两个。**
  //
  // 我第一版写了 `order`（松 → 紧）与 `byRank`（紧 → 松）**两个顺序相反**的数组，
  // 然后把 `order` 的下标拿去索引 `byRank` —— 于是 `Full` 变成了 `None`。
  // 而那一版**编译通过**，测试才抓到它。
  const steps: readonly MaterialPref[] = [MaterialPref.Full, MaterialPref.Reduced, MaterialPref.None];
  const rank = (m: MaterialPref): number => steps.indexOf(m);
  const at = (i: number): MaterialPref => steps[Math.min(Math.max(i, 0), steps.length - 1)] ?? MaterialPref.None;

  // **两个"强制纯色"的触发**（§3.4 的表）：高对比度、远程桌面。
  if (sys.highContrast || sys.remoteSession) {
    return MaterialPref.None;
  }
  // **"降一级"是在用户的选择上降**，不是设成固定值 —— 所以是 `min`。
  // 一个写死 `Reduced` 的实现会在用户已选 `None` 时把它**升回去**。
  if (sys.batterySaver) {
    return at(Math.max(rank(pref), rank(MaterialPref.Reduced)));
  }
  return pref;
}

/**
 * **视觉强度解析** —— 纪律 ① 最直接的落点。
 *
 * 高对比度下 `Enhanced` **不生效**（变成 `Standard`），**且不报错**。
 */
export function resolveIntensity(
  pref: IntensityPref,
  sys: SystemCapabilities,
): { readonly value: IntensityPref; readonly tightened: boolean } {
  if (sys.highContrast && pref === IntensityPref.Enhanced) {
    // ⚠️ **它不抛、不警告、不返回错误** —— 只是被收紧了。
    return { value: IntensityPref.Standard, tightened: true };
  }
  return { value: pref, tightened: false };
}

/**
 * **一次解析出挂到 DOM 上的三个属性。**
 *
 * 而它是本模块**唯一**该被界面调用的函数 —— 三个 `resolveXxx` 是它的零件，
 * 分开调用会让"谁覆盖谁"在调用点被重新拼一遍。
 */
export function resolveAppearance(prefs: UserPrefs, sys: SystemCapabilities): ResolvedAppearance {
  const theme = resolveScheme(prefs.theme, sys);
  const material = resolveMaterial(prefs.material, sys);
  const { value: intensity, tightened } = resolveIntensity(prefs.intensity, sys);

  // ⚠️ `accent` 只在 `Instance` 那一态有意义 —— 一个无条件带上它的实现
  // 会让"跟随系统"的用户也拿到实例的颜色。
  const accent = prefs.theme === ThemePref.Instance ? prefs.instanceAccent : undefined;

  return {
    theme,
    material,
    intensity,
    // "被收紧"的判据是**任一层**被压过，而不只是强度那一层。
    //
    // ⚠️ 我第一版在这里写了一个**毫无意义的比较**
    //（`theme !== resolveScheme(prefs.theme, { ...sys, prefersDark: sys.prefersDark })` ——
    // 那永远为 `false`）。而这类"看起来在做判断而其实恒假"的表达式
    // **编译器不会报**，它只会让设置页永远不显示"当前生效值"。
    tightenedBySystem: tightened || material !== prefs.material,
    accent,
  };
}

/**
 * 把解析结果**写到 `document.documentElement` 上**。
 *
 * ⚠️ **它是本模块唯一的副作用**，而它刻意与解析分开：
 * 解析可以（也应该）在测试里跑几百次，而"改 DOM"是一次性的。
 *
 * 而三个 `data-*` 的属性名**与 `tokens.css` 里的选择器一一对应** ——
 * 有一条测试核对这一点（改了 CSS 却忘了改这里，界面会静默失去主题）。
 */
export function applyAppearance(
  resolved: ResolvedAppearance,
  root: HTMLElement | null = globalThis.document?.documentElement ?? null,
): void {
  if (root === null) return;
  root.dataset["theme"] = resolved.theme;
  root.dataset["material"] = resolved.material;
  root.dataset["intensity"] = resolved.intensity;
  if (resolved.accent === undefined) {
    delete root.dataset["accent"];
  } else {
    root.dataset["accent"] = resolved.accent;
  }
}

/** `tokens.css` 里那三个属性名 —— **唯一真源**。 */
export const DATA_ATTRS = ["theme", "material", "intensity"] as const;
