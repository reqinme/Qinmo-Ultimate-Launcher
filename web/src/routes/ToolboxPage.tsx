/**
 * 「百宝箱 · 内建帮助」—— 一级页面的骨架（§4.6.6）
 * ============================================================================
 *
 * ## 🔴 这一页是六件工具的入口，而**其中一件今天真的能用**
 *
 * 规格 §4.6.6 把百宝箱写成"一组工具"，而"内建帮助"这一页要做的是
 * 把它们摆出来、说清每件解决什么问题。
 *
 * ⚠️ 角标是这一页的诚实处：`日志与诊断` 是**今天就能用**的
 *（`web/src/routes/LogsPage.tsx` 是 M4 的真实落点），其余五件各自
 * 等在它后面的那一段内核工作。一个不给角标的实现会让用户点进
 * 五个空页面之后认定"整个百宝箱是坏的"。
 */

import { Link } from "@tanstack/react-router";
import type { ReactElement } from "react";
import "./pages.css";

const TOOLS = [
  {
    to: "/toolbox",
    label: "内建帮助",
    note: "你正在看的这一页：六件工具各自解决什么问题",
    badge: "本页",
  },
  {
    to: "/toolbox/logs",
    label: "日志与诊断",
    note: "内核与安装过程的原始日志，可按级别过滤、可导出",
    badge: "今天可用",
  },
  {
    to: "/toolbox/speedtest",
    label: "测速",
    note: "官方源与各镜像的实测吞吐 —— 结果只报实测值，不报估算",
    badge: "等 M8",
  },
  {
    to: "/toolbox/components",
    label: "依赖与组件检查",
    note: "运行时 · 加载器 · 依赖是否齐全，缺哪一件、装到哪一步",
    badge: "等 M6",
  },
  {
    to: "/toolbox/cleanup",
    label: "磁盘清理",
    note: "缓存 / 旧版本 / 孤儿目录各占多少，删之前逐项列出要删什么",
    badge: "等 M8",
  },
  {
    to: "/toolbox/schematic",
    label: "投影材质查看",
    note: ".litematic / .nbt 结构预览与材料清单",
    badge: "等 M9",
  },
] as const;

export function ToolboxPage(): ReactElement {
  return (
    <section>
      <h1 className="page__title">百宝箱 · 内建帮助</h1>
      <p className="page__note">
        这一栏是"出问题时去哪里"：六件工具各自回答一个具体问题，
        而不是六个功能入口的堆叠。
      </p>
      <ul className="page__list">
        {TOOLS.map((tool) => (
          <li key={tool.to}>
            <Link className="page__card" to={tool.to}>
              <span className="page__cardTitle">
                {tool.label}
                <span className="page__cardBadge">{tool.badge}</span>
              </span>
              <span className="page__cardNote">{tool.note}</span>
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
