# Qinmo Ultimate Launcher (QUL)

Windows 桌面 Minecraft 启动器。目标形态：**单文件、免安装、双击秒开、体积极小**。

| 项 | 值 |
|---|---|
| 技术方向 | C# + WPF + **net48**（.NET Framework 4.8，Win10/11 自带，无需用户另装运行时） |
| 根命名空间 | `Qul` |
| 程序集 | `QinmoUltimateLauncher.exe` |
| 错误码前缀 | `QUL-` |
| 当前进度 | P0 地基与规范 **已完成**；P1 版本元数据 进行中 |

完整规划见 [docs/Minecraft启动器-架构规划与路线图-v1.2.md](docs/Minecraft启动器-架构规划与路线图-v1.2.md)。

---

## 合规边界

本项目是**编排器**：只做解析、获取、组装、拉起，不实现游戏逻辑，也不分发任何官方游戏内容。

**永不实现**（写进设计红线，不接受"以后再说"）：

- 不伪造 Mojang 官方会话
- 不劫持官方验证接口
- 不实现破解登录
- 不逆向混淆代码以复制其逻辑
- 不分发游戏本体、客户端 jar、官方资源与库
- 不内置来源不明的整合包
- 不冒用官方品牌

**离线账户**作为合法身份来源存在，但其"无法进入正版验证服务器"的限制会在每次启动前被强制告知，且其 UUID 仅作本地身份标识，不冒充官方 UUID。

所有可选能力（JRE 自动下载、镜像源、第三方验证）一律遵循：**默认关闭、用户显式启用、能力与限制明示**。

---

## 安全基线

| 项 | 做法 |
|---|---|
| 令牌存储 | 系统凭据保护（DPAPI）加密，密文只落 `data/secrets/`；明文不落盘 |
| 配置文件 | 永不出现任何凭据字段 |
| 启动计划 | 拆为「可复现骨架」与「运行时秘密引用」两平面；逐字节比对只针对骨架 |
| 日志与诊断导出 | 统一脱敏管道：令牌、UUID、用户名、路径、IP、服务器地址一律折叠为占位符 |
| 解压 | 强制 zip slip 与路径穿越防护，越界条目整包拒绝且不落盘 |
| 证书 | 证书异常明确分类提示，**绝不静默忽略或降级绕过** |
| 失败处理 | 任一失败都有唯一错误码 + 人话提示；配置损坏降级不崩 |

---

## 仓库结构

```
QinmoUltimateLauncher.slnx          解决方案
docs/                               规划与阶段规范文档
src/QinmoUltimateLauncher/          主程序（net48 / WPF / WinExe）
  Domain/                           领域层：纯模型与规则，无任何外部依赖
  Application/                      应用层：用例编排
  Infrastructure/                   基础设施层：JSON / 文件系统 / 配置 / 日志 / 网络 / 进程
  Presentation/                     表示层：视图与视图模型
tests/Qul.Tests/                    测试工程（不随产品分发）
  Golden/                           真实官方元数据样本，供离线回归使用
```

**分层由测试强制**：`tests/Qul.Tests/Architecture/LayerDependencyTests.cs` 通过反射守住依赖方向——领域层一旦引用 WPF、文件系统、网络或进程 API 就会红。

`bin/`、`obj/`、`publish/`、`*.log` 一律排除，不检索、不统计。

---

## 构建与测试

```powershell
# 一次带上全部检查（编译 + 全部单元测试）
dotnet test QinmoUltimateLauncher.slnx

# 发布单文件产物
dotnet publish src\QinmoUltimateLauncher\QinmoUltimateLauncher.csproj -c Release
```

net48 下 `dotnet publish` 不支持 `PublishSingleFile`，产物形态为 **单 exe + 一纸 `.exe.config`**（PCL2 同路线下的客观形态）。当前零第三方依赖，因此单文件是零依赖架构的自然结果，而非靠合并技术达成。

---

## 关于 `tests/Qul.Tests/Golden/`

该目录存放的是**未经修改的 Mojang 公开发布的版本元数据**（版本清单与版本详情 JSON），仅作为解析器的回归样本。它们不含任何游戏二进制内容，不适用本项目的源代码许可证。

样本覆盖 1.7.10 / 1.12.2 / 1.16.5 / 当前正式版四个代表性版本，用来锁死跨年代的结构差异——包括两代 natives 机制、新旧两套参数写法、以及官方元数据自身的若干缺陷。

---

## 许可证

[GPL-3.0](LICENSE)
