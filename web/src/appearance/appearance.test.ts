/**
 * 主题 · 材质 · 强度的解析测试（**M4 主干** · §5.4.1）
 * ============================================================================
 *
 * ## 它测的实质是**纪律 ①**
 *
 * §5.4.1 原文：
 *
 * > **向下只累加收紧，不叠加放开。** 第 1 层说不行，第 2/3/4 层**都不得放开**。
 * > 高对比度模式下，`Visual.Enhanced` **不生效，且不报错** —— 它只是被上层收紧了。
 *
 * 而"只降不升"这个性质**只能靠一组成对的断言**表达：
 * 每一档在每一个系统条件下，结果必须**不松于**用户的选择。
 */

import { describe, expect, it } from "vitest";
import {
  DATA_ATTRS,
  IntensityPref,
  MaterialPref,
  ThemePref,
  applyAppearance,
  resolveAppearance,
  resolveIntensity,
  resolveMaterial,
  resolveScheme,
  type SystemCapabilities,
  type UserPrefs,
} from "./appearance.ts";

/** 一个"什么都没触发"的系统。 */
const CALM: SystemCapabilities = {
  prefersDark: true,
  highContrast: false,
  reducedMotion: false,
  batterySaver: false,
  remoteSession: false,
};

const PREFS: UserPrefs = {
  theme: ThemePref.System,
  material: MaterialPref.Full,
  intensity: IntensityPref.Standard,
};

const ALL_MATERIALS: readonly MaterialPref[] = [
  MaterialPref.Full,
  MaterialPref.Reduced,
  MaterialPref.None,
];

/** 严格程度的序号：**越大越松**（`Full` 最松）。 */
const LOOSENESS: Record<MaterialPref, number> = {
  [MaterialPref.Full]: 2,
  [MaterialPref.Reduced]: 1,
  [MaterialPref.None]: 0,
};

// ============================================================================
// 主题四态
// ============================================================================

describe("主题四态（§5.2）", () => {
  it("深/浅是显式的，**不受系统影响**", () => {
    // ⚠️ 一个"显式选深色但系统是浅色时被覆盖"的实现，会让用户在
    // 深色系统上**没法**选浅色 —— 而那是最基本的可用性。
    expect(resolveScheme(ThemePref.Dark, { ...CALM, prefersDark: false })).toBe("dark");
    expect(resolveScheme(ThemePref.Light, { ...CALM, prefersDark: true })).toBe("light");
  });

  it("跟随系统：看 `prefers-color-scheme`", () => {
    expect(resolveScheme(ThemePref.System, { ...CALM, prefersDark: true })).toBe("dark");
    expect(resolveScheme(ThemePref.System, { ...CALM, prefersDark: false })).toBe("light");
  });

  it("**跟随实例在明暗上等同于跟随系统**（它只影响强调色）", () => {
    // ⚠️ 一个把 `Instance` 当成"第三种明暗"的实现会需要**第三套色板** ——
    // 而那与 §5.2 的"主题只提供调色板"直接冲突。
    expect(resolveScheme(ThemePref.Instance, { ...CALM, prefersDark: true })).toBe("dark");
    expect(resolveScheme(ThemePref.Instance, { ...CALM, prefersDark: false })).toBe("light");
  });

  it("四种偏好**都**解析成 dark 或 light（没有第三个值）", () => {
    for (const p of Object.values(ThemePref)) {
      const r = resolveScheme(p, CALM);
      expect(["dark", "light"]).toContain(r);
    }
  });
});

// ============================================================================
// 材质三档：**只降不升**
// ============================================================================

