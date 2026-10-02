/**
 * 「设置 · 外观与材质」—— 一级页面的骨架（§4.6.2）
 * ============================================================================
 *
 * ## 🔴 这一页里有一个**真的能用**的控件
 *
 * 「侧边栏宽度」的两段按钮直接读写 `web/src/shell/railPref.tsx` 那一份状态，
 * 而外壳的 `data-rail` 由它驱动 —— **点一下就换栏宽**，不是摆设。
 *
 * ⚠️ 而它也是这一页里**唯一**能改的东西，理由在下面那张表里写清了：
 * 主题 / 材质 / 强度（`web/src/appearance/appearance.ts`）今天**只有解析**，
 * 没有设置存储 —— 于是它们**跟随系统**，而这不是"默认值"，是"唯一的值"。
 * 一个把三段开关画出来的实现会让用户点它，然后什么都不发生。
 *
 * ## ⚠️ 而"记住用户选择"这件事还没做
 *
 * §4.6.2 要求宽窄**被记住**并随配置备份迁移。今天它只活在这一次会话里
 *（`railPref.tsx` 的文件头写着为什么不用 `localStorage` 顶上）。
 * 所以这一页明说这件事，而不是让用户以为"设了就该被记住"。
 */

import type { ReactElement } from "react";
import { RAIL_WIDTHS, RAIL_WIDTH_LABELS, useRailPref } from "../shell/railPref.tsx";
import { APP_VERSION } from "../app/version.ts";
import "./pages.css";

export function SettingsPage(): ReactElement {
  const { railWidth, setRailWidth } = useRailPref();
  return (
    <section>
      <h1 className="page__title">设置 · 外观与材质</h1>
      <p className="page__note">
        外观这一页管的是"看得见的东西"：栏宽、主题、材质档位、视觉强度。
        运行时 / 网络 / 存储 / 高级在二级栏的另外四页里。
      </p>

      <h2 className="page__subtitle">侧边栏</h2>
      <div className="page__segmented" role="group" aria-label="侧边栏宽度">
        {RAIL_WIDTHS.map((width) => (
          <button
            key={width}
            type="button"
            className={width === railWidth ? "page__segment page__segment--on" : "page__segment"}
            aria-pressed={width === railWidth}
            onClick={() => {
              setRailWidth(width);
            }}
          >
            {RAIL_WIDTH_LABELS[width]}
          </button>
        ))}
      </div>
      <dl className="page__facts">
        <dt className="page__factTerm">窄栏</dt>
        <dd className="page__factValue">56 px —— 只留图标，名字在悬停与键盘聚焦时浮出来</dd>
        <dt className="page__factTerm">宽栏</dt>
        <dd className="page__factValue">200 px —— 图标 + 名字 + 角标</dd>
        <dt className="page__factTerm">是否记住</dt>
        <dd className="page__factValue">
          还没有：这一份选择只活在本次运行里，重启回到窄栏（要等设置存储接上）
        </dd>
      </dl>

      <h2 className="page__subtitle">主题与材质</h2>
      <dl className="page__facts">
        <dt className="page__factTerm">主题</dt>
        <dd className="page__factValue">
          跟随系统 —— 解析规则已经在（深 / 浅 / 跟随 / 按实例四态），而"改它"要等设置存储
        </dd>
        <dt className="page__factTerm">材质</dt>
        <dd className="page__factValue">
          按系统能力自动收紧（高对比 · 减少动效 · 节电 · 远程会话都会让材质退档）
        </dd>
        <dt className="page__factTerm">视觉强度</dt>
        <dd className="page__factValue">标准 —— 收紧的判据是系统给的，不是我们猜的</dd>
        <dt className="page__factTerm">版本</dt>
        <dd className="page__factValue">{`v${APP_VERSION}`}</dd>
      </dl>
    </section>
  );
}
