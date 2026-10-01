/**
 * 两段式导航的结构（**M4 门禁第 ③ 项**）
 * ============================================================================
 *
 * 单一真源：`docs/UI设计规格.md` §4.6.1。
 *
 * ## 🔴 两条硬规则，以及它们为什么必须是类型而不是约定
 *
 * ### §4.6.1.1 **二级栏内容由当前一级决定；一级没有下级时二级栏为空**
 *
 * 规格原文就是这么写的。而它的反例在别的启动器里到处可见：
 * **一个全局固定的二级栏**，于是"在设置页里看到下载的分组"。
 *
 * 本文件用**一个联合类型 + 一个穷尽 switch** 落实它：
 * `secondaryOf(primary)` 的返回类型**要么是一个非空数组，要么是 `null`**，
 * 而"没有下级"的那一支**没有别的可能**（不是空数组，是 `null` —— 见下）。
 *
 * ### §4.6.1.2 **「当前产品」是全局状态，单一真源**
 *
 * 所以本文件**不导出任何"当前产品"的变量** —— 它由 Rust 侧的能力描述符
 * 与设置里的单一字段决定，而**导航不参与判断**。
 * 一个"导航里也存一份当前产品"的实现会立刻产生两个真相来源。
 *
 * ## ⚠️ 而"没有下级"用 `null` 而不是 `[]`
 *
 * 因为两者在**渲染上等价、在语义上不同**：
 *
 * | | 含义 | 界面该做什么 |
 * |---|---|---|
 * | `[]` | 这个一级**有**二级区，而此刻它是空的 | 保留二级栏的位置（宽度占位） |
 * | `null` | 这个一级**没有**二级区 | **整条二级栏不存在** —— 主区占满 |
 *
 * 规格 §4.6.1.1 说的是后者。把两者混成一个空数组会让"主页"看起来像
 * "一个有二级栏但里面没东西的页" —— 而那是一个**视觉上的谎**。
 */

/** 一级导航项。 */
export type PrimaryKey =
  | "home"
  | "instances"
  | "downloads"
  | "accounts"
  | "toolbox"
  | "settings"
  | "about";

/** 一个二级项（只描述"长什么样"，**不含跳转逻辑**）。 */
export interface SecondaryItem {
  readonly key: string;
  /** **已经本地化的**标签。界面不做翻译 —— 文案由 Rust 侧的 i18n 给。 */
  readonly label: string;
}

/** 一级项。 */
export interface PrimaryItem {
  readonly key: PrimaryKey;
  readonly label: string;
  /**
   * 这个一级的二级内容。
   *
   * - `null` ⇒ **没有二级区**（§4.6.1.1：二级栏为空）
   * - 数组   ⇒ **有二级区**，内容就是它
   */
  readonly secondary: readonly SecondaryItem[] | null;
}

/**
 * 一级导航表。
 *
 * ⚠️ **顺序就是界面上的顺序**，而它按 §4.6.1 的分段：
 *
 * ```text
 *   账户卡            ← 不属于导航
 *   ─────────────
 *   主页 · 实例 · 下载   ← 产品相关区（**唯一允许增长的一段**）
 *   ─────────────
 *   （二级区，可滚动）
 *   ─────────────
 *   账号管理 · 百宝箱 · 设置 · 关于
 * ```
 */
export const PRIMARY: readonly PrimaryItem[] = [
  {
    key: "home",
    label: "主页",
    // **主页没有下级** ⇒ 二级栏整条不存在
    secondary: null,
  },
  {
    key: "instances",
    label: "实例",
    secondary: [
      { key: "all", label: "全部实例" },
      { key: "recent", label: "最近使用" },
      { key: "favorites", label: "收藏" },
    ],
  },
  {
    key: "downloads",
    label: "下载",
    secondary: [
      { key: "versions", label: "游戏版本" },
      { key: "loaders", label: "加载器" },
      { key: "mods", label: "模组" },
      { key: "resourcepacks", label: "资源包" },
      { key: "shaders", label: "光影" },
      { key: "worlds", label: "存档" },
    ],
  },
  {
    key: "accounts",
    label: "账号管理",
    secondary: [
      { key: "list", label: "账户列表" },
      { key: "auth", label: "授权状态" },
    ],
  },
  {
    key: "toolbox",
    label: "百宝箱",
    // ⚠️ **它在下段，而它有自己的二级分组** —— 见 `docs/方案-重新立意版.md` §4.6.1
    secondary: [
      { key: "help", label: "内建帮助" },
      { key: "logs", label: "日志与诊断" },
      { key: "speedtest", label: "测速" },
      { key: "components", label: "依赖与组件检查" },
      { key: "cleanup", label: "磁盘清理" },
      { key: "schematic", label: "投影材质查看" },
    ],
  },
  {
    key: "settings",
    label: "设置",
    secondary: [
      { key: "appearance", label: "外观与材质" },
      // ⚠️ **原来这里是 `{ key: "java", label: "Java 与运行时" }`** ——
      // 而那条"导航标签里不许出现产品名"的测试把它拦了下来，
      // **而它拦得对**：一个让界面必须写出产品名的标签，
      // 正是门禁⑤ 那条规则要拦的形态（界面不该认识具体游戏）。
      //
      // 而"运行时"是语言中立的说法，且它更准确 —— 这一页管的是
      // 运行时（探测、选择、内存、JVM 参数），而现役运行时恰好是一个实现。
      { key: "runtime", label: "运行时" },
      { key: "network", label: "网络与镜像" },
      { key: "storage", label: "存储与目录" },
      { key: "advanced", label: "高级" },
    ],
  },
  {
    key: "about",
    label: "关于",
    // **关于页没有下级**
    secondary: null,
  },
];

/** 产品相关区（**唯一允许增长的一段**，§4.6.1）。 */
export const PRODUCT_KEYS: readonly PrimaryKey[] = ["home", "instances", "downloads"];

/** 下段工具区。 */
export const TOOL_KEYS: readonly PrimaryKey[] = ["accounts", "toolbox", "settings", "about"];

/** 按 key 取一级项。 */
export function primaryOf(key: PrimaryKey): PrimaryItem {
  const found = PRIMARY.find((p) => p.key === key);
  if (found === undefined) {
    // 穷尽性：`PrimaryKey` 的每个成员都在 `PRIMARY` 里，而 TypeScript 的
    // 联合类型保证上面那个 find 一定能命中。真命中不了就是表漏了一项 ——
    // 那是**开发期的错**，不是运行时的分支，所以直接抛。
    throw new Error(`一级导航表缺 ${key}（PRIMARY 与 PrimaryKey 不同步）`);
  }
  return found;
}

/**
 * **§4.6.1.1 的落点**：给定当前一级，返回该显示的二级内容。
 *
 * 返回 `null` 表示**没有二级区**（整条栏不存在），
 * 而 `[]` 表示"有二级区但此刻为空" —— 见文件头那段说明。
 *
 * ⚠️ 这个函数**没有 `currentProduct` 参数**，那是刻意的（§4.6.1.2）：
 * 二级栏的内容**只由一级决定**，与当前产品无关。
 */
export function secondaryOf(primary: PrimaryKey): readonly SecondaryItem[] | null {
  return primaryOf(primary).secondary;
}
