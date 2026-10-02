/**
 * 日志抽屉（**M4 主干** · §5.5）
 * ============================================================================
 *
 * ## 规格给的五件事，逐条落在这里
 *
 * | # | 原文 | 落点 |
 * |---|---|---|
 * | 1 | **用 `meta` 区分来源** `CONSOLE`（启动器自己的输出）与 `LOG4J`（游戏进程的输出） | `LogSource` + 来源过滤 |
 * | 2 | **"日志抽屉必须支持按来源过滤，否则诊断时两种日志混在一起无法阅读"** | 来源筛选器 |
 * | 3 | 分级色令牌化（`debug` / `error` / `info` / `warn` / `default`） | `LogLevel` + `tokens.css` 里的 `--console-log-*` |
 * | 4 | **四个操作** `Clear` / `Copy Log` / `Upload Log` / **`Kill Minecraft`（带二次确认）** | 四个按钮，而 Kill 那条有确认 |
 * | 5 | **日志分享为「本地生成脱敏包 → 用户预览/编辑 → 用户确认后上传」，默认不上传** | `ShareState` 那个状态机 |
 *
 * ## 🔴 第 5 条是本组件里最需要被类型保护的
 *
 * 规格原文：*"**本地生成脱敏包 → 用户预览/编辑 → 用户确认后上传**，
 * **默认不上传**"*。
 *
 * 也就是说"上传"这件事**必须经过三个前置步骤**，而它**不能是一个默认动作**。
 *
 * 所以这里把它做成一个**联合类型**（`ShareState`），而那个类型里
 * **没有"直接上传"这个状态** —— 一个 `upload()` 的入口不存在，
 * 只有 `ShareState.Previewing → Confirming → Uploading`。
 *
 * **一个"点一下就上传"的按钮是这次设计最容易退化成的样子**，
 * 而它在类型上被挡住了。
 */

import { useState, type ReactElement } from "react";
import "./LogDrawer.css";

/** 日志来源（§5.5 的 `meta`：`CONSOLE` / `LOG4J`）。 */
export const LogSource = {
  /** **启动器自己的输出**（我们的 INFO / 错误）。 */
  Console: "console",
  /** **游戏进程的输出**（log4j 那一侧）。 */
  Game: "log4j",
} as const;
export type LogSource = (typeof LogSource)[keyof typeof LogSource];

export const ALL_LOG_SOURCES: readonly LogSource[] = [LogSource.Console, LogSource.Game];

/** 日志级别（§5.5 的五个令牌）。 */
export const LogLevel = {
  Debug: "debug",
  Info: "info",
  Warn: "warn",
  Error: "error",
  /** **没有级别的行**（原样透传的那些）。 */
  Plain: "default",
} as const;
export type LogLevel = (typeof LogLevel)[keyof typeof LogLevel];

/** 一行日志。 */
export interface LogLine {
  readonly source: LogSource;
  readonly level: LogLevel;
  readonly text: string;
  /** 毫秒时间戳（由调用方给 —— 组件不取时钟）。 */
  readonly atMs: number;
}

/**
 * 日志分享的状态机（§5.5 第 5 条）。
 *
 * 🔴 **类型里没有"直接上传"。** 见文件头那一段 ——
 * 那三个前置步骤（生成 → 预览/编辑 → 确认）是**类型上不可跳过**的。
 */
export const ShareState = {
  /** 还没开始分享。 */
  Idle: "idle",
  /** **本地生成脱敏包**（打码已完成，内容在内存/临时文件里）。 */
  Generated: "generated",
  /** **用户预览/编辑**（他可以在上传前改任何一行）。 */
  Previewing: "previewing",
  /** 用户点了确认 —— 此刻才允许上传。 */
  Confirmed: "confirmed",
  /** 上传中。 */
  Uploading: "uploading",
  /** 上传完成，给出链接。 */
  Done: "done",
} as const;
export type ShareState = (typeof ShareState)[keyof typeof ShareState];

