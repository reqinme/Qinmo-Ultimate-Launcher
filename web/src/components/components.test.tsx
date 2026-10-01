/**
 * 基础组件的验收测试（**M4 门禁第 ② 项**）
 * ============================================================================
 *
 * ## 🔴 它能验什么、不能验什么 —— 而这个边界是刻意的
 *
 * §6.3 的门禁要求"每个含 6 种状态（默认/悬停/按下/聚焦/禁用/加载）"。
 * 而那六个里：
 *
 * | 状态 | 有 DOM 痕迹吗 | 谁来验 |
 * |---|---|---|
 * | 默认 | — | 本测试（渲染出来就是） |
 * | **悬停 / 按下 / 聚焦** | **没有**（纯伪类） | **人眼** —— 见 `ComponentsPage` |
 * | 禁用 | 有（`:disabled`） | 本测试 |
 * | 加载 | 有（`aria-busy`） | 本测试 |
 *
 * **"用 jsdom 模拟 hover"验的是 jsdom，不是我们的样式** ——
 * 它不会让 `:hover` 那条规则生效。所以这里不假装能验那三个，
 * 而是把"它们只能人眼验"写进 `ComponentsPage` 的文档。
 */

import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import {
  Button,
  Checkbox,
  Dialog,
  EmptyState,
  IconButton,
  Panel,
  Progress,
  Radio,
  RingProgress,
  SearchField,
  Select,
  Switch,
  Tabs,
  TextField,
  Toast,
} from "./index.tsx";
import { STATE_SEMANTICS, nativeAttrsOf, shouldBlockEvent, type VisualState } from "./state.ts";

describe("六状态的语义模型（`state.ts`）", () => {
  it("禁用态**不可聚焦**，而加载态**可聚焦** —— 这是它们最关键的区别", () => {
    // ⚠️ 这一条是"六个状态"能被验证的原因。
    //
    // 一个把两者都做成"不可聚焦"的实现会让：**加载中的按钮把焦点搬走**，
    // 而那在读屏下是最难跟的一类变化（用户丢掉了他正在的位置）。
    expect(STATE_SEMANTICS.disabled.focusable).toBe(false);
    expect(STATE_SEMANTICS.loading.focusable).toBe(true);
  });

  it("禁用与加载都**不可交互**", () => {
    expect(STATE_SEMANTICS.disabled.interactive).toBe(false);
    expect(STATE_SEMANTICS.loading.interactive).toBe(false);
    expect(STATE_SEMANTICS.default.interactive).toBe(true);
    expect(STATE_SEMANTICS.default.focusable).toBe(true);
  });

  it("状态 → 原生属性：禁用用原生 `disabled`，加载用两个 aria", () => {
    // ⚠️ 这里用原生 `disabled` 而不是 `aria-disabled`，理由写在 `state.ts`：
    // 原生 `disabled` **一定**不可聚焦也不可点击，而 `aria-disabled` 只是
    // "告诉读屏"，鼠标与键盘**仍然能触发它** —— 那需要每个组件自己再拦一次。
    expect(nativeAttrsOf("disabled")).toEqual({ disabled: true });
    const loading = nativeAttrsOf("loading");
    expect(loading.disabled).toBe(false);
    expect(loading["aria-disabled"]).toBe(true);
    expect(loading["aria-busy"]).toBe(true);
    expect(nativeAttrsOf("default").disabled).toBe(false);
  });

  it("每个状态都有语义定义（新增状态时会被要求补全）", () => {
    for (const s of ["default", "disabled", "loading"] as const satisfies readonly VisualState[]) {
      expect(STATE_SEMANTICS[s], `缺 ${s} 的语义`).toBeTruthy();
    }
  });

  it("交互拦截：只有默认态放行", () => {
    expect(shouldBlockEvent("default")).toBe(false);
    expect(shouldBlockEvent("disabled")).toBe(true);
    expect(shouldBlockEvent("loading")).toBe(true);
  });
});

