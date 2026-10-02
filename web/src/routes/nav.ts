/**
 * 两段式导航的结构（**M4 门禁第 ③ 项**）
 * ============================================================================
 *
 * 单一真源：`docs/UI设计规格.md` §4.6.1 / §4.6.1.1 / §4.6.1.2。
 *
 * ## 🔴 三条硬规则，以及它们为什么必须是类型而不是约定
 *
 * ### §4.6.1.1 **二级栏内容由当前一级决定；一级没有下级时二级栏为空**
 *
 * 规格原文就是这么写的。而它的反例在别的启动器里到处可见：
 * **一个全局固定的二级栏**，于是"在设置页里看到下载的分组"。
 *
 * 本文件用**一个联合类型 + 一个穷尽 switch** 落实它：
 * `secondaryOf(primary)` 的返回类型**要么是一个 `SecondaryArea`，要么是 `null`**，
 * 而"没有下级"的那一支**没有别的可能**（不是空的，是 `null` —— 见下）。
 *
 * ### §4.6.1.1 的**区**：二级栏不是一条平铺的列表
 *
 * 规格 §4.6.1.1 的四页对应表写得很具体：
 *
 * | 一级 | 二级栏里有什么 |
 * |---|---|
 * | 主页 | （没有二级栏） |
 * | 实例 | **产品** + 分组 |
 * | 下载 | 可下载的类型 + **装到哪个实例** |
 * | 设置 | 设置类别 + **按产品** |
 * | 关于 | （没有二级栏） |
 *
 * 所以二级栏的数据是 `SecondaryArea`：**若干个区**（`sections`，数组顺序
 * 就是界面上的顺序）+ **一张平铺的项表**（`items`，每一项用 `section`
 * 说明自己属于哪个区）。
 *
 * ⚠️ **它是扁平的，不是嵌套的** —— 嵌套（区里装项）读起来更自然，但那样
 * 这张表就没法被逐行读出来，而 `tools/check-shell-contract.ps1` 要拿它
 * 与规格里那张表**逐格核对**。"能被机器核对"是这一层存在的理由。
 *
 * ⚠️ **区的形式是"竖排列表项"**（§4.6.1.1）：横排的分段控件只用于
 * "筛选 / 排序"这类**页内**控件，**不表达层级**。
 *
 * ### §4.6.1.2 **「当前产品」是全局状态，单一真源**
 *
 * 所以本文件**不导出任何"当前产品"的变量** —— 它由 Rust 侧的能力描述符
 * 与设置里的单一字段决定，而**导航不参与判断**。
 * 一个"导航里也存一份当前产品"的实现会立刻产生两个真相来源。
 *
 * 而"产品"那一个区的 `source` 是 `products`：**它说明这一区的内容
 * 不是写在这里的，而是来自那份全局状态**。
 *
 * ## ⚠️ 而"没有下级"用 `null` 而不是空区
 *
 * 因为两者在**渲染上等价、在语义上不同**：
 *
 * | | 含义 | 界面该做什么 |
 * |---|---|---|
 * | 有区、无项 | 这个一级**有**二级区，而此刻它是空的 | 保留二级栏的位置（宽度占位） |
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

/**
 * 二级栏里一个区的**数据来源**。
 *
 * | 值 | 谁提供内容 | 今天是 |
 * |---|---|---|
 * | `static` | 本文件（`items` 里写死） | 全部可用 |
 * | `products` | §4.6.1.2 那份全局的「当前产品」状态 | **已有状态源**（`web/src/product/product.tsx`） |
 * | `instances` | 实例列表（`qul-core` 侧） | **还没有那份数据** ⇒ 界面只给一句事实说明，不编数 |
 */
export type SectionSource = "static" | "products" | "instances";

