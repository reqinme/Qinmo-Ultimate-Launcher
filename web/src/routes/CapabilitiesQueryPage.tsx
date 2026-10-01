import type { ReactElement } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchCapabilities } from "../api/index.ts";
import { qk } from "../api/query.ts";

/**
 * 数据层贯通示例（**M4 门禁第 ④ 项**）
 * ============================================================================
 *
 * ## 它证明的是"一条数据从 Rust 流到界面"这条链的**每一段都在位**
 *
 * ```text
 *   Rust 的能力描述符（{ key, enabled, reason }）
 *      ↓  IPC（M1 的剩余项接上；现在是桩）
 *   web/src/api/index.ts       ← 边界层 + 契约校验
 *      ↓
 *   web/src/api/query.ts       ← 查询键 + 缓存策略
 *      ↓
 *   TanStack Query 的三种状态（pending / error / success）
 *      ↓
 *   组件只渲染，不判断
 * ```
 *
 * ## 🔴 三条状态**都要**有可见的形态
 *
 * 一个只画"成功态"的示例页会让缓存策略与错误处理变成**没有地方验证的东西**。
 * 所以下面三支都在，而门禁④ 的验收就是"三条路都能被看到"。
 *
 * ## ⚠️ 而它不做任何数据加工
 *
 * 计数、排序、分组 —— 都是**渲染**的一部分（它不需要产品知识）。
 * 而"这个能力该不该显示"是**后端的结论**，这里只照着画。
 */
export function CapabilitiesQueryPage(): ReactElement {
  const q = useQuery({
    queryKey: qk.capabilities(),
    queryFn: () => fetchCapabilities(null),
    // 能力随实例的形态变 ⇒ **短**。
    // ⚠️ 这个值写在查询上而不是全局 —— 见 `api/query.ts` 里那段说明。
    staleTime: 30_000,
  });

  if (q.isPending) {
    return (
      <section aria-busy="true">
        <h1 className="page__title">能力表（数据层示例）</h1>
        <p className="page__note">加载中……</p>
        <div className="datastate datastate--pending" role="status">
          请求已发出，界面**不等**它 —— 冷启动期间没有任何网络请求（§8）。
        </div>
      </section>
    );
  }

  if (q.isError) {
    return (
      <section>
        <h1 className="page__title">能力表（数据层示例）</h1>
        <div className="datastate datastate--error" role="alert">
          <p className="datastate__head">取不到能力表</p>
          {/* ⚠️ 错误消息**原样显示**，不翻译、不归纳。
              一个把它们归纳成"加载失败"的实现会让诊断少掉最关键的一行。 */}
          <p className="datastate__detail">{q.error.message}</p>
          <button type="button" className="btn" onClick={() => void q.refetch()}>
            重试
          </button>
        </div>
      </section>
    );
  }

  const entries = Object.entries(q.data).sort(([a], [b]) => a.localeCompare(b));
  const on = entries.filter(([, c]) => c.enabled);
  const off = entries.filter(([, c]) => !c.enabled);

  return (
    <section>
      <h1 className="page__title">能力表（数据层示例）</h1>
      <p className="page__note">
        门禁④ 的贯通示例：Rust 契约 → 边界层 → 查询缓存 → 组件只渲染。
        屏幕上的每一个字都来自后端，界面没有做任何判断。
      </p>

      <div className="datastate datastate--ok">
        <p>
          可用 <strong>{on.length}</strong> 项 · 不可用 <strong>{off.length}</strong> 项
        </p>
      </div>

      <h2 className="page__title" style={{ fontSize: "var(--font-title-sm)" }}>
        不可用（**必须**带原因）
      </h2>
      <ul className="caps">
        {off.map(([key, cap]) => (
          <li key={key} className="caps__item caps__item--off">
            {/* `cap.reason` 在 `enabled === false` 时**必有值** ——
                那是 `Capability` 联合类型保证的，所以这里不需要 `?.`
                也不需要兜底文案。**一个兜底文案会掩盖契约被破坏。** */}
            <span className="caps__reason">{cap.enabled ? "" : cap.reason}</span>
            <span className="caps__key">{key}</span>
          </li>
        ))}
      </ul>

      <h2 className="page__title" style={{ fontSize: "var(--font-title-sm)" }}>
        可用
      </h2>
      <ul className="caps">
        {on.map(([key]) => (
          <li key={key} className="caps__item caps__item--on">
            {key}
          </li>
        ))}
      </ul>
    </section>
  );
}
