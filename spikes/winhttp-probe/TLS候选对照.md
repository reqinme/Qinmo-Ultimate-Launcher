# TLS 后端的三个候选：**实测对照**

> **这份文件的每一条都是实测的，不是从文档抄的。**
> 三个候选的许可证来自**本地已解开的 crate 目录里的 LICENSE 文件**，
> 依赖树来自本机 `cargo tree --offline`，而候选 3 是**真的跑通了从
> `piston-meta.mojang.com` 下载**。

---

## 0. 起因：一个必须记下来的事实

> ### **「下载引擎」从没联过网。**

而工作区 `Cargo.lock` 现在**只有 18 个包**：

```
qul-app · qul-cli · qul-core · qul-infra · qul-provider-mock
serde · serde_core · serde_derive · serde_json
itoa · memchr · proc-macro2 · quote · syn · thiserror · thiserror-impl
unicode-ident · zmij
```

**零网络依赖。** 而官方端点全是 `https://` —— 所以引 TLS 是**做不下去就无法继续**的事。

---

## 1. 三个候选的实测对照

| | **A · rustls + ring** | **B · native-tls（Windows → schannel）** | **C · WinHTTP FFI** |
|---|---|---|---|
| **新增 crate 数** | **10** | **4** | **0** |
| 新增的是哪些 | `rustls` `rustls-pki-types` `rustls-webpki` `ring` `untrusted` `subtle` `zeroize` `once_cell` `getrandom` `cfg-if` + 构建期 `cc` `shlex` `find-msvc-tools` | `native-tls` `schannel` `windows-link` `windows-sys` | — |
| **要不要 C 工具链** | **要**（`ring` 编译 C 与汇编；`cc` 进树就是证据） | 不要 | 不要 |
| **TLS 由谁实现** | 自己（`ring` 的 BoringSSL 派生） | **操作系统**（SChannel） | **操作系统**（WinHTTP） |
| **许可证** | `rustls` Apache-2.0 OR ISC OR MIT · `rustls-webpki` **ISC** · `ring` **Apache-2.0 AND ISC** · `rustls-pki-types` MIT OR Apache-2.0 | `native-tls` MIT OR Apache-2.0 · `schannel` **MIT** · `windows-*` MIT OR Apache-2.0 | **无变化**（系统 DLL） |
| **实测结论** | 依赖树最重 | 轻，且用系统栈 | **零依赖，且实测走通** |

### 而 A 有一个**实测才发现的坑**

`rustls` 0.23 的**默认后端是 `aws-lc-rs`**，而它**不在本机缓存里**：

```
error: no matching package named `aws-lc-rs` found
  required by package `rustls v0.23.45`
```

也就是说**"加一个 rustls"在我这台机器上第一步就失败**，除非显式关掉默认 features、
改用 `ring`。而那意味着**我们不能用它的默认配置** —— 一件必须写下来的事。

### 许可证：**我读的是本地解开的 LICENSE 文件，不是文档**

| crate | `license` 字段 | 目录里的文件 | 实际内容 |
|---|---|---|---|
| `native-tls` 0.2.18 | `MIT OR Apache-2.0` | LICENSE-APACHE, LICENSE-MIT | 标准 |
| `schannel` 0.1.29 | **`MIT`** | LICENSE.md | 标准 |
| `rustls` 0.23.45 | `Apache-2.0 OR ISC OR MIT` | 三份 | 标准 |
| `rustls-webpki` 0.103.15 | **`ISC`** | LICENSE | 标准 |
| `rustls-pki-types` 1.15.1 | `MIT OR Apache-2.0` | 两份 | 标准 |
| `ring` 0.17.14 | **`Apache-2.0 AND ISC`** | LICENSE, LICENSE-BoringSSL, LICENSE-other-bits | 见下 |

**`ring` 那一行值得单独说**，因为它的 `license` 字段带了 `AND`，
而那通常意味着"两个都要满足"：

```
LICENSE               → "*ring* uses an ISC license … See LICENSE-other-bits"
LICENSE-other-bits    → 标准 ISC 文本（"Permission to use, copy, modify,
                        and/or distribute this software for any purpose with
                        or without fee is hereby granted…"）
LICENSE-BoringSSL     → 标准 Apache License 2.0
另有                  → src/polyfill/once_cell/LICENSE-{APACHE,MIT}
```