/** 二级栏里的一个区（**只有标题与来源，项在 `SecondaryArea.items` 里**）。 */
export interface SecondarySection {
  readonly key: string;
  /** **已经本地化的**标题。 */
  readonly label: string;
  readonly source: SectionSource;
  /**
   * 这一区里**落在一级页本身**的那一项的 key；`null` ⇒ 每一项都有自己的路径。
   *
   * ⚠️ 这一列存在的理由是一次真缺陷：`secondaryHref()` 机械地拼
   * `一级路径 + "/" + 项 key`，而它的返回类型是 `string`（不是路由字面量联合），
   * 所以类型系统看不见"这一项指向一条不存在的路由"。于是
   * **账户列表 / 内建帮助 / 外观与材质 / 全部实例**这四项曾经指着
   * `/accounts/list`、`/toolbox/help`、`/settings/appearance`、`/instances/all`
   * —— 四条都不存在，真窗口里点下去是 `Not Found`。
   *
   * 而它们的正确落点就是一级页本身：`/accounts` 是账户列表、
   * `/toolbox` 是内建帮助、`/settings` 是外观与材质、`/instances` 是全部实例。
   * 这件事**只有写下来才有人能核对** —— 所以它是一列数据，不是一句注释。
   *
   * 钉住它的是 `tools/check-shell-contract.ps1` 的**检查 G**：
   * 每个静态二级项的路径都必须有一条同路径的路由，除非它就是一级页。
   */
  readonly rootItem: string | null;
}

/** 一个二级项（只描述"长什么样"，**不含跳转逻辑**）。 */
export interface SecondaryItem {
  readonly key: string;
  /** **已经本地化的**标签。界面不做翻译 —— 文案由 Rust 侧的 i18n 给。 */
  readonly label: string;
  /** 它属于哪个区 —— 必须是同一级 `sections` 里的一个 `key`（有测试钉着）。 */
  readonly section: string;
}

/** 一个一级的二级栏内容：**区的顺序 + 一张平铺的项表**。 */
export interface SecondaryArea {
  readonly sections: readonly SecondarySection[];
  readonly items: readonly SecondaryItem[];
}