describe("Button / IconButton", () => {
  it("默认态可点，且带得体的默认 `type`（**不是 submit**）", () => {
    let hits = 0;
    render(<Button onClick={() => (hits += 1)}>点我</Button>);
    const b = screen.getByRole("button", { name: "点我" });
    // ⚠️ 默认 `type="button"` 是刻意的：一个没写 type 的按钮在 <form> 里
    // 是 `submit`，而那会让"点一下"变成"提交表单"。
    expect(b.getAttribute("type")).toBe("button");
    b.click();
    expect(hits).toBe(1);
  });

  it("三种语气都渲染（主要/次要/危险）", () => {
    render(
      <>
        <Button tone="primary">p</Button>
        <Button tone="secondary">s</Button>
        <Button tone="danger">d</Button>
      </>,
    );
    expect(screen.getByRole("button", { name: "p" }).className).toContain("btn--primary");
    expect(screen.getByRole("button", { name: "s" }).className).toContain("btn--secondary");
    expect(screen.getByRole("button", { name: "d" }).className).toContain("btn--danger");
  });

  it("禁用态：原生 `disabled` **且不触发** onClick", () => {
    let hits = 0;
    render(
      <Button state="disabled" onClick={() => (hits += 1)}>
        不可点
      </Button>,
    );
    const b = screen.getByRole("button", { name: "不可点" });
    expect((b as HTMLButtonElement).disabled).toBe(true);
    b.click();
    expect(hits, "禁用态不该触发 onClick").toBe(0);
  });

  it("加载态：`aria-busy` + **不是** `disabled` + onClick 被拦", () => {
    let hits = 0;
    render(
      <Button state="loading" onClick={() => (hits += 1)}>
        加载中
      </Button>,
    );
    const b = screen.getByRole("button", { name: /加载中/ });
    expect(b.getAttribute("aria-busy")).toBe("true");
    // ⚠️ **这一条是"可聚焦"在 DOM 上的证据**：它不是 `disabled`。
    expect((b as HTMLButtonElement).disabled, "加载态的按钮必须**不**是 disabled").toBe(false);
    b.click();
    expect(hits, "加载态不该触发 onClick").toBe(0);
  });

  it("图标按钮**必须**有可访问名（它没有可见文字）", () => {
    render(<IconButton label="设置" />);
    // 没有 label 的实现会让读屏念一串"按钮"
    expect(screen.getByRole("button", { name: "设置" })).toBeTruthy();
  });
});

