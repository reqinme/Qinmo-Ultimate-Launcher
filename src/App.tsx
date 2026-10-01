import type { ReactElement } from "react";
import type { Capabilities } from "./api/contract.ts";

/**
 * 外壳的根组件。
 *
 * **它有意做得很少**，因为"前端零业务逻辑"是这个项目的硬约束：
 * 界面只把后端给的数据画出来，不做判断、不写死产品知识。
 *
 * U0 阶段它只做一件事：把能力表按"可用 / 不可用（附原因）"两组渲染出来。
 * 这已经足够验证三件事同时成立：
 *   1. Rust ↔ TS 的契约形状能对上；
 *   2. `reason` 在禁用态确实存在（而且**没有**禁用态缺原因）；
 *   3. 界面不写 `if (product === ...)` 也能把差异表达清楚。
 */
export function App({ capabilities }: { readonly capabilities: Capabilities }): ReactElement {
  const entries = Object.entries(capabilities).sort(([a], [b]) => a.localeCompare(b));
  const held = entries.filter(([, c]) => c.enabled);
  const blocked = entries.filter(([, c]) => !c.enabled);

  return (
    <main className="shell">
      <header className="shell__head">
        <h1 className="shell__title">秦墨 · 外壳骨架</h1>
        <p className="shell__note">
          U0 验收页：证明 Rust 契约能流到界面，且禁用态**必须**带原因。
        </p>
      </header>

      <section className="panel">
        <h2 className="panel__title">
          可用 <span className="badge">{held.length}</span>
        </h2>
        <ul className="caps">
          {held.map(([key]) => (
            <li key={key} className="caps__item caps__item--on">
              {key}
            </li>
          ))}
        </ul>
      </section>

      <section className="panel">
        <h2 className="panel__title">
          不可用 <span className="badge">{blocked.length}</span>
        </h2>
        <ul className="caps">
          {blocked.map(([key, cap]) => (
            <li
              key={key}
              className="caps__item caps__item--off"
              /* 原因直接来自后端——界面不解释、不翻译、不猜测 */
              title={cap.enabled ? undefined : cap.reason}
            >
              {key}
              <span className="caps__reason">{cap.enabled ? "" : cap.reason}</span>
            </li>
          ))}
        </ul>
      </section>
    </main>
  );
}
