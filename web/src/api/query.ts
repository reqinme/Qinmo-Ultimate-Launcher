/**
 * 数据获取与缓存的接线（**M4 门禁第 ④ 项**）
 * ============================================================================
 *
 * 选型理由（`docs/方案-重新立意版.md` §4.5）：
 *
 * > **TanStack Query** —— 版本清单、模组列表、下载进度都需要缓存与失效策略；
 * > **缓存 key 直接用实例 id / 版本 id**，与内核的实例模型一一对应。
 *
 * ## 🔴 这个文件的形状就是两条纪律
 *
 * ### ① 查询键**由内核的标识构成**，不是界面编出来的
 *
 * `qk.instance(id)` / `qk.capabilities()` —— 见下面。一个"界面自己拼字符串
 * 当 key"的实现会让"同一个实例在两处用不同的 key"变成**静默的重复请求**。
 *
 * ### ② **`retry: false` 是刻意的**
 *
 * TanStack Query 的默认值是重试 3 次（指数退避）。而我们的失败**大多数不是
 * 瞬时的**：契约不合法、能力被禁用、实例不存在 —— 那些重试三次的结果只是
 * **让用户多等 7 秒才看到同一个错误**。
 *
 * 所以默认不重试，而**要重试的地方必须显式打开**（例如网络下载 ——
 * 那是 `qul-core` 的下载引擎在做，不在这一层）。
 */

import { QueryClient, type DefaultOptions } from "@tanstack/react-query";

/**
 * 查询键的**唯一构造处**。
 *
 * ⚠️ **不要在组件里写 `["instance", id]`。** 一个拼错的键不会报错，
 * 它只会让"同一个数据被取两次"，而那在界面上表现为**莫名其妙的多余请求**。
 */
export const qk = {
  /** 全部能力的结论（方案 §3.3 的能力描述符）。 */
  capabilities: () => ["capabilities"] as const,
  /** 一个实例的摘要。**键里是实例 id**，与内核的实例模型一一对应。 */
  instance: (id: string) => ["instance", id] as const,
  /** 一个实例的能力表（**按实例算**，因为能力是实例相关的）。 */
  instanceCapabilities: (id: string) => ["instance", id, "capabilities"] as const,
  /** 版本清单。**它与实例无关**，所以键里没有实例 id。 */
  versionManifest: () => ["versions", "manifest"] as const,
} as const;

/**
 * 默认选项。
 *
 * ⚠️ **`staleTime` 不是一个"性能参数"，它是一条产品判断**：
 * 内核那边变了吗？
 *
 * - 能力表随实例的形态变 ⇒ **短**（30 秒）
 * - 版本清单随官方发布变 ⇒ **长**（1 小时）
 *
 * 所以全局只给一个保守的默认值，而**每一条查询自己声明它的 `staleTime`**
 * —— 一个"全局 staleTime = 5 分钟"的实现会让"刚换了实例形态却还看到旧能力"
 * 变成一个偶发且难以复现的 bug。
 */
const defaults: DefaultOptions = {
  queries: {
    retry: false,
    staleTime: 30_000,
    /**
     * **窗口重新聚焦时不自动重取。**
     *
     * 默认值是 `true`（回到窗口就刷新）。而我们的数据大多来自**本机** ——
     * 切出去看一眼浏览器再回来就重取一次，代价是**一个本不需要的 IPC 往返**，
     * 而收益接近于零（本机数据不会在 5 秒里变）。
     *
     * 要刷新的地方自己提供刷新按钮 —— 那比"悄悄重取"更可解释。
     */
    refetchOnWindowFocus: false,
  },
  mutations: {
    retry: false,
  },
};

/**
 * ⚠️ **单例，而不是在 `main.tsx` 里 `new` 一个传给 Provider。**
 *
 * 因为 `web/src/api/` 的边界层函数可能需要在**组件之外**读缓存
 *（例如"启动完成后让实例查询失效"）。一个只存在于 React 树里的 client
 * 会让那件事做不到。
 *
 * 而"单例"在这里是安全的：**这一个前端进程只有一个数据源**。
 */
export const queryClient = new QueryClient({ defaultOptions: defaults });

/**
 * 一个**薄**的、带类型的取数助手。
 *
 * ⚠️ 它存在是为了让"取数的三件事"（键 / 取数函数 / 选项）在**一处**被看到。
 * 而它**不做**任何数据加工 —— 加工属于 Rust 侧（§2 的"前端零业务逻辑"）。
 */
export function queryOptionsFor<T>(
  key: readonly unknown[],
  fetch: () => Promise<T>,
  staleTime: number,
): { queryKey: readonly unknown[]; queryFn: () => Promise<T>; staleTime: number } {
  return { queryKey: key, queryFn: fetch, staleTime };
}