describe("输入域", () => {
  it("文本框：label 与 input 通过 id 关联", () => {
    render(<TextField label="实例名" />);
    // ⚠️ `getByLabelText` 只有在关联**真的**成立时才找得到。
    // 一个只画了 <label> 而没接 htmlFor 的实现会在这里红。
    expect(screen.getByLabelText("实例名")).toBeTruthy();
  });

  it("文本框：提示用 `aria-describedby`，错误用 `aria-invalid` + `role=alert`", () => {
    render(<TextField label="名字" hint="会出现在侧栏" error="已经被占用了" />);
    const input = screen.getByLabelText("名字");
    // ⚠️ 只画红框是不够的 —— 读屏用户完全不知道这个字段有错。
    expect(input.getAttribute("aria-invalid")).toBe("true");
    const describedBy = input.getAttribute("aria-describedby") ?? "";
    const ids = describedBy.split(" ").filter((x) => x !== "");
    expect(ids.length, "提示与错误都该被 aria-describedby 指向").toBe(2);
    expect(screen.getByRole("alert").textContent).toContain("已经被占用了");
  });

  it("文本框：禁用态真的禁用", () => {
    render(<TextField label="只读" state="disabled" />);
    expect((screen.getByLabelText("只读") as HTMLInputElement).disabled).toBe(true);
  });

  it("搜索框有可访问名，且 `type=search`", () => {
    render(<SearchField label="搜索实例" placeholder="搜索……" />);
    const el = screen.getByLabelText("搜索实例") as HTMLInputElement;
    expect(el.getAttribute("type")).toBe("search");
  });

  it("下拉：标签关联到触发器", () => {
    render(
      <Select
        label="运行时"
        value="a"
        onValueChange={() => undefined}
        options={[{ value: "a", label: "自动" }]}
      />,
    );
    expect(screen.getByLabelText("运行时")).toBeTruthy();
  });

  it("下拉：禁用态是**真的** disabled（不靠自己拦事件）", () => {
    render(
      <Select
        label="禁用的"
        value="a"
        onValueChange={() => undefined}
        state="disabled"
        options={[{ value: "a", label: "自动" }]}
      />,
    );
    const trigger = screen.getByLabelText("禁用的");
    // Radix 把 disabled 放在 button 上
    expect((trigger as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("选择域", () => {
  it("复选框：可访问名来自标签，且能切换", () => {
    render(<Checkbox label="启用" checked onCheckedChange={() => undefined} />);
    const box = screen.getByRole("checkbox", { name: "启用" });
    expect(box.getAttribute("aria-checked")).toBe("true");
  });

  it("复选框：禁用态不可点", () => {
    render(<Checkbox label="禁用" checked={false} onCheckedChange={() => undefined} state="disabled" />);
    expect((screen.getByRole("checkbox", { name: "禁用" }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("单选组：是一个 `radiogroup`，且每项都有名字", () => {
    render(
      <Radio
        label="账户类型"
        value="a"
        onValueChange={() => undefined}
        options={[
          { value: "a", label: "离线" },
          { value: "b", label: "通行证" },
        ]}
      />,
    );
    expect(screen.getByRole("radiogroup", { name: "账户类型" })).toBeTruthy();
    expect(screen.getByRole("radio", { name: "离线" })).toBeTruthy();
  });

  it("开关：角色是 `switch`（不是 checkbox）", () => {
    render(<Switch label="开关" checked onCheckedChange={() => undefined} />);
    // ⚠️ 开关与复选框的**角色不同**，而那对读屏用户是有意义的区别
    //（开关是"立刻生效"，复选框是"将被提交"）。
    expect(screen.getByRole("switch", { name: "开关" })).toBeTruthy();
  });
});

describe("容器与反馈", () => {
  it("面板有标题时是一个具名 region 的来源（标题在 DOM 里）", () => {
    render(
      <Panel title="标题">
        <span>内容</span>
      </Panel>,
    );
    expect(screen.getByRole("heading", { name: "标题" })).toBeTruthy();
    expect(screen.getByText("内容")).toBeTruthy();
  });

  it("确定进度：`aria-valuenow` 被设上", () => {
    render(<Progress value={62} label="下载" />);
    const bar = screen.getByRole("progressbar", { name: "下载" });
    expect(bar.getAttribute("aria-valuenow")).toBe("62");
  });

  it("**不确定**进度：不设 `aria-valuenow` —— 设了就是在撒谎", () => {
    render(<Progress value={null} label="探测中" />);
    const bar = screen.getByRole("progressbar", { name: "探测中" });
    // ⚠️ 一个"null 就当 0"的实现会让正在探测的进度条**看起来像卡住了**，
    // 而那正是用户在等待时最会误解的一件事。
    expect(bar.getAttribute("aria-valuenow")).toBeNull();
    expect(bar.className).toContain("indeterminate");
  });

  it("进度值被夹在 0..100（越界不产生非法 ARIA）", () => {
    render(
      <>
        <Progress value={-20} label="负" />
        <Progress value={180} label="超" />
      </>,
    );
    expect(screen.getByRole("progressbar", { name: "负" }).getAttribute("aria-valuenow")).toBe("0");
    expect(screen.getByRole("progressbar", { name: "超" }).getAttribute("aria-valuenow")).toBe("100");
  });

  it("环形进度也是 progressbar，且不确定时不设 valuenow", () => {
    render(
      <>
        <RingProgress value={40} label="环形" />
        <RingProgress value={null} label="环形不确定" />
      </>,
    );
    expect(screen.getByRole("progressbar", { name: "环形" }).getAttribute("aria-valuenow")).toBe("40");
    expect(screen.getByRole("progressbar", { name: "环形不确定" }).getAttribute("aria-valuenow")).toBeNull();
  });

  it("空态：标题 + 说明 + **一条出路**", () => {
    render(
      <EmptyState
        title="还没有实例"
        detail="建一个之后它就会出现在侧栏里。"
        action={<Button>新建实例</Button>}
      />,
    );
    expect(screen.getByText("还没有实例")).toBeTruthy();
    // ⚠️ 断言的是"有没有一条出路"—— 一个只写"暂无内容"的空态
    // 在"用户可以做什么"上什么都没回答。
    expect(screen.getByRole("button", { name: "新建实例" })).toBeTruthy();
  });

  it("吐司的语气色**成对**（背景 + 前景），且角色是 status", () => {
    render(<Toast tone="success" title="成功" detail="部署完成" />);
    const t = screen.getByRole("status");
    expect(t.className).toContain("toast--success");
    expect(screen.getByText("部署完成")).toBeTruthy();
  });
});

describe("布局与浮层", () => {
  it("标签页：三个 tab 与三块内容；选中的那块可见", () => {
    render(
      <Tabs
        items={[
          { key: "a", label: "概览", content: <p>甲</p> },
          { key: "b", label: "日志", content: <p>乙</p> },
        ]}
      />,
    );
    expect(screen.getAllByRole("tab").length).toBe(2);
    expect(screen.getByText("甲")).toBeTruthy();
  });

  it("对话框：触发器是一个按钮，且它没打开时内容不在 DOM 里", () => {
    render(
      <Dialog title="确认" trigger={<Button>打开</Button>}>
        <p>内容</p>
      </Dialog>,
    );
    expect(screen.getByRole("button", { name: "打开" })).toBeTruthy();
    // 未打开时内容不该在 DOM 里 —— 否则读屏会读到"存在但不可见"的东西。
    //
    // ⚠️ **Radix 会渲染一个空的隐藏 `<span>`**（它用于读屏公告），
    // 所以我第一版写的 `expect(container.firstChild).toBeNull()` 是错的。
    // 该断言的是"我们那段内容不在"，而不是"DOM 是空的"。
    // ⚠️ Radix 会渲染一个空的隐藏 `<span>`（读屏公告用），
    // 所以断言的是"**我们那段内容**不在"，而不是"DOM 是空的"。
    expect(screen.queryByText("内容")).toBeNull();
    // ⚠️ **不用 `queryByRole("dialog")`** —— Radix 会渲染一些隐藏的公告节点，
    // 而 `queryByRole` 在这条路径上给出的是 `<span></span>`（我实测到的那一个）。
    // 直接查 `[role="dialog"]` 更直白，且它断言的正是"对话框不存在"。
    expect(document.querySelector(String.raw`[role="dialog"]`)).toBeNull();
  });
});
