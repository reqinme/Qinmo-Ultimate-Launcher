/**
 * 基础组件骨架（**M4 门禁第 ② 项**，§6.4 的最低集 20 个）
 * ============================================================================
 *
 * ## 口径（§6.3 门禁原文）
 *
 * > **只做样式与 a11y，不做业务**；**每个含 6 种状态**
 * >（默认/悬停/按下/聚焦/禁用/加载）
 *
 * ## 🔴 三条实现纪律
 *
 * ### ① 样式全自写，Radix 只给行为与 a11y
 *
 * §4.5 原文：*"Radix **无样式、可访问性已做好、与设计系统完全解耦** ——
 * 我们只取行为与 a11y，样式全部自己写（**我们的设计语言不允许现成皮肤**）。"*
 *
 * ### ② 六状态只有**两个**是 props，其余四个是伪类
 *
 * 见 `state.ts` 的文档。这条是本轮最容易做错的地方 ——
 * 一个把 `hover` 也做成 prop 的实现会逼每个调用方自己管鼠标状态。
 *
 * ### ③ **组件不认识产品**（门禁⑤ 的 lint 规则）
 *
 * 所以这里没有一个 `if (product === ...)`，也没有一个产品名。
 */
import {
  createContext,
  forwardRef,
  useContext,
  useId,
  useState,
  type ButtonHTMLAttributes,
  type HTMLAttributes,
  type InputHTMLAttributes,
  type ReactElement,
  type ReactNode,
} from "react";
import * as RCheckbox from "@radix-ui/react-checkbox";
import * as RDialog from "@radix-ui/react-dialog";
import * as RPopover from "@radix-ui/react-popover";
import * as RRadio from "@radix-ui/react-radio-group";
import * as RSelect from "@radix-ui/react-select";
import * as RSwitch from "@radix-ui/react-switch";
import * as RTabs from "@radix-ui/react-tabs";
import * as RTooltip from "@radix-ui/react-tooltip";
import { nativeAttrsOf, guardHandler, type VisualState } from "./state.ts";
import "./components.css";

/* ==========================================================================
 * 操作域：Button（主要/次要/危险/图标）· IconButton
 * ======================================================================== */

export type ButtonTone = "primary" | "secondary" | "danger";

export interface ButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "disabled"> {
  readonly tone?: ButtonTone;
  readonly state?: VisualState;
  readonly children: ReactNode;
}

/**
 * 按钮。
 *
 * ⚠️ **`state` 而不是 `disabled` / `loading` 两个布尔。**
 * 两个布尔允许"同时禁用且加载"这种非法组合 —— 而它会让两种样式叠加，
 * 表现为一个既灰又转的按钮。`VisualState` 让那个组合**在类型上不存在**。
 */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { tone = "secondary", state = "default", children, onClick, className, ...rest },
  ref,
): ReactElement {
  return (
    <button
      {...rest}
      {...nativeAttrsOf(state)}
      ref={ref}
      type={rest.type ?? "button"}
      className={cls("btn", `btn--${tone}`, state !== "default" && `btn--${state}`, className)}
      onClick={guardHandler(state, onClick)}
    >
      {state === "loading" ? <Spinner /> : null}
      <span>{children}</span>
    </button>
  );
});

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "disabled"> {
  readonly label: string;
  readonly state?: VisualState;
  /** **可选**：加载态会把它换成一个转圈，而不给就是"只有转圈"。 */
  readonly children?: ReactNode;
}

/**
 * 图标按钮。**`label` 是必填的**，因为它没有可见文字。
 *
 * 一个"图标按钮不需要标签"的实现会让读屏用户听到一串"按钮"。
 */
export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { label, state = "default", children, onClick, className, ...rest },
  ref,
): ReactElement {
  return (
    <button
      {...rest}
      {...nativeAttrsOf(state)}
      ref={ref}
      type={rest.type ?? "button"}
      aria-label={label}
      className={cls("btn", "btn--icon", state !== "default" && `btn--${state}`, className)}
      onClick={guardHandler(state, onClick)}
    >
      {state === "loading" ? <Spinner /> : children}
    </button>
  );
});

