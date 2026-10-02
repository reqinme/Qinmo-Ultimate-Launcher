/**
 * 「实例详情」—— 骨架（§4.6.3 / §7.2 状态表）
 * ============================================================================
 *
 * ## 🔴 这一页第一次让"实例"有一个**地址**
 *
 * 实例 id 从路由参数里读（`useParams({ from: "/instances/$instanceId" })`），
 * 而不是从某个全局状态里取 —— 于是"把详情页的链接发给别人"、
 * "刷新后还在同一页"、"后退回到列表"这三件事**自动成立**。
 *
 * ⚠️ 而它能显示的**只有 id**：`instance_summary` 那条命令要一个真实存在的
 * 实例（`web/src/api/` 里的 `fetchInstanceSummary`），今天一个实例都没有，
 * 所以这里**不显示名字、不显示版本、不显示任何编出来的状态** ——
 * 那三样要等实例列表与安装流程落地（M6）。
 *
 * ## ⚠️ 而"状态"这件事在这一页上是有规格的
 *
 * §7.2 要求实例状态用**五阶段**表达（检测 → 准备 → 下载 → 部署 → 启动），
 * 而 §7.3 要求"进行中的那一步"是唯一被强调的那一格。今天没有状态可显示，
 * 所以这一页把"状态会怎么显示"写在文案里，而不是画五个灰格子假装在跑。
 */

import { Link, useParams } from "@tanstack/react-router";
import type { ReactElement } from "react";
import { PageState } from "../components/PageState.tsx";
import "./pages.css";

export function InstanceDetailPage(): ReactElement {
  const { instanceId } = useParams({ from: "/instances/$instanceId" });
  return (
    <section>
      <h1 className="page__title">{`实例 · ${instanceId}`}</h1>
      <p className="page__note">
        一个实例 = 一份版本 + 一个加载器 + 一套运行时 + 它自己的目录。
        这一页将来是它的"体检报告"：版本、运行时、加载器、模组数、
        以及它在哪一步。
      </p>
      <PageState
        kind="empty"
        title="这个实例的详情还没有数据源"
        body="内核现在只提供「给定 id 取一份摘要」这一条命令，而没有「列出我这台机器上装过什么」，所以一个实例从哪来、叫什么名字都还不知道。这一页只把路由参数里的 id 显示出来，不编造其余内容。"
        action={
          <Link className="page__action" to="/instances">
            回到实例列表
          </Link>
        }
      />
    </section>
  );
}
