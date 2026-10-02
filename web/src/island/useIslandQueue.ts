/**
 * 灵动岛的**队列接线**：把 `Channel` 的流接进 `Island` 组件
 * ============================================================================
 *
 * ## 🔴 它是 M4 那句"全流程一次点通"的**最后一环**
 *
 * ```text
 *   Rust  ChannelSink ──send──▶ IPC ──onmessage──▶ parseIslandState（校验）
 *        ──▶ toIslandContent（翻译）──▶ 本 hook 的状态 ──▶ <Island …>
 * ```
 *
 * ## ⚠️ 而它守的第一条纪律是"**不编**"
 *
 * | 情形 | 它做什么 |
 * |---|---|
 * | 还没开始 | `Idle`，而**分**数由调用方给（那要账户与源） |
 * | 载荷翻译不过来 | **保留上一个状态** + `console.error` |
 *
 * 第二条值得说：`bridge.ts` 里"认不出的 `kind` **抛**"是有意的 ——
 * 而**抛到哪儿**是这里决定的。
 *
 * 一个 `catch { setState(idle) }` 的实现会让"内核加了状态而前端没跟上"
 * 表现为**岛突然空闲** —— 而那与"没事发生"一模一样，
 * 于是那个 bug 会**活很久**。
 *
 * 所以这里：**保留上一个状态**（界面不闪），而把错误**喊出来**
 *（`console.error` 是仓库 lint 白名单允许的那个）。
 *
 * ## ⚠️ 而它**不是**一个"自动开始"的 hook
 *
 * `start()` 必须由用户动作触发（点"启动"）。一个 `useEffect` 里自动跑的
 * 实现会在**打开界面时就开始下载** —— 而那是一个**几十 GB 的操作**。
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { cancelInstall, startInstall, type InstallSummary } from "../api/index.ts";
import { toIslandContent } from "./bridge.ts";
import type { IslandContent } from "./Island.tsx";

/** 一次安装的运行态。 */
export interface InstallRun {
  readonly busy: boolean;
  /** 最近的失败（**给人看的那一句**）。 */
  readonly error: string | null;
  /** 上一次成功的结局。 */
  readonly last: InstallSummary | null;
}

export interface IslandQueue {
  readonly content: IslandContent;
  readonly run: InstallRun;
  /** 开始一次安装（**必须由用户动作触发**）。 */
  readonly start: (versionId: string) => Promise<void>;
  /** 请后端停下来。 */
  readonly cancel: () => Promise<void>;
}

/**
 * 一个**不编**的空闲内容。
 *
 * ⚠️ 它是 `Idle` 的**兜底**，而 `Island` 有自己的 `idleContent(account, source)`
 *（那一个带账户与源，是两个真实信息）。
 * 所以这里的文案刻意**不假装知道**那些东西。
 */
const IDLE: IslandContent = {
  kind: "idle",
  headline: "就绪",
  fraction: null,
  hints: [],
};

export function useIslandQueue(): IslandQueue {
  const [content, setContent] = useState<IslandContent>(IDLE);
  const [run, setRun] = useState<InstallRun>({
    busy: false,
    error: null,
    last: null,
  });

  // ⚠️ **一个"还活着吗"的标记。**
  //
  // 安装可能在组件卸载之后才推来最后几条进度（用户切走了页），
  // 而那时 `setContent` 会作用在一个**已经卸载**的组件上 ——
  // React 18 不再为此报警告，但那仍然是一次**没有意义的写入**。
  //
  // 一个 `AbortController` 在这里**做不到**这件事：取消是要**问后端**的
  //（见 `cancel()`），而那与"我不想再收消息了"是两回事。
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const start = useCallback(async (versionId: string): Promise<void> => {
    setRun({ busy: true, error: null, last: null });
    try {
      const summary = await startInstall(versionId, (state) => {
        if (!alive.current) return;
        try {
          setContent(toIslandContent(state));
        } catch (e) {
          // 🔴 **保留上一个状态，而把错误喊出来** —— 见模块文档。
          //
          // ⚠️ 而**不**在这里把界面变回空闲：那会让"内核加了一个状态
          // 而前端没跟上"看起来像"没事发生"，于是那个 bug 会活很久。
          console.error("[qinmo] 这条灵动岛载荷翻译不过来，界面停在上一态：", e);
        }
      });
      if (alive.current) {
        setRun({ busy: false, error: null, last: summary });
      }
    } catch (e) {
      const human = e instanceof Error ? e.message : String(e);
      // ⚠️ 而**这里不写 `setContent`** —— 失败那一态**由后端经 channel 报过**
      //（见 `src-tauri` 的 `install`：它先 `send(Error{..})` 再 `Err`）。
      //
      // 一个"这里再设一次错误态"的实现会有**两个**真相来源，
      // 而它们会在"失败发生在前端（例如没有后端）"时不一致 ——
      // 那种情况下后端**什么都没发**，于是这里的 `error` 是唯一的信息。
      if (alive.current) {
        setRun({ busy: false, error: human, last: null });
      }
    }
  }, []);

  const cancel = useCallback(async (): Promise<void> => {
    await cancelInstall();
  }, []);

  return { content, run, start, cancel };
}
