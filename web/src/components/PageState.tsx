/**
 * 页面级三态容器（`docs/UI设计规格.md` §7.6）
 * ============================================================================
 *
 * ## 它解决的是"每个页面各写各的"
 *
 * §7.6 的问题陈述原文：
 *
 * > **此前的问题**：百宝箱空状态写得很细，但**主页 / 实例 / 下载 / 存档 /
 * > 账户 / 设置都没有三态规范** → 每个页面会各写各的，最终风格不统一。
 *
 * 所以 §7.6 规则 1 是硬规矩：
 *
 * > **三态共用同一个容器组件**（`PageState`），页面只传参数——**不许各页面自己搭**
 *
 * ## 🔴 这里没有 `loading: boolean`
 *
 * §7.6 的三个态是**互斥**的：一个容器不可能同时"在加载"与"加载失败"。
 * 用三个布尔（`loading` / `empty` / `error`）就允许 `loading && error` 这种
 * **画不出来**的组合，而它只能靠调用方的自觉去避免。
 *
 * 判别联合 `kind` 让那个组合**在类型上不存在** —— 与 `index.tsx` 里
 * `VisualState` 取代 `disabled + loading` 是同一个手法，理由也同一句：
 * *"两个布尔允许'同时禁用且加载'这种非法组合。"*
 *
 * 同理，`empty` 少了主文案 / 说明 / 行动按钮中的任何一个都是**编译错误**，
 * 所以这里**一句兜底文案都没有**（兜底会让"漏传字段"变成运行时才发现的事）。
 *
 * ## 骨架屏最短显示 300 ms —— 机制在这里，不在调用方
 *
 * §7.6 规则 6：*"骨架屏不得闪烁：最短显示 300 ms（避免'闪一下就没'）"*。
 *
 * 这句话必须由**容器**落实：真正知道"数据到了"的是页面，而页面一旦把
 * `kind` 从 `loading` 改掉，骨架屏就已经不在树上了 —— 调用方**没有机会**
 * 再补上那几百毫秒。所以这里的做法是：
 *
 * 1. 加载态**结束**的那一刻开一个 `setTimeout`（剩余时间 = 300 − 已显示）；
 * 2. 计时器没到之前，容器**仍然渲染骨架屏**，只是把收到的那个态挂起来；
 * 3. 计时器一到，**立即**交出真正的 `empty` / `error`。
 *
 * 三条刻意的取舍：
 *
 * - **延迟的是"换内容"，不是"推迟加载"** —— 屏幕上从头到尾都有一只骨架屏
 *   在，用户看到的是"稳"，而不是"卡"。
 * - **只有 `empty` 会被留**（见下面的 `heldKinds`）：加载失败是
 *   **值得打断**的事，把它压在骨架屏后面 300 ms 只会让人以为"卡住了"，
 *   所以 `error` 一拿到就立刻上屏。
 * - **新挂载就传 `empty` / `error` 的容器不走这条路** —— 那时候
 *   **从来没有过骨架屏**，没有东西需要"显示满 300 ms"（判断依据是
 *   `startedAtRef` 仍是 `null`）。
 *
 * ⚠️ 反过来说：这个最短时长**不是**"数据到了还要再等 300 ms"，
 * 它只对**刚刚还在加载**的那一次切换生效。
 *
 * 计时器在 `prefers-reduced-motion: reduce` 下**不再等待**：那个偏好
 * 属于"少给一点过渡"，而这里留住的正是过渡本身（§8.3 保留骨架屏是
 * 为了"加载指示"这件事，不是为了留住一段动画）。
 *
 * ## 局部失败不整页报错（规则 5）
 *
 * 三态都渲染成一个**具名的 `region`**（`aria-label` = `title`），
 * 而 `title` 还是空态 / 错误态的**可见小标题**。于是把一个 `PageState`
 * 放进某个区块里，读屏用户听到的是"某某区块：加载失败"，
 * 而不是一整个页面被错误态顶掉。
 */

import { useEffect, useRef, useState, type ReactElement, type ReactNode } from "react";
import "./PageState.css";

/* ==========================================================================
 * 常量
 * ======================================================================== */