/* ==========================================================================
 * 输入域：TextField · SearchField · Select
 * ======================================================================== */

export interface TextFieldProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "disabled" | "id"> {
  readonly label: string;
  readonly hint?: string;
  readonly error?: string;
  readonly state?: VisualState;
}

export const TextField = forwardRef<HTMLInputElement, TextFieldProps>(function TextField(
  { label, hint, error, state = "default", className, ...rest },
  ref,
): ReactElement {
  const id = useId();
  const describedBy = [hint !== undefined ? `${id}-hint` : null, error !== undefined ? `${id}-err` : null]
    .filter((x): x is string => x !== null)
    .join(" ");
  return (
    <div className={cls("field", state !== "default" && `field--${state}`, className)}>
      <label className="field__label" htmlFor={id}>
        {label}
      </label>
      <input
        {...rest}
        {...nativeAttrsOf(state)}
        ref={ref}
        id={id}
        className="field__input"
        aria-describedby={describedBy === "" ? undefined : describedBy}
        // ⚠️ **错误用 `aria-invalid` 而不只是一个红框。** 一个只画红框的实现
        // 让读屏用户完全不知道这个字段有错。
        aria-invalid={error !== undefined ? true : undefined}
      />
      {hint !== undefined ? (
        <p className="field__hint" id={`${id}-hint`}>
          {hint}
        </p>
      ) : null}
      {error !== undefined ? (
        <p className="field__error" id={`${id}-err`} role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
});

export interface SearchFieldProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, "disabled" | "id" | "type"> {
  readonly label: string;
  readonly state?: VisualState;
}

export const SearchField = forwardRef<HTMLInputElement, SearchFieldProps>(function SearchField(
  { label, state = "default", className, ...rest },
  ref,
): ReactElement {
  const id = useId();
  return (
    <div className={cls("field field--search", state !== "default" && `field--${state}`, className)}>
      <label className="field__label field__label--sr" htmlFor={id}>
        {label}
      </label>
      <span className="field__searchGlyph" aria-hidden="true" />
      <input
        {...rest}
        {...nativeAttrsOf(state)}
        ref={ref}
        id={id}
        type="search"
        className="field__input field__input--search"
      />
    </div>
  );
});

export interface SelectOption {
  readonly value: string;
  readonly label: string;
}

export interface SelectProps {
  readonly label: string;
  readonly value: string;
  readonly options: readonly SelectOption[];
  readonly onValueChange: (v: string) => void;
  readonly state?: VisualState;
  readonly placeholder?: string;
}

/** 下拉选择。行为与 a11y 来自 Radix；样式全自写。 */
export function Select({
  label,
  value,
  options,
  onValueChange,
  state = "default",
  placeholder,
}: SelectProps): ReactElement {
  const id = useId();
  const blocked = state !== "default";
  return (
    <div className={cls("field", blocked && `field--${state}`)}>
      <label className="field__label" htmlFor={id}>
        {label}
      </label>
      <RSelect.Root
        value={value}
        onValueChange={onValueChange}
        // ⚠️ Radix 的 `disabled` 让我们**不用自己拦事件** ——
        // 而那正是"用无头组件而不是自己写"的价值。
        disabled={blocked}
      >
        <RSelect.Trigger id={id} className="select__trigger" aria-busy={state === "loading"}>
          <RSelect.Value placeholder={placeholder ?? "请选择"} />
          <span className="select__chevron" aria-hidden="true" />
        </RSelect.Trigger>
        <RSelect.Portal>
          <RSelect.Content className="select__content" position="popper" sideOffset={4}>
            <RSelect.Viewport>
              {options.map((o) => (
                <RSelect.Item key={o.value} value={o.value} className="select__item">
                  <RSelect.ItemText>{o.label}</RSelect.ItemText>
                </RSelect.Item>
              ))}
            </RSelect.Viewport>
          </RSelect.Content>
        </RSelect.Portal>
      </RSelect.Root>
    </div>
  );
}

/* ==========================================================================
 * 选择域：Checkbox · Radio · Switch
 * ======================================================================== */

export interface CheckboxProps {
  readonly label: string;
  readonly checked: boolean;
  readonly onCheckedChange: (v: boolean) => void;
  readonly state?: VisualState;
}

export function Checkbox({
  label,
  checked,
  onCheckedChange,
  state = "default",
}: CheckboxProps): ReactElement {
  const id = useId();
  return (
    <div className={cls("choice", state !== "default" && `choice--${state}`)}>
      <RCheckbox.Root
        id={id}
        className="choice__box"
        checked={checked}
        onCheckedChange={(v) => onCheckedChange(v === true)}
        disabled={state !== "default"}
      >
        {/* ✓ 的样式来自 `.choice__box[data-state="checked"]` 的 color，不是这里。 */}
        <RCheckbox.Indicator>✓</RCheckbox.Indicator>
      </RCheckbox.Root>
      <label className="choice__label" htmlFor={id}>
        {label}
      </label>
    </div>
  );
}

export interface RadioProps {
  readonly label: string;
  readonly value: string;
  readonly options: readonly SelectOption[];
  readonly onValueChange: (v: string) => void;
  readonly state?: VisualState;
}

export function Radio({
  label,
  value,
  options,
  onValueChange,
  state = "default",
}: RadioProps): ReactElement {
  const name = useId();
  /*
    ⚠️ 这里**故意不再挂 `fieldset--${state}`**（它是被 `tools/check-css-classes.ps1`
    的 R2 找出来的）。

    它曾经挂过，而 `components.css` 里一个 `.fieldset--*` 都没有 ——
    一个"发出来但没人写规则"的类，在真机上**完全没有症状**：看不出错，
    也测不出来。而这个 Radio 的禁用视觉本来就由内层
    `RRadio.Root disabled` + `.choice__radio:disabled` 表达（这个组没有
    `<legend>`，可访问名走 `aria-label`，所以外层没有可表达的东西）。

    以后真需要组一级的视觉，就**同时**加类和规则。

    ⚠️ 而且这段说明**必须写在 `return (` 外面**：它第一次写在了 `return (`
    之后、用花括号包着的 JSX 注释 —— 那是一个 JSX 表达式，于是 `return (`
    就有两个孩子，tsc 直接报 `TS1005: ')' expected`。
    `web/src/routes/Shell.tsx:79-81` 早就记着这条，我还是踩了一次。
    （而写这段注释时我又踩了第二次：注释里原样写出那段符号，它的结尾
    提前关掉了这一整段块注释。）
  */
  return (
    <div className="fieldset">
      {/*
        ⚠️ **可访问名用显式的 `aria-label`，而不是 `<legend>`。**

        我第一版写的是 `<fieldset>` + `<legend>`，并**假设**那会构成一个
        可访问名 —— 而实测界面上它**不是**（`getByRole("radiogroup", { name })`
        找不到）。所以这里给一个显式的 `aria-label`，而它更可靠。
      */}
      <RRadio.Root
        className="choiceGroup"
        value={value}
        onValueChange={onValueChange}
        disabled={state !== "default"}
        name={name}
        aria-label={label}
      >
        {options.map((o) => (
          <div className="choice" key={o.value}>
            <RRadio.Item className="choice__radio" value={o.value} id={`${name}-${o.value}`}>
              <RRadio.Indicator className="choice__dot" />
            </RRadio.Item>
            <label className="choice__label" htmlFor={`${name}-${o.value}`}>
              {o.label}
            </label>
          </div>
        ))}
      </RRadio.Root>
    </div>
  );
}

export interface SwitchProps {
  readonly label: string;
  readonly checked: boolean;
  readonly onCheckedChange: (v: boolean) => void;
  readonly state?: VisualState;
}

export function Switch({
  label,
  checked,
  onCheckedChange,
  state = "default",
}: SwitchProps): ReactElement {
  const id = useId();
  return (
    <div className={cls("choice", state !== "default" && `choice--${state}`)}>
      <RSwitch.Root
        id={id}
        className="switch"
        checked={checked}
        onCheckedChange={onCheckedChange}
        disabled={state !== "default"}
      >
        <RSwitch.Thumb className="switch__thumb" />
      </RSwitch.Root>
      <label className="choice__label" htmlFor={id}>
        {label}
      </label>
    </div>
  );
}

/* ==========================================================================
 * 容器域：Panel · Card
 * ======================================================================== */

export interface PanelProps extends HTMLAttributes<HTMLElement> {
  readonly title?: string;
  readonly children: ReactNode;
}

/**
 * 容器面板。
 *
 * ⚠️ **`elevation` 不在这里给** —— §2 约束 6 说"阴影只有 4 档，不许即兴加"，
 * 而 `Panel` 是**最底层**的容器，它**没有阴影**（`elev.0`）。
 * 一个让调用方传 `elevation` 的实现会立刻被用来做"浮动面板"，
 * 而那正是 `surface.raised` 存在的理由。
 */
export function Panel({ title, children, className, ...rest }: PanelProps): ReactElement {
  return (
    <section {...rest} className={cls("panel", className)}>
      {title !== undefined ? <h2 className="panel__title">{title}</h2> : null}
      <div className="panel__body">{children}</div>
    </section>
  );
}

export interface CardProps extends HTMLAttributes<HTMLElement> {
  readonly children: ReactNode;
}

export function Card({ children, className, ...rest }: CardProps): ReactElement {
  return (
    <div {...rest} className={cls("card", className)}>
      {children}
    </div>
  );
}

/* ==========================================================================
 * 浮层域：Dialog · Popover · Tooltip · Toast
 * ======================================================================== */

export interface DialogProps {
  readonly trigger: ReactNode;
  readonly title: string;
  readonly description?: string;
  readonly children: ReactNode;
  // ⚠️ `exactOptionalPropertyTypes` 之下，`?:` 表示"可以不给"，
  // 而 `| undefined` 才表示"可以显式给 undefined"。透传 props 时要后者。
  readonly open?: boolean | undefined;
  readonly onOpenChange?: ((v: boolean) => void) | undefined;
}

export function Dialog({
  trigger,
  title,
  description,
  children,
  open,
  onOpenChange,
}: DialogProps): ReactElement {
  return (
  // ⚠️ 遮罩用 `--overlay-scrim` 令牌（§5.1 补的那一项），
  // 而不是字面 rgba —— 那是"界面不写色值"的落点。
    // ⚠️ **条件展开**，而不是直接传 `open={open}` ——
    // `exactOptionalPropertyTypes` 之下 `boolean | undefined` 不能赋给 `boolean`。
    // 而"让接收方允许 undefined"是更差的选择：那会把 `undefined`
    // 当成一个合法状态传进 Radix。
    <RDialog.Root
      onOpenChange={onOpenChange ?? ((): void => undefined)}
      {...(open === undefined ? {} : { open })}
    >
      <RDialog.Overlay className="overlay" />
      <RDialog.Trigger asChild>{trigger}</RDialog.Trigger>
      <RDialog.Portal>
        <RDialog.Content className="dialog">
          <RDialog.Title className="dialog__title">{title}</RDialog.Title>
          {description !== undefined ? (
            <RDialog.Description className="dialog__desc">{description}</RDialog.Description>
          ) : null}
          <div className="dialog__body">{children}</div>
          <RDialog.Close asChild>
            <Button tone="secondary">关闭</Button>
          </RDialog.Close>
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  );
}

export interface PopoverProps {
  readonly trigger: ReactNode;
  readonly children: ReactNode;
}

export function Popover({ trigger, children }: PopoverProps): ReactElement {
  return (
    <RPopover.Root>
      <RPopover.Trigger asChild>{trigger}</RPopover.Trigger>
      <RPopover.Portal>
        <RPopover.Content className="popover" sideOffset={6}>
          {children}
          <RPopover.Arrow className="popover__arrow" />
        </RPopover.Content>
      </RPopover.Portal>
    </RPopover.Root>
  );
}

export interface TooltipProps {
  readonly label: string;
  readonly children: ReactNode;
}

export function Tooltip({ label, children }: TooltipProps): ReactElement {
  return (
    <RTooltip.Provider delayDuration={400}>
      <RTooltip.Root>
        <RTooltip.Trigger asChild>{children}</RTooltip.Trigger>
        <RTooltip.Portal>
          <RTooltip.Content className="tooltip" sideOffset={6}>
            {label}
          </RTooltip.Content>
        </RTooltip.Portal>
      </RTooltip.Root>
    </RTooltip.Provider>
  );
}

/** Toast 的语气 —— **它复用状态色令牌，不新开一套**。 */
export type ToastTone = "info" | "success" | "warning" | "danger";

/**
 * 一条 Toast。
 *
 * ⚠️ **它刻意不自带队列。** 队列是「灵动岛」的职责（§7.1：
 * "任意时刻只有一个岛、一个主状态，多个通知**排队**，不并排"）——
 * 一个自带队列的 Toast 组件会变成**第二个通知中心**，
 * 而那与灵动岛的存在理由直接冲突。
 */
export function Toast({
  tone = "info",
  title,
  detail,
  onDismiss,
}: {
  readonly tone?: ToastTone;
  readonly title: string;
  readonly detail?: string;
  readonly onDismiss?: () => void;
}): ReactElement {
  return (
    <div className={cls("toast", `toast--${tone}`)} role="status">
      <div className="toast__body">
        <p className="toast__title">{title}</p>
        {detail !== undefined ? <p className="toast__detail">{detail}</p> : null}
      </div>
      {onDismiss !== undefined ? (
        <IconButton label="关闭" onClick={onDismiss} className="toast__close">
          ×
        </IconButton>
      ) : null}
    </div>
  );
}

/* ==========================================================================
 * 反馈域：Progress（线性/环形）· EmptyState
 * ======================================================================== */

export interface ProgressProps {
  /** `null` = **不确定进度**（它必须有自己的样子，而不是画成 0%）。 */
  readonly value: number | null;
  readonly label?: string;
}

/**
 * 进度。
 *
 * ⚠️ **`value === null` 是"不确定"，不是 0。**
 * 一个"null 就当 0"的实现会让一个正在探测的进度条**看起来像卡住了** ——
 * 而那正是用户在等待时最会误解的一件事。
 */
export function Progress({ value, label }: ProgressProps): ReactElement {
  const indeterminate = value === null;
  const pct = indeterminate ? 0 : Math.max(0, Math.min(100, value));
  return (
    <div className="progress">
      {label !== undefined ? <p className="progress__label">{label}</p> : null}
      <div
        className={cls("progress__track", indeterminate && "progress__track--indeterminate")}
        role="progressbar"
        // ⚠️ 不确定进度**不设 `aria-valuenow`** —— 设了就是在撒谎。
        aria-valuenow={indeterminate ? undefined : Math.round(pct)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label={label ?? "进度"}
      >
        <div className="progress__fill" style={{ inlineSize: `${pct}%` }} />
      </div>
    </div>
  );
}

/** 环形进度：**用 `conic-gradient` 而不是画 SVG 弧** —— 它不需要额外的 DOM。 */
export function RingProgress({ value, label }: ProgressProps): ReactElement {
  const pct = value === null ? 0 : Math.max(0, Math.min(100, value));
  return (
    <div
      className="ring"
      role="progressbar"
      aria-valuenow={value === null ? undefined : Math.round(pct)}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={label ?? "进度"}
      style={{ background: `conic-gradient(var(--accent) ${pct * 3.6}deg, var(--surface-active) 0deg)` }}
    >
      <span className="ring__hole">
        <span className="ring__text">{value === null ? "…" : `${Math.round(pct)}%`}</span>
      </span>
    </div>
  );
}

export interface EmptyStateProps {
  readonly title: string;
  readonly detail?: string;
  readonly action?: ReactNode;
}

/**
 * 空态。
 *
 * ⚠️ **它必须接受一个 `action`。** 一个只写"暂无内容"的空态在
 * "用户可以做什么"这个问题上什么都没回答 —— 而空态出现的时刻
 * 恰好是用户最需要那句话的时刻。
 */
export function EmptyState({ title, detail, action }: EmptyStateProps): ReactElement {
  return (
    <div className="empty">
      <p className="empty__title">{title}</p>
      {detail !== undefined ? <p className="empty__detail">{detail}</p> : null}
      {action !== undefined ? <div className="empty__action">{action}</div> : null}
    </div>
  );
}

/* ==========================================================================
 * 布局域：Tabs · Sidebar
 * ======================================================================== */

export interface TabItem {
  readonly key: string;
  readonly label: string;
  readonly content: ReactNode;
}

export function Tabs({
  items,
  defaultKey,
}: {
  readonly items: readonly TabItem[];
  readonly defaultKey?: string;
}): ReactElement {
  const first = items[0]?.key ?? "";
  return (
    <RTabs.Root className="tabs" defaultValue={defaultKey ?? first}>
      <RTabs.List className="tabs__list">
        {items.map((t) => (
          <RTabs.Trigger key={t.key} value={t.key} className="tabs__trigger">
            {t.label}
          </RTabs.Trigger>
        ))}
      </RTabs.List>
      {items.map((t) => (
        <RTabs.Content key={t.key} value={t.key} className="tabs__content">
          {t.content}
        </RTabs.Content>
      ))}
    </RTabs.Root>
  );
}

/**
 * 侧栏容器。
 *
 * ⚠️ **它只提供布局与 a11y，不提供导航数据。**
 * 导航内容由 `routes/Shell.tsx` 从 `nav.ts` 给 —— 一个"侧栏自己知道有哪几项"
 * 的实现会让 §4.6.1 的两条硬规则各有两个真相来源。
 */
export function Sidebar({
  label,
  children,
  className,
}: {
  readonly label: string;
  readonly children: ReactNode;
  readonly className?: string;
}): ReactElement {
  return (
    <nav {...(className === undefined ? {} : { className })} aria-label={label}>
      {children}
    </nav>
  );
}

/* ==========================================================================
 * 内部：加载指示
 * ======================================================================== */

/**
 * 一个**纯 CSS** 的旋转指示器。
 *
 * ⚠️ **它遵守 `prefers-reduced-motion`** —— 而其做法**不是**把它停掉
 *（那会让"在加载"这个信息消失，而 §5.4.3 说信息性"永远存在"），
 * 而是**把转速降到几乎不可察觉**（见 `components.css`）。
 */
function Spinner(): ReactElement {
  return <span className="spinner" aria-hidden="true" />;
}

/* ==========================================================================
 * 主题（材质/强度）的调试开关 —— **组件层不碰它**，这里只导出类型
 * ======================================================================== */

/** 主题上下文（`data-theme` / `data-material` / `data-intensity`）。 */
export interface PresentationPrefs {
  readonly theme: "system" | "dark" | "light";
  readonly material: "full" | "reduced" | "none";
  readonly intensity: "standard" | "enhanced";
}

const PrefsCtx = createContext<PresentationPrefs>({
  theme: "system",
  material: "full",
  intensity: "standard",
});

export function PresentationProvider({
  prefs,
  children,
}: {
  readonly prefs: PresentationPrefs;
  readonly children: ReactNode;
}): ReactElement {
  return <PrefsCtx.Provider value={prefs}>{children}</PrefsCtx.Provider>;
}

export function usePresentationPrefs(): PresentationPrefs {
  return useContext(PrefsCtx);
}

/* ==========================================================================
 * 工具
 * ======================================================================== */

/** 拼 className，**丢掉 `false` / `undefined`**。 */
function cls(...parts: readonly (string | false | undefined)[]): string {
  return parts.filter((p): p is string => typeof p === "string" && p !== "").join(" ");
}

/** 一个受控状态的小助手（示例与测试用）。 */
export function useToggle(initial: boolean): [boolean, (v: boolean) => void] {
  const [v, setV] = useState(initial);
  return [v, setV];
}
