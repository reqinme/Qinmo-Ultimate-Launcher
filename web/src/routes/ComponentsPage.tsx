import { useState, type ReactElement } from "react";
import {
  Button,
  Card,
  Checkbox,
  Dialog,
  EmptyState,
  IconButton,
  Panel,
  Popover,
  Progress,
  Radio,
  RingProgress,
  SearchField,
  Select,
  Sidebar,
  Switch,
  Tabs,
  TextField,
  Toast,
  Tooltip,
} from "../components/index.tsx";
import "./ComponentsPage.css";

/**
 * 基础组件展示（**M4 门禁第 ② 项的自验页**）
 * ============================================================================
 *
 * ## 它存在的理由：**六状态必须能被眼睛验到**
 *
 * §6.3 的门禁要求"每个含 6 种状态（默认/悬停/按下/聚焦/禁用/加载）"。
 * 而其中**三个是伪类**（悬停/按下/聚焦）—— 它们**没有 DOM 痕迹**，
 * 所以自动化测试**验不了**它们（除非模拟真实指针与键盘事件，
 * 而那验的是 jsdom 而不是我们的样式）。
 *
 * 所以这一页是那三个状态的**唯一验收手段**：人打开它，把鼠标放上去、
 * 按下去、Tab 过去，看它是不是四件事都发生了。
 *
 * ⚠️ 而**自动化测试验另外三个**（默认/禁用/加载）—— 它们有 DOM 痕迹
 * （`:disabled` 与 `aria-busy`）。**两条路合起来才是"六状态齐备"。**
 */
export function ComponentsPage(): ReactElement {
  const [checked, setChecked] = useState(false);
  const [radio, setRadio] = useState("a");
  const [sw, setSw] = useState(true);
  const [sel, setSel] = useState("a");
  const [text, setText] = useState("");

  return (
    <div className="gallery">
      <h1 className="page__title">基础组件（20 个 · 六状态）</h1>
      <p className="page__note">
        门禁② 的自验页。把鼠标放上去、按下去、按 Tab 走过去 ——
        <strong>悬停 / 按下 / 聚焦</strong>这三个状态没有 DOM 痕迹，
        所以它们只能在这里用眼睛验。
        <strong>默认 / 禁用 / 加载</strong>由自动化测试验（它们有 <code>:disabled</code> 与{" "}
        <code>aria-busy</code>）。
      </p>

      <Panel title="① 操作 · Button（主要/次要/危险/图标）· IconButton">
        <div className="gallery__row">
          <Button tone="primary">主要</Button>
          <Button tone="secondary">次要</Button>
          <Button tone="danger">危险</Button>
          <IconButton label="设置">⚙</IconButton>
        </div>
        <div className="gallery__row">
          <Button tone="primary" state="disabled">
            禁用的主要
          </Button>
          <Button tone="secondary" state="disabled">
            禁用的次要
          </Button>
          <Button tone="primary" state="loading">
            加载中（**仍可聚焦**）
          </Button>
          <IconButton label="加载中" state="loading" />
        </div>
      </Panel>

      <Panel title="② 输入 · TextField · SearchField · Select">
        <div className="gallery__grid">
          <TextField label="实例名" hint="会出现在侧栏里" value={text} onChange={(e) => setText(e.target.value)} />
          <TextField label="有错的字段" error="这个名字已经被占用了" defaultValue="重复的名字" />
          <TextField label="禁用的字段" state="disabled" defaultValue="不可编辑" />
          <SearchField label="搜索" placeholder="搜索实例……" />
          <Select
            label="运行时"
            value={sel}
            onValueChange={setSel}
            options={[
              { value: "a", label: "自动选择" },
              { value: "b", label: "手动指定" },
            ]}
          />
          <Select
            label="禁用的下拉"
            value="a"
            onValueChange={() => undefined}
            state="disabled"
            options={[{ value: "a", label: "不可选" }]}
          />
        </div>
      </Panel>

      <Panel title="③ 选择 · Checkbox · Radio · Switch">
        <div className="gallery__row">
          <Checkbox label="启用" checked={checked} onCheckedChange={setChecked} />
          <Checkbox label="禁用" checked={false} onCheckedChange={() => undefined} state="disabled" />
          <Switch label="开关" checked={sw} onCheckedChange={setSw} />
          <Switch label="禁用的开关" checked={false} onCheckedChange={() => undefined} state="disabled" />
        </div>
        <Radio
          label="账户类型"
          value={radio}
          onValueChange={setRadio}
          options={[
            { value: "a", label: "离线" },
            { value: "b", label: "统一通行证" },
          ]}
        />
      </Panel>

      <Panel title="④ 容器 · Panel · Card">
        <div className="gallery__grid">
          <Card>这是一张卡片（surface.card，比面板远一档）</Card>
          <Card>另一张卡片</Card>
        </div>
      </Panel>

      <Panel title="⑤ 浮层 · Dialog · Popover · Tooltip · Toast">
        <div className="gallery__row">
          <Dialog
            title="确认删除这个实例？"
            description="它里面的存档与配置会一起被删掉。"
            trigger={<Button tone="danger">打开对话框</Button>}
          >
            <p>这里的浮层用的是**不透明**底色（`surface.raised`）—— 见 §5.3 使用纪律 2。</p>
          </Dialog>
          <Popover trigger={<Button>打开浮层</Button>}>
            <p style={{ margin: 0 }}>浮层内容</p>
          </Popover>
          <Tooltip label="这是一条提示">
            <Button>悬停我</Button>
          </Tooltip>
        </div>
        <div className="gallery__col">
          <Toast tone="info" title="信息" detail="这是一条信息提示" />
          <Toast tone="success" title="成功" detail="部署完成" onDismiss={() => undefined} />
          <Toast tone="warning" title="警告" detail="这个实例没有备份" />
          <Toast tone="danger" title="失败" detail="下载被中断" onDismiss={() => undefined} />
        </div>
      </Panel>

      <Panel title="⑥ 反馈 · Progress（线性/环形）· EmptyState">
        <div className="gallery__col">
          <Progress value={62} label="下载中 62%" />
          <Progress value={null} label="探测中（**不确定** —— 它必须与 0% 看起来不同）" />
          <div className="gallery__row">
            <RingProgress value={62} label="环形进度" />
            <RingProgress value={null} label="不确定" />
          </div>
        </div>
        <EmptyState
          title="还没有实例"
          detail="实例是一个独立的游戏目录；建一个之后它就会出现在侧栏里。"
          action={<Button tone="primary">新建实例</Button>}
        />
      </Panel>

      <Panel title="⑦ 布局 · Tabs · Sidebar">
        <Tabs
          items={[
            { key: "a", label: "概览", content: <p>概览内容</p> },
            { key: "b", label: "日志", content: <p>日志内容</p> },
            { key: "c", label: "设置", content: <p>设置内容</p> },
          ]}
        />
        <div className="gallery__sidebarDemo">
          <Sidebar label="示例侧栏">
            <ul className="shell__group">
              <li className="shell__railItem">项目一</li>
              <li className="shell__railItem shell__railItem--on">项目二（选中）</li>
            </ul>
          </Sidebar>
          <Card>侧栏只提供布局与 a11y —— 导航数据由 `nav.ts` 给。</Card>
        </div>
      </Panel>
    </div>
  );
}