describe("材质三档只降不升（§3.4 / §5.4.1 第 2 层）", () => {
  it("平静的系统里，**用户的选择原样生效**", () => {
    for (const m of ALL_MATERIALS) {
      expect(resolveMaterial(m, CALM)).toBe(m);
    }
  });

  it("高对比度 ⇒ **强制纯色**（而它也是放开的反面）", () => {
    for (const m of ALL_MATERIALS) {
      expect(resolveMaterial(m, { ...CALM, highContrast: true })).toBe(MaterialPref.None);
    }
  });

  it("远程桌面 ⇒ **强制纯色**", () => {
    for (const m of ALL_MATERIALS) {
      expect(resolveMaterial(m, { ...CALM, remoteSession: true })).toBe(MaterialPref.None);
    }
  });

  it("🔴 省电模式的降一级是在**用户的选择**上降，而不是设成固定值", () => {
    // ⚠️ **这一条是本模块最容易写错的地方。**
    //
    // 一个把省电模式写死成 `Reduced` 的实现会在**用户已经选了 `None`** 时
    // 把它**升回** `Reduced` —— 而那是"放开"，正是纪律 ① 禁止的。
    expect(resolveMaterial(MaterialPref.Full, { ...CALM, batterySaver: true })).toBe(
      MaterialPref.Reduced,
    );
    expect(resolveMaterial(MaterialPref.Reduced, { ...CALM, batterySaver: true })).toBe(
      MaterialPref.Reduced,
    );
    // 而 `None` 必须**留在** `None`
    expect(resolveMaterial(MaterialPref.None, { ...CALM, batterySaver: true })).toBe(
      MaterialPref.None,
    );
  });

  it("🔴 **穷尽**：任何系统条件下，结果都不松于用户的选择", () => {
    // 这条是纪律 ① 的**机器版** —— 它不用逐条列触发条件，
    // 而是把 3 档 × 2^5 种系统组合全跑一遍。
    for (let bits = 0; bits < 32; bits += 1) {
      const sys: SystemCapabilities = {
        prefersDark: (bits & 1) !== 0,
        highContrast: (bits & 2) !== 0,
        reducedMotion: (bits & 4) !== 0,
        batterySaver: (bits & 8) !== 0,
        remoteSession: (bits & 16) !== 0,
      };
      for (const m of ALL_MATERIALS) {
        const got = resolveMaterial(m, sys);
        expect(
          LOOSENESS[got],
          `用户选 ${m}，系统 bits=${bits}，结果 ${got} —— **它比用户的选择更松**`,
        ).toBeLessThanOrEqual(LOOSENESS[m]);
      }
    }
  });

  it("而三种材质**都可能**出现（不是恒等于某一个）", () => {
    const seen = new Set<MaterialPref>();
    for (const m of ALL_MATERIALS) {
      seen.add(resolveMaterial(m, CALM));
      seen.add(resolveMaterial(m, { ...CALM, batterySaver: true }));
      seen.add(resolveMaterial(m, { ...CALM, highContrast: true }));
    }
    expect([...seen].sort()).toEqual(["full", "none", "reduced"]);
  });
});

// ============================================================================
// 视觉强度：**不生效且不报错**
// ============================================================================

describe("视觉强度（§5.4.1 第 3 层 + 纪律 ③）", () => {
  it("平静的系统里原样生效", () => {
    expect(resolveIntensity(IntensityPref.Enhanced, CALM)).toEqual({
      value: IntensityPref.Enhanced,
      tightened: false,
    });
    expect(resolveIntensity(IntensityPref.Standard, CALM)).toEqual({
      value: IntensityPref.Standard,
      tightened: false,
    });
  });

  it("🔴 高对比度下 `Enhanced` **不生效**，而它是**收紧**而不是错误", () => {
    const r = resolveIntensity(IntensityPref.Enhanced, { ...CALM, highContrast: true });
    expect(r.value).toBe(IntensityPref.Standard);
    // ⚠️ 而 `tightened: true` **不是错误码** —— §5.4.1 原文是
    // "**不生效，且不报错**"。它存在的唯一用途是设置页显示"当前生效值"。
    expect(r.tightened).toBe(true);
  });

  it("而 `Standard` 在高对比度下**不算被收紧**（它本来就是那一档）", () => {
    // 一个把"高对比度"一律标成 tightened 的实现会让设置页
    // 对**什么都没失去**的用户显示一句"你的设置被系统收紧了"。
    expect(resolveIntensity(IntensityPref.Standard, { ...CALM, highContrast: true })).toEqual({
      value: IntensityPref.Standard,
      tightened: false,
    });
  });

  it("`reducedMotion` **不影响**视觉强度（它是另一条轴）", () => {
    // §5.4.1 的第 1 层包含 `prefers-reduced-motion`，而它的作用是
    // **归零动效**（在 CSS 里），不是把 `Enhanced` 降级 ——
    // 装饰性渐变在"减少动态"下**照旧可以存在**（它不动）。
    expect(resolveIntensity(IntensityPref.Enhanced, { ...CALM, reducedMotion: true }).value).toBe(
      IntensityPref.Enhanced,
    );
  });
});

