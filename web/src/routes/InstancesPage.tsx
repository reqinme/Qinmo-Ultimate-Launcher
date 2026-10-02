/**
 * 「实例」—— 一级页面的骨架（§4.6.3）
 * ============================================================================
 *
 * ## 🔴 这一页今天**能说什么、不能说什么**
 *
 * 二级栏给了三种看法（全部 / 最近使用 / 收藏），而三种看法都需要
 * **同一件东西**：一份实例清单。而那份清单**没有数据源** ——
 * `web/src/api/index.ts` 只有 `fetchCapabilities` 与 `fetchInstanceSummary(id)`，
 * 没有"列出实例"那条命令。
 *
 * 所以这里**不画假的卡片**，也**不写"暂无实例"** —— 后者是一句关于
 * 这台机器的事实（"一个实例都没有"），而我们今天**无从知道**它
 *（清单接口不在，不等于清单是空的）。这是 §4.2 那一行"不适用 =
 * 禁用态 + 一句原因"的用法：说清"为什么现在是空的"，而不是编一个空。
 */

import { Link } from "@tanstack/react-router";
import type { ReactElement } from "react";
import { PageState } from "../components/PageState.tsx";
import "./pages.css";

export function InstancesPage(): ReactElement {
  return (
    <section>
      <h1 className="page__title">实例</h1>
      <p className="page__note">
        一个实例是一份被托管的游戏目录：版本 · 加载器 · 运行时 · 存档 · 模组，
        全都在它的目录里。
      </p>
      <PageState
        kind="empty"
        title="还不能列出实例"
        body="「列出实例」那条命令今天还没有 —— 内核只给单个实例的摘要（instance_summary，按 id 查）。所以这一页既不画假的卡片，也不写「暂无实例」：后者是一句关于这台机器的事实，而我们今天无从知道它。"
        action={
          <Link className="page__action" to="/">
            去主页启动第一个实例
          </Link>
        }
      />
    </section>
  );
}