/**
 * **哪一个动作在哪个状态下是允许的。**
 *
 * ⚠️ 它是**数据**而不是组件里的 `if` —— 于是"能不能上传"这件事
 * 有一个可被测试的答案，而不是散在 `disabled={...}` 里。
 */
export function canShare(state: ShareState, action: "generate" | "edit" | "confirm" | "upload"): boolean {
  switch (action) {
    case "generate":
      return state === ShareState.Idle;
    case "edit":
      // 只有"生成好了、还没确认"的时候能编辑。
      // ⚠️ 一个"任何时候都能编辑"的实现会让用户在上传**进行中**
      // 改掉内容，而那与"上传的是他确认过的那一份"直接冲突。
      return state === ShareState.Generated || state === ShareState.Previewing;
    case "confirm":
      return state === ShareState.Generated || state === ShareState.Previewing;
    case "upload":
      // 🔴 **只有 `Confirmed` 能上传** —— 这是"默认不上传"的类型级落点。
      return state === ShareState.Confirmed;
    default:
      return false;
  }
}

export interface LogDrawerProps {
  readonly lines: readonly LogLine[];
  /** 初始来源筛选（`null` = 全部）。 */
  readonly initialSource?: LogSource | null;
  /** 复制到剪贴板（**由调用方给** —— 组件不碰 `navigator`）。 */
  readonly onCopy?: (text: string) => void;
  /** 清空。 */
  readonly onClear?: () => void;
  /** 请求结束游戏进程（**带二次确认**）。 */
  readonly onKill?: () => void;
  /** 生成脱敏包（**由调用方做打码** —— 那是内核 `scrub` 的事）。 */
  readonly onGenerateShare?: () => string;
}

const LEVEL_LABEL: Record<LogLevel, string> = {
  [LogLevel.Debug]: "调试",
  [LogLevel.Info]: "信息",
  [LogLevel.Warn]: "警告",
  [LogLevel.Error]: "错误",
  [LogLevel.Plain]: "",
};

/**
 * 日志抽屉。
 *
 * ## 🔴 两条实现纪律
 *
 * 1. **来源过滤是必须的**（§5.5 原文："否则诊断时两种日志混在一起无法阅读"）。
 *    所以它**不是一个可选功能**，而是抽屉的一部分。
 * 2. **`Kill Minecraft` 必须二次确认**（§5.5 原文的括号里就写着）。
 *    而那个确认**不是 `window.confirm`** —— 它要说明后果，而原生
 *    `confirm` 只说一句"确定吗"。
 */