// ============================================================================
// 合成
// ============================================================================

describe("`resolveAppearance`：一次解析出三个 `data-*`", () => {
  it("平静的系统：三个轴都原样", () => {
    const r = resolveAppearance(PREFS, CALM);
    expect(r.theme).toBe("dark");
    expect(r.material).toBe(MaterialPref.Full);
    expect(r.intensity).toBe(IntensityPref.Standard);
    expect(r.tightenedBySystem).toBe(false);
  });

  it("🔴 高对比度下：材质纯色 + 强度回标准 + `tightened` 为真", () => {
    const r = resolveAppearance(
      { ...PREFS, material: MaterialPref.Full, intensity: IntensityPref.Enhanced },
      { ...CALM, highContrast: true },
    );
    expect(r.material).toBe(MaterialPref.None);
    expect(r.intensity).toBe(IntensityPref.Standard);
    expect(r.tightenedBySystem).toBe(true);
  });

  it("🔴 `tightenedBySystem` **不是恒假**（我第一版写成了恒假）", () => {
    // ⚠️ 那个表达式（`theme !== resolveScheme(theme, {...sys, prefersDark: sys.prefersDark})`）
    // **编译器不会报**，它只会让设置页永远不显示"当前生效值"。
    const changed = resolveAppearance(
      { ...PREFS, material: MaterialPref.Full },
      { ...CALM, remoteSession: true },
    );
    expect(changed.tightenedBySystem).toBe(true);
    const unchanged = resolveAppearance(PREFS, CALM);
    expect(unchanged.tightenedBySystem).toBe(false);
  });

  it("强调色**只在 `Instance` 那一态**被带上", () => {
    // ⚠️ 一个无条件带上它的实现会让"跟随系统"的用户也拿到实例的颜色。
    const inst = resolveAppearance(
      { ...PREFS, theme: ThemePref.Instance, instanceAccent: "ocean" },
      CALM,
    );
    expect(inst.accent).toBe("ocean");

    const sys = resolveAppearance(
      { ...PREFS, theme: ThemePref.System, instanceAccent: "ocean" },
      CALM,
    );
    expect(sys.accent).toBeUndefined();
  });
});

// ============================================================================
// 应用到 DOM
// ============================================================================

describe("`applyAppearance`：写 `data-*`", () => {
  it("三个属性都写上了，且名字与 `tokens.css` 的选择器一致", () => {
    const el = document.createElement("div");
    applyAppearance(
      {
        theme: "light",
        material: MaterialPref.None,
        intensity: IntensityPref.Enhanced,
        tightenedBySystem: false,
        accent: undefined,
      },
      el,
    );
    // ⚠️ 这三个名字**必须**与 `tokens.css` 里的 `[data-theme=…]` 等选择器
    // 一一对应 —— 改了一边忘了另一边，界面会**静默失去主题**。
    expect(el.dataset["theme"]).toBe("light");
    expect(el.dataset["material"]).toBe("none");
    expect(el.dataset["intensity"]).toBe("enhanced");
    expect([...DATA_ATTRS]).toEqual(["theme", "material", "intensity"]);
  });

  it("没有强调色时**删掉**那个属性（而不是写上 `undefined`）", () => {
    const el = document.createElement("div");
    el.dataset["accent"] = "old";
    applyAppearance(
      {
        theme: "dark",
        material: MaterialPref.Full,
        intensity: IntensityPref.Standard,
        tightenedBySystem: false,
        accent: undefined,
      },
      el,
    );
    expect(el.dataset["accent"]).toBeUndefined();
  });

  it("根元素为 `null` 时**不抛**（SSR / 测试环境）", () => {
    expect(() =>
      applyAppearance(
        {
          theme: "dark",
          material: MaterialPref.Full,
          intensity: IntensityPref.Standard,
          tightenedBySystem: false,
          accent: undefined,
        },
        null,
      ),
    ).not.toThrow();
  });
});