/** 一级项。 */
export interface PrimaryItem {
  readonly key: PrimaryKey;
  readonly label: string;
  /**
   * 这个一级的二级内容。
   *
   * - `null` ⇒ **没有二级区**（§4.6.1.1：二级栏为空）
   * - 否则  ⇒ 有二级区，内容是这些区与项
   */
  readonly secondary: SecondaryArea | null;
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
 *   （有意留白 —— 预留给产品列表的增长）
 *   ─────────────
 *   账号管理 · 百宝箱 · 设置 · 关于   ← 下段：从"要干活"渐进到"配置与信息"
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
    secondary: {
      sections: [
        // ⚠️ **顺序就是 §4.6.1.1 那张表的顺序：产品在前，分组在后。**
        // 产品那一行选中的是"看哪个产品的实例"（§4.6.1.2），
        // 而分组是在**那个产品之内**再分（全部 / 最近 / 收藏）。
        { key: "product", label: "产品", source: "products", rootItem: null },
        // ⚠️ 「全部实例」**就是这一页本身**（`/instances` 是实例列表）⇒ `rootItem`。
        { key: "group", label: "分组", source: "static", rootItem: "all" },
      ],
      items: [
        { section: "group", key: "all", label: "全部实例" },
        { section: "group", key: "recent", label: "最近使用" },
        { section: "group", key: "favorites", label: "收藏" },
      ],
    },
  },
  {
    key: "downloads",
    label: "下载",
    secondary: {
      sections: [
        { key: "type", label: "类型", source: "static", rootItem: null },
        // ⚠️ **「装到」是这一页的必要一半**：§4.6.7 的一句话是
        // "下载页负责**装进来**"，而"装到哪儿"必须在这里选 ——
        // 否则用户会在别处（实例内部）发现第二个入口。
        { key: "target", label: "装到", source: "instances", rootItem: null },
      ],
      items: [
        { section: "type", key: "versions", label: "游戏版本" },
        { section: "type", key: "loaders", label: "加载器" },
        { section: "type", key: "mods", label: "模组" },
        { section: "type", key: "resourcepacks", label: "资源包" },
        { section: "type", key: "shaders", label: "光影" },
        { section: "type", key: "worlds", label: "存档" },
      ],
    },
  },
  {
    key: "accounts",
    label: "账号管理",
    // ⚠️ **规格里这两页的下级写着"暂未设计"**（§4.6.1.1 的对应表）。
    // 这两项是**代码先落地**的形态 —— 而规格落后于代码这件事要写下来
    // （§4.6.1.1 的对应表在 v2.4 里补上了它们）。
    secondary: {
      // ⚠️ 「账户列表」**就是这一页本身**（`/accounts` 是账户列表）⇒ `rootItem`。
      sections: [{ key: "account", label: "账户", source: "static", rootItem: "list" }],
      items: [
        { section: "account", key: "list", label: "账户列表" },
        { section: "account", key: "auth", label: "授权状态" },
      ],
    },
  },
  {
    key: "toolbox",
    label: "百宝箱",
    // ⚠️ **它在下段，而它有自己的二级分组** —— 这六项恰好是 §4.3 的
    // "首批 6 件工具"（内建帮助 / 日志与诊断 / 测速 / 依赖与组件检查 /
    // 磁盘清理 / 投影材质查看）。
    secondary: {
      // ⚠️ 「内建帮助」**就是这一页本身**（`/toolbox` 的标题就是它）⇒ `rootItem`。
      sections: [{ key: "tool", label: "工具", source: "static", rootItem: "help" }],
      items: [
        { section: "tool", key: "help", label: "内建帮助" },
        { section: "tool", key: "logs", label: "日志与诊断" },
        { section: "tool", key: "speedtest", label: "测速" },
        { section: "tool", key: "components", label: "依赖与组件检查" },
        { section: "tool", key: "cleanup", label: "磁盘清理" },
        { section: "tool", key: "schematic", label: "投影材质查看" },
      ],
    },
  },
  {
    key: "settings",
    label: "设置",
    secondary: {
      sections: [
        // ⚠️ 「外观与材质」**就是这一页本身**（`/settings` 的标题就是它）⇒ `rootItem`。
        { key: "category", label: "类别", source: "static", rootItem: "appearance" },
        // ⚠️ **「按产品」那一区与「实例」页的「产品」区是同一个状态源**
        // （§4.6.1.2 的方案 A：三处都能切、三处联动）。
        { key: "byProduct", label: "按产品", source: "products", rootItem: null },
      ],
      items: [
        { section: "category", key: "appearance", label: "外观与材质" },
        // ⚠️ **原来这里是 `{ key: "java", label: "Java 与运行时" }`** ——
        // 而那条"导航标签里不许出现产品名"的测试把它拦了下来，
        // **而它拦得对**：一个让界面必须写出产品名的标签，
        // 正是门禁⑤ 那条规则要拦的形态（界面不该认识具体游戏）。
        //
        // 而"运行时"是语言中立的说法，且它更准确 —— 这一页管的是
        // 运行时（探测、选择、内存、JVM 参数），而现役运行时恰好是一个实现。
        { section: "category", key: "runtime", label: "运行时" },
        { section: "category", key: "network", label: "网络与镜像" },
        { section: "category", key: "storage", label: "存储与目录" },
        { section: "category", key: "advanced", label: "高级" },
      ],
    },
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
 * 而"有区但没有项"表示"有二级区但此刻为空" —— 见文件头那段说明。
 *
 * ⚠️ 这个函数**没有 `currentProduct` 参数**，那是刻意的（§4.6.1.2）：
 * 二级栏的**结构**只由一级决定。产品只决定"产品/按产品"那一区**里面**
 * 显示哪几行（那是 `web/src/product/product.tsx` 的事）。
 */
export function secondaryOf(primary: PrimaryKey): SecondaryArea | null {
  return primaryOf(primary).secondary;
}