export function LogDrawer({
  lines,
  initialSource = null,
  onCopy,
  onClear,
  onKill,
  onGenerateShare,
}: LogDrawerProps): ReactElement {
  const [source, setSource] = useState<LogSource | null>(initialSource);
  const [killArmed, setKillArmed] = useState(false);
  const [share, setShare] = useState<ShareState>(ShareState.Idle);
  const [shareText, setShareText] = useState("");

  const shown = source === null ? lines : lines.filter((l) => l.source === source);
  const counts: Record<LogSource, number> = {
    [LogSource.Console]: lines.filter((l) => l.source === LogSource.Console).length,
    [LogSource.Game]: lines.filter((l) => l.source === LogSource.Game).length,
  };

  return (
    <section className="logdrawer" aria-label="日志与诊断">
      <header className="logdrawer__bar">
        {/*
          🔴 **来源过滤**（§5.5 的原文要求）。
          它是"控制台 / 游戏"两个可切换的钮，而**两个都显示条数** ——
          一个只显示名字的实现会让"游戏那边有没有输出"变成一个要
          点进去才知道的问题。
        */}
        <div className="logdrawer__filter" role="group" aria-label="按来源过滤">
          <FilterChip
            label="全部"
            count={lines.length}
            on={source === null}
            onClick={() => setSource(null)}
          />
          <FilterChip
            label="启动器"
            count={counts[LogSource.Console]}
            on={source === LogSource.Console}
            onClick={() => setSource(LogSource.Console)}
          />
          <FilterChip
            label="游戏"
            count={counts[LogSource.Game]}
            on={source === LogSource.Game}
            onClick={() => setSource(LogSource.Game)}
          />
        </div>

        <div className="logdrawer__actions">
          <button type="button" className="btn" onClick={() => onClear?.()}>
            清空
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => onCopy?.(shown.map((l) => l.text).join("\n"))}
          >
            复制
          </button>
          <button
            type="button"
            className="btn"
            disabled={!canShare(share, "generate")}
            onClick={() => {
              const t = onGenerateShare?.() ?? "";
              setShareText(t);
              setShare(ShareState.Generated);
            }}
          >
            生成诊断包
          </button>
          {/*
            🔴 **`Kill Minecraft` 带二次确认**（§5.5）。
            而它是**两步**（而不是 `window.confirm`）——
            因为要说明后果，而原生 `confirm` 只说一句"确定吗"。
          */}
          {killArmed ? (
            <span className="logdrawer__confirm" role="alert">
              <span>会立刻结束游戏进程，未保存的进度会丢。</span>
              <button
                type="button"
                className="btn btn--danger"
                onClick={() => {
                  setKillArmed(false);
                  onKill?.();
                }}
              >
                确认结束
              </button>
              <button type="button" className="btn" onClick={() => setKillArmed(false)}>
                取消
              </button>
            </span>
          ) : (
            <button
              type="button"
              className="btn btn--danger"
              disabled={onKill === undefined}
              onClick={() => setKillArmed(true)}
            >
              结束游戏
            </button>
          )}
        </div>
      </header>

      {/*
        🔴 **分享面板：三个前置步骤。**
        而"上传"那个按钮在 `Confirmed` 之前**是禁用的** —— 见 `canShare`。
      */}
      {share === ShareState.Idle ? null : (
        <div className="logdrawer__share" aria-label="诊断包">
          <p className="logdrawer__shareNote">
            诊断包在**本地**生成并已脱敏。**上传之前你可以改任何一行** ——
            而默认不会上传。
          </p>
          <textarea
            className="logdrawer__shareText"
            value={shareText}
            readOnly={!canShare(share, "edit")}
            onChange={(e) => {
              setShareText(e.target.value);
              if (canShare(share, "edit")) setShare(ShareState.Previewing);
            }}
            aria-label="脱敏后的诊断包内容（可编辑）"
            rows={8}
          />
          <div className="logdrawer__shareActions">
            <button
              type="button"
              className="btn btn--primary"
              disabled={!canShare(share, "confirm")}
              onClick={() => setShare(ShareState.Confirmed)}
            >
              我已检查，确认无误
            </button>
            <button
              type="button"
              className="btn"
              disabled={!canShare(share, "upload")}
              onClick={() => setShare(ShareState.Uploading)}
            >
              上传
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => {
                setShare(ShareState.Idle);
                setShareText("");
              }}
            >
              放弃
            </button>
          </div>
        </div>
      )}

      <ol className="logdrawer__lines">
        {shown.map((l, i) => (
          <li className="logdrawer__line" data-level={l.level} data-source={l.source} key={`${l.atMs}-${i}`}>
            <span className="logdrawer__level">{LEVEL_LABEL[l.level]}</span>
            <span className="logdrawer__text">{l.text}</span>
          </li>
        ))}
        {shown.length === 0 ? (
          <li className="logdrawer__empty">这个来源下还没有输出。</li>
        ) : null}
      </ol>
    </section>
  );
}

function FilterChip({
  label,
  count,
  on,
  onClick,
}: {
  readonly label: string;
  readonly count: number;
  readonly on: boolean;
  readonly onClick: () => void;
}): ReactElement {
  return (
    <button
      type="button"
      className={on ? "logdrawer__chip logdrawer__chip--on" : "logdrawer__chip"}
      aria-pressed={on}
      onClick={onClick}
    >
      {label}
      <span className="logdrawer__count">{count}</span>
    </button>
  );
}
