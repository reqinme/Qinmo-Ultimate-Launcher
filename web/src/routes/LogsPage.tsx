import type { ReactElement } from "react";
import { LogDrawer, LogLevel, LogSource, type LogLine } from "../logs/LogDrawer.tsx";

/**
 * 日志与诊断页（**百宝箱的第一件** · §5.5）
 * ============================================================================
 *
 * ## ⚠️ 它现在挂的是**夹具数据**，而那是刻意的
 *
 * 真实的日志要等**内核的日志管道**接上来（`qul-infra/src/logging.rs` 已经
 * 有脱敏那一段，而"把行推到前端"是 M4/M4.5 之间的接线）。
 *
 * 而**现在就把 `LogDrawer` 挂在这里**有三个理由：
 *
 * 1. 它是 §5.5 的落点，而"没接线的组件"**不进产物** —— 一个不被 import 的
 *    组件不会被打包，于是 `impeccable` 也测不到它的 CSS。**这一轮就是这样
 *    发现的**（CSS 大小没变）。
 * 2. 那四个操作（含 `Kill Minecraft` 的二次确认）与**分享状态机**现在
 *    可以被眼睛验一遍。
 * 3. 夹具里**刻意混着两个来源与五档级别** —— 于是"按来源过滤"这件事
 *    在界面上是可验的，而不是只能读代码。
 */
export function LogsPage(): ReactElement {
  return (
    <section>
      <h1 className="page__title">日志与诊断</h1>
      <p className="page__note">
        按来源过滤是**必须的**（§5.5 原文：「否则诊断时两种日志混在一起无法阅读」）。
        而诊断包的分享是「本地生成 → 你预览/编辑 → 你确认后上传」，
        <strong>默认不上传</strong>。
      </p>
      <p className="page__note" data-fixture="true">
        ⚠️ 下面这些行是<strong>夹具</strong> —— 真实日志要等内核的日志管道接上来。
        而四个操作与分享状态机<strong>现在就是真的</strong>。
      </p>
      <LogDrawer
        lines={FIXTURE}
        onCopy={(t) => {
          // ⚠️ **`navigator.clipboard` 可能不存在**（非安全上下文 / 老 WebView），
          // 而一个不 try 的实现会让"点复制"抛异常并炸掉整个界面。
          try {
            void globalThis.navigator?.clipboard?.writeText(t);
          } catch {
            /* 复制失败不该影响任何事 */
          }
        }}
        onGenerateShare={() => FIXTURE.map((l) => l.text).join("\n")}
      />
    </section>
  );
}

/** 夹具：**两个来源 × 五档级别** 都有，于是过滤与配色都能被眼睛验到。 */
const FIXTURE: readonly LogLine[] = [
  { source: LogSource.Console, level: LogLevel.Info, text: "启动器：开始解析版本详情", atMs: 1 },
  { source: LogSource.Console, level: LogLevel.Debug, text: "启动器：元数据 sha1 已核对", atMs: 2 },
  { source: LogSource.Game, level: LogLevel.Info, text: "[Render thread/INFO]: Setting user: qinme", atMs: 3 },
  { source: LogSource.Game, level: LogLevel.Warn, text: "[Render thread/WARN]: 找不到资源包 foo", atMs: 4 },
  { source: LogSource.Console, level: LogLevel.Plain, text: "natives 已解压到实例内", atMs: 5 },
  { source: LogSource.Game, level: LogLevel.Error, text: "[main/ERROR]: java.lang.UnsatisfiedLinkError", atMs: 6 },
];