/**
 * 骨架屏的最短显示时长（§7.6 规则 6 的字面值：300 ms）。
 *
 * ⚠️ **它是毫秒数，不是令牌** —— 令牌层管的是"观感刻度"
 *（`--duration-*`），而这个是**一条交互契约**（"不许闪"）。
 * 把它写成 `var(--duration-slow)` 会让"改一个观感令牌"顺手改掉契约。
 */
export const MIN_SKELETON_MS = 300;

/** 骨架屏在没给 `lines` 时画几行（贴近"列表 / 卡片"的常见形状）。 */
const DEFAULT_SKELETON_LINES = 4;

/* ==========================================================================
 * 属性：判别联合（非法组合不存在）
 * ======================================================================== */

/**
 * `PageState` 的属性。
 *
 * 三个分支对应 §7.6 的三个态，**字段就是那张"必须给的三样"表**：
 *
 * | 分支 | §7.6 要求 |
 * |---|---|
 * | `loading` | ① 进度感 ② 不遮挡已有的可操作区 —— 所以这里**只有一个容器**，调用方把它放进"数据还没到的那块区域"即可 |
 * | `empty` | ① 这是干什么的 ② 推荐从哪开始 ③ 怎么看到全部 —— `title` / `body` / `action` 三样**缺一即编译错误** |
 * | `error` | ① 为什么失败 ② 怎么办 ③ 重试 / 导出诊断 —— `code` + `reason` + `advice` + `actions` 四件套 |
 */
export type PageStateProps =
  | {
      readonly kind: "loading";
      /** 这块区域是什么（同时是 `region` 的可访问名）。 */
      readonly title: string;
      /** 骨架行数；默认 {@link DEFAULT_SKELETON_LINES}。 */
      readonly lines?: number;
      /** 额外的"在忙什么"（例如"正在读取存档列表"）；不给就只用 `title`。 */
      readonly busyLabel?: string;
    }
  | {
      readonly kind: "empty";
      /** 主文案：**这是干什么的**（§4.3.1）。 */
      readonly title: string;
      /** 说明：**推荐从哪开始**（§4.3.1）。 */
      readonly body: string;
      /** 主行动按钮：**怎么看到全部**（§4.3.1）。 */
      readonly action: ReactNode;
      /** 自绘图形的替换项；不给就用内置的空箱线稿。 */
      readonly icon?: ReactNode;
    }
  | {
      readonly kind: "error";
      /** 这块区域是什么（同时是 `region` 的可访问名）。 */
      readonly title: string;
      /** 错误码：**它是给人报故障用的**，不是给人读的句子。 */
      readonly code: string;
      /** 人话原因：**为什么失败**。 */
      readonly reason: string;
      /** 建议：**怎么办**。 */
      readonly advice: string;
      /** 可点击操作：重试 / 导出诊断。 */
      readonly actions: ReactNode;
    };

/* ==========================================================================
 * 组件
 * ======================================================================== */

/**
 * 页面级三态容器。
 *
 * ⚠️ **调用方只传参数，不要自己搭骨架屏 / 空态 / 错误态**（§7.6 规则 1）。
 * 一个页面里可以放多个 `PageState` —— 每个管自己那一块，
 * 这就是规则 5 说的"局部失败不整页报错"。
 */
export function PageState(props: PageStateProps): ReactElement {
  const skeleton = useSkeletonHold(props.kind);

  if (skeleton && props.kind !== "loading") {
    // 真正的态已经拿到，只是还没到 300 ms —— 先继续画骨架屏。
    // 行数用默认值：这个分支下 `props` 已经**不是** `loading`，
    // 所以没有 `lines` 可读，而它只是过渡的一帧。
    return <LoadingState title={props.title} lines={DEFAULT_SKELETON_LINES} />;
  }

  switch (props.kind) {
    case "loading":
      return renderLoading(props);
    case "empty":
      return renderEmpty(props);
    case "error":
      return renderError(props);
  }
}

/* ==========================================================================
 * 三个态各自的渲染 —— 分开是为了让"某个态该怎么读"只在一个地方回答
 * ======================================================================== */

/**
 * 加载态。
 *
 * ⚠️ **没有 `role="progressbar"`、没有转圈元素** —— §7.6 规则 2：
 * *"Loading 用骨架屏而非转圈——骨架能提前暗示布局，转圈只表示'在忙'"*。
 * 于是这里连 `aria-live` 也不用（§7.5 G9：进度条不要用 `aria-live`），
 * 只给容器一个 `aria-busy="true"`。
 */