**全部是宽松许可，没有 AGPL 那类。**
而 `AND` 在这里的实际含义是"不同文件不同许可"（新代码 ISC、BoringSSL 派生部分 Apache-2.0），
**不是"同一个文件要同时满足两个"**。

---

## 2. 候选 C 的实测证据（**两组探针，都跑通了**）

### 第一组：https 能不能走通

```
✓ WinHttpOpen
✓ WinHttpConnect piston-meta.mojang.com:443
✓ WinHttpOpenRequest（SECURE 标志 = 走 TLS）
✓ 收到响应 —— **https 握手成功**
✓ 状态码 = 200
  Content-Length = 277187（✓ 读到了）
✓ 分段读 body：共 277187 字节
  开头：{"latest": {"release": "26.3", "snapshot": "26.4-snapshot-2"}, "
```

### 第二组：**续传所依赖的三件事**（它决定这条路能不能用）

```
【①】按名字查响应头
  ETag = 0x8DF1E31C78D55DA
  Last-Modified = Tue, 29 Sep 2026 13:58:46 GMT
  Accept-Ranges = bytes
  Content-Length = 277187
【②】带 Range 的请求
  ✓ 状态码 = 206 Partial Content
  Content-Range = bytes 100000-100999/277187
  ✓ 实际读回 1000 字节（期望 1000）
```

**"能不能走 https"不等于"能不能支持续传"** —— 所以第二组是必要的。
而下载引擎的 I5 完全建立在 `Accept-Ranges` / `ETag` / `Content-Range` 之上。

### 🔴 而我在探针里犯了**两个都差点写错结论**的错

| # | 我写的 | 真相 | 若没核对会写下的错结论 |
|---|---|---|---|
| ① | `Content-Length = 27718` | **277187**（WinHTTP 的长度**不含**结尾 NUL，我多减了 1） | — |
| ② | `Range: 1000000-1000999` | 那文件**只有 277187 字节** ⇒ **416 是正确答案** | **"服务端不支持 Range"** |
| ③ | `ETag = 0x8DF1E31C78D55D` | `…DDA`（同一个"减 1"的错） | — |

**②是三者里最危险的**：它会把"服务端不支持续传"写进结论，
而真因是我自己填了一个超界的偏移。

> **两次都是同一个模式**：一个**看起来像"能力缺失"**的现象，
> 真因却是**我这边的输入错了**。
> 而判别的方法很简单：**去核对服务器自己给的那个数字**。

---

## 3. 建议

| | |
|---|---|
| **我建议 C（WinHTTP FFI）** | **零新依赖、零 C 工具链、零许可证变化**，且**实测走通**（含 206 与 `Content-Range`）。它把"TLS 从哪来"这件事变成一个**不需要维护的选择** —— 系统的 TLS 栈由 Windows 更新来维护。 |
| **代价** | 300+ 行 FFI，且**只支持 Windows** —— 而方案 §7 的预算与 §11.5 的端点清单**本来就只针对 Windows**（Tauri 2 的 Windows 目标是唯一目标平台）。 |
| **B 作为备选** | 只要 4 个 crate，且**用系统栈**。如果"300 行 FFI 太重"是更重要的考虑，它是最接近 C 的方案。 |
| **A 我不建议** | 依赖树最重、要 C 工具链、且**默认配置在这台机器上直接失败**（缺 `aws-lc-rs`）。许可证虽然都合格，但它把一棵加密库的树变成了我们要长期跟进维护的东西。 |

### 无论选哪个，有一件事是确定的

**这个选择必须进 `docs/来源记录.md` 的台账**（这是本项目"每个依赖都要有登记"的纪律）。
若选 C，台账里要写的是"**零新增依赖，用的是 Windows 自带的 WinHTTP**" ——
而那本身是一条值得记下来的设计决定。

---

## 4. 取证产物

| 文件 | 说明 |
|---|---|
| `spikes/winhttp-probe/winhttp-probe.rs` | https 可行性探针（第一组） |
| `spikes/winhttp-probe/winhttp-resume-probe.rs` | **续传三件套**探针（第二组，含那两处自我纠正的注释） |

**运行**：

```powershell
rustc --edition 2021 -O spikes/winhttp-probe/winhttp-resume-probe.rs -o probe.exe
./probe.exe
```

**两者都不引任何 crate** —— 那正是候选 C 的要点。
