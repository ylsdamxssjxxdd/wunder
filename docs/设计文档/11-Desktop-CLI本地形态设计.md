# Desktop / CLI 本地形态设计

> 本册于 2026-09-25 按 0.4.x 现状重写：桌面默认形态为 `frontend-slint/`（Rust + Slint）同进程原生架构；遗留 WebView 前端壳已移除。

## 1. 设计目标

Desktop 与 CLI 是 wunder 的两种本地运行形态。它们的意义不是"给 server 再包两层壳"，而是在复用同一核心语义的前提下，为个人用户和自动化场景提供不同的人机接口。

## 2. 形态定位

| 形态 | 面向对象 | 设计重点 |
| --- | --- | --- |
| Desktop（Slint 原生） | 个人用户（wunder 主推形态） | 易用、低门槛、低资源占用、可分发、Win7 x86 兼容 |
| CLI | 开发者、自动化和脚本场景 | 可脚本化、可批处理、可纳入流水线 |

二者都必须复用 Wunder 的线程运行时、工具治理和实时投影语义，不能因为本地形态而单独生长出另一套智能体系统。

## 3. 模块组成

| 模块 | 作用 | 主要目录 |
| --- | --- | --- |
| Slint 前端 | 桌面默认用户界面（Rust + Slint） | `frontend-slint/`（`ui/` 界面、`src/` 接入与 UI 状态投影） |
| 桌面本地运行时 | 原生桌面运行时、bridge、系统能力与启动装配 | `crates/wunder-desktop` |
| NativeDesktop | 无参数启动时的同进程原生调用入口，不经本机 HTTP/WS、不拉起 bridge 子进程 | `crates/wunder-desktop::NativeDesktop`（`native.rs`、`native_catalog.rs` 等） |
| 遗留 bridge | 仅为已有兼容客户端保留的独立桥接入口 | `crates/wunder-desktop/src/bridge.rs`、`crates/wunder-runtime/src/api/desktop.rs` `desktop_lan.rs` |
| CLI 程序 | 命令行入口与 TUI | `crates/wunder-cli` |
| 构建入口 | 构建、交叉编译与打包统一入口 | `builders/` |

## 4. Desktop 架构

Desktop 当前稳定为"Slint 前端 + 进程内原生运行时 + 本地 SQLite"的组合：

- 无参数启动即在同一进程内通过 `NativeDesktop` 原生调用后端，不经本机 HTTP/WebSocket，不拉起 bridge 子进程。
- 桌面页面新增能力使用强类型 Rust façade 暴露给 Slint；本地系统能力放 `crates/wunder-desktop/`。
- 执行、工具与存储语义复用 runtime 核心，不另造平行运行时。
- 数据库采用 SQLite；本地路径、备份、迁移与并发访问保守设计，避免长事务阻塞 UI。
- 桌面控制、浏览器控制、工作区与本地文件能力通过授权后接入。
- 独立 bridge 仅服务已有兼容客户端，不影响原生桌面后端的持续维护。

### 4.1 渲染与流式约束

- 保持原浅色蜂巢基调，不引入 WebView、浏览器渲染链或重型组件；Slint 以软件渲染与完整内嵌字体保证 Win7 x86 兼容。
- Markdown 采用最低必要能力：纯文本保底，逐步支持段落、列表和代码块；复杂内容降级为可读文本或占位提示，原始消息与复制内容必须完整保留。
- 流式输出优先复用后端既有 WebSocket 事件、去重、replay 与补水语义；按帧合并 token 增量（初始约 16–33 ms 刷新节奏），事件缓冲必须有界，合并文本增量不得丢字。
- 长消息按块组织，已完成块保持稳定，仅更新活动尾块；历史消息使用虚拟列表与限量加载，用户停留底部时才自动跟随输出。
- 网络读取、事件处理与较重解析放在后台，UI 线程只应用必要增量投影。

## 5. CLI 架构

CLI 的长期定位是"同一运行时的脚本化入口"：

- 复用同一线程、tooling、prompt、recovery 语义（`crates/wunder-cli/`）。
- 支持终端流式输出、批处理和自动化管线集成。
- 默认本地 SQLite，考虑离线、启动速度、路径权限与配置迁移。
- CLI 不为了终端交互便利绕开主运行时或重写一套私有流式协议。
- Win7 兼容构建走 GNU 工具链，见 `docs/方案/wunder-cli-win7-gnu构建SOP.md`。

## 6. 设计原则

本地形态必须遵守以下原则：

- 本地形态与服务端共享同一核心语义。
- 本地接口面可以裁剪，但线程、mission、tool、projection 语义不能裁剪错位。
- Desktop 与 CLI 的差异只应体现在壳层、交互层和本地权限接入层。
- 本地模式仍需保留恢复、日志、工作区和审计能力。
- 本地 bridge、LAN overlay、系统能力调用要最小权限、显式能力声明、可观测错误，不把系统能力默默暴露给远端。

## 7. 与其他系统的关系

- 与用户侧前端：`frontend-slint/` 是独立的 Slint 界面体系，以清晰可用为准，不要求完整复刻 TS 端富文本与视觉细节；`frontend/`（Vue3）仅服务端网页版。
- 与存储系统：Desktop 使用 SQLite，但数据模型与服务端共享抽象。
- 与工具系统：本地文件、桌面控制、浏览器控制等能力更强，但仍受统一治理。
- 与实时投影：Desktop 和 CLI 都消费统一投影语义，而不是私有事件结构。

## 8. 当前演进重点

- Win7 x86 与 Linux（Ubuntu 18.04 x86_64 / ARM64 AppImage）分发链维护，构建入口统一在 `builders/`。
- Slint 桌面能力对齐：智能体管理、设置、文件浏览、头像与个人概况等页面继续以强类型 façade 原生接入。
- 流式验收覆盖长消息、连续增量、输出期间输入/滚动、断线恢复与最终文本完整性，并记录 UI 更新耗时、积压和内存表现。

## 9. 验收标准

- Desktop 与 CLI 能在不依赖完整 server 的情况下独立运行。
- 二者消费的是同一套线程与实时语义，而不是私有变体。
- 本地模式依然具备工作区、恢复、工具治理和日志能力。
- 壳层与业务层职责清晰，不把业务逻辑写进打包壳中。
- Slint 渲染改动需查看实际截图并通过 Win7 真机运行验收，构建通过不能代替运行验收。

## 10. 相关文档

- `docs/设计文档/01-系统总体设计.md`
- `docs/技术说明书/08-部署、启动与运行模式.md`
- `docs/使用说明书/zh-CN/start/desktop.md`
- `docs/方案/wunder-cli-win7-gnu构建SOP.md`
