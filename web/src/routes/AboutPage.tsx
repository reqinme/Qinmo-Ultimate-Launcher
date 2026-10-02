/**
 * 「关于」—— 一级页面的骨架（§4.6.9）
 * ============================================================================
 *
 * ## 🔴 这一页的版本号**不是写在这里的**
 *
 * 它读 `web/src/app/version.ts` 的 `APP_VERSION`，而那个常量必须与
 * `package.json` 的 `"version"` 与 `src-tauri/tauri.conf.json` 的
 * `"version"` **逐字相同** —— `tools/check-shell-contract.ps1` 钉着这三处。
 *
 * ⚠️ 一个在这里手写 `v0.1.0` 的实现（规格稿的 ASCII 里就是那么写的）
 * 会让"关于页说的版本"、"安装包名字里的版本"、"内核报的版本"三处漂移，
 * 而那种漂移**只在用户报故障时才被发现**。
 *
 * ## ⚠️ 而许可证与来源这两行**必须留在界面上**
 *
 * §1.4 的图形纪律（"一点官方素材都不用"）是一个**对外承诺**，
 * 而承诺要写在用户看得到的地方 —— 不是只写在仓库的文档里。
 */

import type { ReactElement } from "react";
import { APP_VERSION } from "../app/version.ts";
import "./pages.css";

export function AboutPage(): ReactElement {
  return (
    <section>
      <h1 className="page__title">关于</h1>
      <p className="page__note">
        秦墨是一个自研的 Minecraft 启动器：界面、图形、内核全部自写，
        不打包任何官方素材。
      </p>
      <dl className="page__facts">
        <dt className="page__factTerm">版本</dt>
        <dd className="page__factValue">{`v${APP_VERSION}`}</dd>
        <dt className="page__factTerm">界面</dt>
        <dd className="page__factValue">React 19 + TypeScript + Vite —— web/ 目录</dd>
        <dt className="page__factTerm">内核</dt>
        <dd className="page__factValue">Rust —— crates/ 里的 qul-core 与它下面几层</dd>
        <dt className="page__factTerm">外壳</dt>
        <dd className="page__factValue">Tauri 2 —— 自绘标题栏，窗口无系统边框</dd>
        <dt className="page__factTerm">图形与文案</dt>
        <dd className="page__factValue">
          全部自绘/自写：不用官方素材、不用官方图标、不抄官方文案
        </dd>
        <dt className="page__factTerm">这一版到哪了</dt>
        <dd className="page__factValue">
          界面主干闭合（M4）：导航、按键契约、主页、日志页是真的；其余页面是骨架
        </dd>
      </dl>
    </section>
  );
}