function renderLoading(props: Extract<PageStateProps, { kind: "loading" }>): ReactElement {
  return (
    <LoadingState
      title={props.title}
      lines={props.lines ?? DEFAULT_SKELETON_LINES}
      {...(props.busyLabel === undefined ? {} : { busyLabel: props.busyLabel })}
    />
  );
}

function LoadingState({
  title,
  lines,
  busyLabel,
}: {
  readonly title: string;
  readonly lines: number;
  readonly busyLabel?: string;
}): ReactElement {
  // 行数在这里夹住：`lines={0}` 会画出一只空盒子，"在加载"这件事就没了。
  const count = Math.max(1, Math.trunc(lines));
  const rows = Array.from({ length: count }, (_, index) => index);

  return (
    <div className={cls("pageState", "pageState--loading")} role="region" aria-label={title} aria-busy="true">
      <div className="pageState__skeleton" aria-hidden="true">
        <div className="pageState__lines">
          {rows.map((row) => (
            // 最后一行短一截 —— 这是"这是一段正文"的形状提示，不是装饰
            <span
              key={row}
              className={cls("pageState__skeletonLine", row === count - 1 && "pageState__skeletonLine--short")}
            />
          ))}
        </div>
      </div>
      {busyLabel === undefined ? null : <p className="pageState__body">{busyLabel}</p>}
    </div>
  );
}

/**
 * 空态。
 *
 * §4.3.1 的纪律：**不得出现"暂无数据"这类措辞** —— 那是"没做设计"的表现。
 * 这条纪律在类型上就已经被逼住了：`title` / `body` / `action` 全是必填，
 * 于是**没有一个字段可以被"暂无数据"这种占位话糊过去**。
 *
 * DOM 顺序 = 视觉顺序 = 键盘顺序 = 读屏顺序：图形 → 主文案 → 说明 → 操作。
 * 四者一致是刻意的 —— 一个"视觉上按钮在最上面、Tab 却要绕过文案才到"的
 * 引导卡，会让键盘用户以为自己漏掉了什么。
 */
function renderEmpty(props: Extract<PageStateProps, { kind: "empty" }>): ReactElement {
  return (
    <div className={cls("pageState", "pageState--empty")} role="region" aria-label={props.title}>
      {props.icon === undefined ? <EmptyBoxGlyph /> : <span className="pageState__icon">{props.icon}</span>}
      <h2 className="pageState__title">{props.title}</h2>
      <p className="pageState__body">{props.body}</p>
      <div className="pageState__action">{props.action}</div>
    </div>
  );
}

/**
 * 错误态。
 *
 * §7.6 规则 4：**"错误码 + 原因 + 建议 + 操作"四件套**。
 * `role="alert"` 是这里唯一的状态播报 —— 失败值得打断，加载不值得。
 */
function renderError(props: Extract<PageStateProps, { kind: "error" }>): ReactElement {
  return (
    <div className={cls("pageState", "pageState--error")} role="alert">
      <ErrorGlyph />
      <p className="pageState__title">{props.title}</p>
      <p className="pageState__code">{props.code}</p>
      <p className="pageState__body">{props.reason}</p>
      <p className="pageState__advice">{props.advice}</p>
      <div className="pageState__actions">{props.actions}</div>
    </div>
  );
}

/* ==========================================================================
 * 最短显示 300 ms
 * ======================================================================== */

/** 骨架屏留住的开关。**不用布尔**（`useState(false)` 那种字面布尔初值在本项目是被禁的）。 */
type SkeletonHold = "hold" | "release";

/**
 * **哪些态会被"留住到 300 ms"**。
 *
 * 只列 `empty`：加载失败要**立刻**可见（原因见文件头），
 * 而"查完发现是空的"晚 300 ms 出现，不会误导任何人。
 */
const HELD_ON_LEAVE: readonly PageStateProps["kind"][] = ["empty"];

