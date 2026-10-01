# 上次收工（2026-10-01 · U0 完成）

## 上次做到哪

**U0 · 环境就绪 —— 已完成并验收通过**。仓库从"只有文档"变成"能跑测试的骨架"。

**工具链（全部就绪）**：Rust 1.98.1（MSVC，含 clippy/rustfmt）· Node 24.9 · pnpm 10.34.5 ·
JDK **8 / 17 / 21 三套齐全** + 额外 11/25 · MSVC 14.51 + Windows SDK 10.0.26100 · WebView2 154.0.4258.37。

**U0 验收结果**：

| 项 | 结果 |
|---|---|
| `cargo test --workspace` | ✅ **16 项通过**（10 功能 + 6 架构约束） |
| `cargo clippy -D warnings` | ✅ 0 警告 |
| `cargo fmt --check` | ✅ 通过 |
| `pnpm typecheck` | ✅ 通过 |
| `pnpm lint` | ✅ 0 报错（含 3 条纪律规则） |
| `pnpm test` | ✅ **12 项通过** |

**落地的东西**（不是空壳，都是方案里已定死的契约）：

- `crates/qul-core/src/caps.rs` —— **能力描述符**（方案 §3.3）。禁用态**无法不带原因**：类型系统挡住了，
  且 JSON 反序列化也补了校验（`CapabilityWire`），因为"JSON 不认类型"。
- `crates/qul-core/src/layout.rs` —— **三层目录 + 相对路径**（方案 §5.5）。含 `..` / 绝对路径 / 盘符拦截。
- `crates/qul-core/tests/architecture.rs` —— **6 条架构约束**，把纪律变成断言。
- `src/` —— 前端骨架 + **契约校验**（`assertCapabilitiesValid`，与 Rust 侧同一规则的边界兜底）。
- `eslint.config.js` —— **3 条纪律规则**：禁字面颜色值 / **禁按产品名分支** / 禁 toggle 布尔状态。
- `.github/workflows/verify.yml` —— CI 三个 job（rust / web / guard）。

## 下一步的第一件事

**做 U1 = S1 尖刺：材质 4×2 矩阵。**

1. 新建 `spikes/s1-material/`，放一个最小 Tauri 2 应用（`cargo new` + `tauri init` 即可）
2. 遍历 **`{Mica, Acrylic}` × `{有边框 decorations:true, 无边框 decorations:false}`** 四格，
   加上**浅色主题**与**纯色兜底**，凑满 8 张截图
3. 每格记录：用的是哪个 API（`DWMWA_SYSTEMBACKDROP_TYPE` 还是 `SetWindowCompositionAttribute`）、
   材质是否真的生效、有没有黑边/白边
4. 产出：8 张截图 + 一张 API 记录表 → **这是 U1 的交付物，也是全项目最早能看到东西的时刻**

**结论出来后对号入座**：`docs/UI设计规格.md` **§11.2** 已写死 A / B1 / B2 三档预案，
**不需要临场决策**。若结果是 B1 或 B2，§4 的布局设计要按那一档改（这是唯一被 S1 阻塞的东西）。

## 卡住的 / 待查的

- **git 身份未设置**（`user.name` / `user.email` 均空），**所以还没有首次提交**。
  待用户给出身份后执行 `git config user.name/user.email`（仓库级即可，不必动全局）。
- **Minecraft 未安装**（`%APPDATA%\.minecraft` 不存在）→ S10「干净环境」与 S11「官方能力基线」**暂不可做**（属 U11）。
- **代理环境**：系统只有 PAC（`http://127.0.0.1/pac.txt?S302`），`ProxyEnable=0`。
  Node/pnpm **不走 PAC**，实测直连 npmjs 对部分包慢到不可用（单请求 240 秒）→ 已用 `.npmrc` 固定镜像源解决。
  **这条以后装任何 Node 依赖都会用到。**

## 这次决定的（避免下次重新纠结）

- **`.npmrc` 固定 `registry.npmmirror.com` 并入库**。理由：直连 npmjs 实测不可用（不是配置错，是链路），
  而写进项目级配置能让本机/CI/以后任何机器行为一致、可评审、可回滚。**镜像不影响 lockfile 的 sha512 校验。**
- **pnpm 的构建脚本白名单只放行 `esbuild`**（写进 `package.json` 的 `pnpm.onlyBuiltDependencies`）。
  **保留白名单机制本身**——将来引入带 postinstall 的依赖要逐个评审，不图省事关掉整个限制。
- **vitest 必须与 vite 同代**（vite 6 → vitest 3）。vitest 2 会拖进 vite 5，造成两套类型冲突。
- **U0 只做"能跑通"，但**不造空壳**：落的都是方案已定死的契约（能力描述符 / 三层目录 / 路径安全）。
  **理由**：这些契约的测试本身就是 U0 验收的一部分，而空壳的测试没有意义。
- **架构测试的禁用词表要精确到"断言的对象"**。本次两处误报（`shader` 被当成游戏词汇、
  `零 Tauri` 注释被当成依赖）都源于"扫全文"而非"只看该看的东西"——
  **改法是收窄断言对象，不是放宽规则**。

## 候选（想到但**未批准开工**的活）

- `.editorconfig` / `rustfmt.toml` / `clippy.toml` —— 目前用默认配置已能通过 CI，**建议并入 U1 顺手做**。
- `docs/待决.md` 与 `docs/任务清单.md` —— M0 阶段用本文件 + 任务书原文已够，**留到 M1 再建**。
- `repos/` 的只读性需要"防误改"保障（现在是靠自觉 + `.gitignore`）——
  可考虑加只读属性或提交前检查，**但 M0 阶段收益低，记为候选**。
