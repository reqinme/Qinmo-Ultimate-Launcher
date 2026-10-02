/**
 * 「下载」—— 一级页面的骨架（§4.6.7）
 * ============================================================================
 *
 * ## 🔴 这一页是**六个分类的目录**，不是一张空表
 *
 * 「下载」下面的六件事各有自己的页面（§4.6.1.1 的二级分区），
 * 而**一级页**要做的是把六件事摆出来、说清每件是干什么的。
 * 所以这里不长成"清单 + 空态"，而是一张**卡片目录** ——
 * 而那六张卡片**现在都点得动**（路由是通的，每张自己会说它还缺什么）。
 *
 * ⚠️ 卡片的角标（`page__cardBadge`）**是这一页最重要的一列**：
 * 六个分类今天**没有一个能真的下载**，而"看起来能点"与"能下到东西"
 * 是两件事 —— 一个不标注的实现会让用户以为源挂了。
 */

import { Link } from "@tanstack/react-router";
import type { ReactElement } from "react";
import "./pages.css";

/**
 * 六个分类。
 *
 * ⚠️ `to` 是**类型安全的字面量**：拼错一个路径在这里是编译错误
 *（`router.tsx` 的 `declare module` 就是为这件事存在的）。
 */
const CATEGORIES = [
  {
    to: "/downloads/versions",
    label: "游戏版本",
    note: "正式版 · 快照 · 旧版 —— 官方源清单里的版本",
  },
  {
    to: "/downloads/loaders",
    label: "加载器",
    note: "Fabric / NeoForge / Forge / Quilt，以及它们与版本、运行时的兼容",
  },
  {
    to: "/downloads/mods",
    label: "模组",
    note: "Modrinth 优先，CurseForge 作回退",
  },
  {
    to: "/downloads/resourcepacks",
    label: "资源包",
    note: "装进实例的 resourcepacks 目录",
  },
  {
    to: "/downloads/shaders",
    label: "光影",
    note: "装进 shaderpacks 目录 —— 需要时一并说明前置",
  },
  {
    to: "/downloads/worlds",
    label: "存档",
    note: "导入 / 导出实例的 saves 目录",
  },
] as const;

export function DownloadsPage(): ReactElement {
  return (
    <section>
      <h1 className="page__title">下载</h1>
      <p className="page__note">
        六个分类各有自己的页面；二级栏就在左边。源的选择（官方源优先、镜像作回退）
        是全局的一件事，不在这一页里重复。
      </p>
      <ul className="page__list">
        {CATEGORIES.map((category) => (
          <li key={category.to}>
            <Link className="page__card" to={category.to}>
              <span className="page__cardTitle">
                {category.label}
                <span className="page__cardBadge">清单未接</span>
              </span>
              <span className="page__cardNote">{category.note}</span>
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
