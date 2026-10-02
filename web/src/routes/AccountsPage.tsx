/**
 * 「账号管理」—— 一级页面的骨架（§4.6.5）
 * ============================================================================
 *
 * ## 🔴 这一页今天**一个账户都不显示**，而那不是"空"
 *
 * 规格 §4.6.5 把**账户层与授权层拆开**（账户 = "这台机器上有哪些身份"，
 * 授权 = "那个身份现在能不能用"），而两层都还没有落地。
 *
 * ⚠️ 而有一个**看起来很像账户**的东西存在：灵动岛里那行「离线账户」
 *（`web/src/island/Island.tsx` 的 `idleContent(account, source)`）。
 * 那是**一句占位文案**，不是一份账户数据 —— 一个把它搬进这一页的实现
 * 会让界面**看起来已经登录了**。
 *
 * 所以这一页写的是"账户层还没落地"这件事本身，以及它落地后长什么样。
 */

import { Link } from "@tanstack/react-router";
import type { ReactElement } from "react";
import { PageState } from "../components/PageState.tsx";
import "./pages.css";

export function AccountsPage(): ReactElement {
  return (
    <section>
      <h1 className="page__title">账号管理</h1>
      <p className="page__note">
        账户层回答「这台机器上有哪些身份」，授权层回答「那个身份现在能不能用」——
        两层分开，因为一个账户可以同时有可用的与过期的授权。
      </p>
      <PageState
        kind="empty"
        title="账户层还没有落地"
        body="今天没有任何地方存账户，也没有「添加账户」那条命令。灵动岛里那行「离线账户」是一句占位文案，不是一份账户数据 —— 所以这一页不把它搬过来假装已经登录了。"
        action={
          <Link className="page__action" to="/toolbox/logs">
            去日志页看内核现在报什么
          </Link>
        }
      />
    </section>
  );
}