/** `matchMedia` 的可用性（jsdom 里没有它，而那是唯一会走到的例外）。 */
function prefersReducedMotion(): boolean {
  if (typeof window.matchMedia !== "function") return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * 返回"此刻要不要继续画骨架屏"。
 *
 * 这个钩子**只关心一件事**：`kind` 从 `loading` 离开之后，够不够 300 ms。
 * 它不缓存数据、不改变调用方能看到的任何东西 —— 除了屏幕上是骨架屏
 * 还是真内容。见文件头的机制说明。
 */
function useSkeletonHold(kind: PageStateProps["kind"]): boolean {
  const [hold, setHold] = useState<SkeletonHold>("release");
  // 加载态持续了多久 —— 用 ref 而不是 state：它只在事件回调里被读，
  // 写进 state 会为一个没人渲染的数字多触发一轮渲染。
  const startedAtRef = useRef<number | null>(null);
  // 回调要读"此刻还在不在加载"，而 effect 闭包拿到的是**那一刻**的 kind
  const loadingRef = useRef(false);
  loadingRef.current = kind === "loading";

  useEffect(() => {
    if (kind === "loading") {
      startedAtRef.current = Date.now();
      setHold("release");
      return;
    }

    const since = startedAtRef.current;
    startedAtRef.current = null;
    // 从别的态直接切到别的态（例如 empty → error）：**从来没有过骨架屏**，
    // 没有任何东西需要"显示满 300 ms"。
    if (since === null) return;
    // 换到的这个态自己说了"不用留"（目前只有 error 这么说）
    if (!HELD_ON_LEAVE.includes(kind)) return;

    const remaining = Math.max(0, MIN_SKELETON_MS - (Date.now() - since));
    if (remaining === 0 || prefersReducedMotion()) return;

    setHold("hold");
    // ⚠️ 加载态如果在计时器没到之前又回来了（重试 / 轮询），
    // 这个计时器必须失效 —— 否则它会把**新一次**加载的骨架屏提前收掉。
    const timer = window.setTimeout(() => {
      if (loadingRef.current) return;
      setHold("release");
    }, remaining);

    return () => {
      window.clearTimeout(timer);
    };
  }, [kind]);

  return hold === "hold";
}

/* ==========================================================================
 * 自绘图形（§6.1：图标一律自绘，禁止 emoji 与彩色插画）
 * ======================================================================== */

/**
 * 空态的内置图形：一只**空箱子**的线稿（48 px，`currentColor`）。
 *
 * 为什么是箱子而不是"文件夹 / 放大镜"：三态共用**同一个容器**，
 * 所以内置图形必须是**中性的**（"这里现在是空的"），
 * 具体语义由调用方通过 `icon` 给 —— 那才是它知道得比容器多的部分。
 */
function EmptyBoxGlyph(): ReactElement {
  return (
    <svg
      className="pageState__icon"
      viewBox="0 0 48 48"
      width="48"
      height="48"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      aria-hidden="true"
      focusable="false"
    >
      {/* 箱体：一个开口向上的梯形侧影 + 一条箱沿 */}
      <path d="M8 18h32v20a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2z" />
      <path d="M6 14h36v4H6z" />
      {/* 箱内的"什么都没有"：两道短横，读作"底" */}
      <path d="M17 27h14" />
      <path d="M20 33h8" />
    </svg>
  );
}

/**
 * 错误态的图形：警示三角 + 感叹号（48 px，颜色由 CSS 给 `--state-danger-fg`）。
 *
 * ⚠️ **错误态的图形不可替换** —— 属性里没有 `icon`。理由：失败的样子
 * 是这个容器的语义，不是调用方的；让每个页面各画一个"失败脸"
 * 正是 §7.6 开头那句"每个页面各写各的"要防的事。
 */
function ErrorGlyph(): ReactElement {
  return (
    <svg
      className="pageState__icon"
      viewBox="0 0 48 48"
      width="48"
      height="48"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M24 7 44 41H4z" />
      <path d="M24 19v11" />
      {/* 一个**圆点**而不是 `M24 35h.01`：零长度子路径 + `stroke-linecap: butt`
          **什么都不画**，"感叹号"会变成一个"三角形里一根竖线"。 */}
      <circle cx="24" cy="35" r="1" fill="currentColor" stroke="none" />
    </svg>
  );
}

/* ==========================================================================
 * 工具
 * ======================================================================== */

/** 拼 className，**丢掉 `false` / `undefined`**（与 `index.tsx` 同一条口径）。 */
function cls(...parts: readonly (string | false | undefined)[]): string {
  return parts.filter((p): p is string => typeof p === "string" && p !== "").join(" ");
}
