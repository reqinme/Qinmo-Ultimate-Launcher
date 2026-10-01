import type { ReactElement } from "react";

/**
 * 占位页。
 *
 * ## 它不是"还没写"，它是**门禁③ 的交付物本身**
 *
 * §8 对 M4 门禁第 ③ 项的要求是「两段式导航 + 百宝箱二级分组的
 * **可跑通空壳**」—— "可跑通"指的是**能点、能跳、能后退、URL 是对的**，
 * 而不是"内容填满了"。
 *
 * ## ⚠️ 而它刻意**不画**内容的样子
 *
 * 一个"用灰条假装有内容"的占位（骨架屏）会让评审者以为那些布局已经定了。
 * 而 §2 的纪律是「**形式类似 ≠ 一模一样**」—— 参考图只回答"长什么样"，
 * 不回答"应该有什么"。
 *
 * 所以这里只写**这一页将是什么**，而不假装它已经是什么。
 */
export function PagePlaceholder({
  title,
  note,
}: {
  readonly title: string;
  readonly note: string;
}): ReactElement {
  return (
    <section>
      <h1 className="page__title">{title}</h1>
      {note === "" ? null : <p className="page__note">{note}</p>}
      <p className="page__stub">
        骨架占位 —— 路由已通（URL、前进后退、二级栏归属都由路由决定）。
        内容与交互在 M4 主干 / 相应里程碑里落地。
      </p>
    </section>
  );
}
