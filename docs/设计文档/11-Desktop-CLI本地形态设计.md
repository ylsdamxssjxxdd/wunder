# Desktop / CLI 本地形态设计

> 约束范围：本册约束蛹与舵机两种本地形态的进程模型、启动与恢复语义、目标平台约束、本地文件安全与最小权限能力声明。分发命令、构建矩阵与配置键见 `../技术说明书/08-部署、启动与运行模式.md`，本册不复述。

## 1. 形态定位（不变量）

蛹与舵机不是舰体的壳，而是与舰体平级的两种可执行形态，共享 `wunder-core → wunder-runtime` 同一引擎：

| 形态 | 定位 | 进程模型 | 存储 |
| --- | --- | --- | --- |
| 蛹（desktop） | 本地 AI 工作台、个人用户默认入口 | 桌面进程内同进程调用引擎 | 本地 SQLite |
| 舵机（cli） | 手工本地操作入口、终端驱动与脚本化 | 本地进程，TTY/非 TTY 双模式 | 本地 SQLite |

执行、工具与存储语义完全复用引擎，**不另起一套执行链路**。Electron / Tauri 前端壳停止日常维护与功能迭代；「停止维护前端壳」不等于「停止维护蛹后端」。

## 2. 蛹：同进程原生是唯一默认路径

| 事实 | 依据 |
| --- | --- |
| 无参数启动即 `NativeDesktop::start()` → `DesktopArgs::native_defaults()`（`port=0, bridge_only=true, native_runtime=true`），自建 tokio Runtime 并 `DesktopRuntime::init` 同进程 | `crates/wunder-desktop/args.rs:53-67`（已实测） |
| native 路径无 axum、无 TcpListener：不经本机 HTTP/WS，不拉起 bridge 子进程 | `crates/wunder-desktop/native.rs` |
| Slint 入口只接受 `None | --native`，其余参数直接报「旧 bridge 参数已移除」 | `frontend-slint/src/main.rs:123-126` |
| 本地能力放 `crates/wunder-desktop/`，以强类型 Rust façade 暴露（`lib.rs` 公开 `args/native/runtime` 三模块 + `NativeDesktop`、事件流、设置等），façade 与 UI 调用点一一对应 | `wunder-desktop/lib.rs:6-24` |

**bridge 只是兼容层**：独立 bridge（默认端口 18123、挂 `build_desktop_router`）仅为既有客户端保留；`lib.rs` 不导出 bridge，只有 bridge bin 才组装它。新能力不得再往 bridge 协议面加。

### 2.1 启动与恢复语义

本地 SQLite 必须假设「进程可能被硬杀」：

- 启动恢复扫描限定数量上限（4096），把 queued/waiting/running/cancelling 的残留会话清理为一致状态，回收会话锁与任务（`services/desktop_runtime_recovery.rs`，已实测）。
- 蛹与舵机不依赖舰体即可运行；互通隧道仅在存在云会话且会话 `interlink.enabled` 开关联动时启动，策略变更即拆隧道。
- 数据目录收敛在 `WUNDER_HOME` → `~/.wunder`（工作区、temp、设置各有子路径），首启动不要求网络。

## 3. 舵机：双模式与脚本契约

| 模式 | 触发 | 语义 |
| --- | --- | --- |
| TUI |  stdin/stdout/stderr 三端均为 TTY（`main.rs:359`，已实测） | ratatui Inline 视口，slash 命令与键位是交互面 |
| exec | 非 TTY | 一次性执行降级，管道友好 |

不变量：

- 舵机同进程调引擎（`CliRuntime.state: Arc<AppState>` → orchestrator stream），不为终端体验重写私有流式协议。
- `exec --json` 的 JSONL（thread/turn/item.* 事件）是**对外脚本契约**，破坏兼容即破坏用户脚本，改动需走契约测试。
- 舵机本地形态显式关闭定时任务（`runtime.rs:683 config.cron.enabled = false`）：定时执行需要常驻进程，与终端形态冲突，宁可关闭也不做隐式后台常驻。
- `cloud` 子命令族与互通隧道（`interlink_tunnel.rs`、`cloud_interlink.rs`）是舵机独有的云端接入面，走 `07` 册的互通协议，不开任何本地监听。

## 4. 目标平台约束

蛹与舵机面向 **32 位 Win7** 与 **Ubuntu 18.04**（x86_64 / ARM64）：

| 约束 | 设计后果 |
| --- | --- |
| Win7 x86：GNU 工具链交叉构建、Slint 软件渲染 + 字体完整内嵌 | 不依赖 D3D/WebView；GUI 栈 API 白名单 |
| Win7 控制台 GBK 输出 | 命令会话 winpty 路径按平台解码（`tools/command_sessions/desktop.rs`） |
| Ubuntu 18.04 老 glibc | sysroot 锁定构建，产物目录区分 `*-ubuntu18` |
| win7 构建的互通节点**不声明 `tool.exec`** | 能力声明跟随构建事实（`07` 册第 4 节），不假装支持 |
| 路径与编码 | canonicalize 与 Windows verbatim/大小写归一在 `path_utils.rs` 统一处理 |

32 位编译链只用于分发与 PE 验收，不作为日常正确性或性能基线。

## 5. 本地文件安全与最小权限

本地形态默认离线可用、数据不出机器；对系统能力的调用遵循**最小权限、显式能力声明、可观测错误**：

| 面 | 纪律 |
| --- | --- |
| 工作区与配置 | 本地 SQLite 与配置文件受文件系统权限保护；路径、备份与迁移保守设计；不把系统能力默默暴露给远端 |
| LAN overlay | 仅绑私网 CIDR、peer TTL 有界、去重表有界；能力声明当前**只有 `user_world`**（`services/desktop_lan.rs:521` announce 载荷，已实测）——LAN 上的对端只能交换站内消息域内容，拿不到文件、命令或线程能力 |
| 本地 bridge | 最小权限、显式声明、错误可观测；不承载新能力 |
| 桌面工具 | 桌面控制/监视需 desktop 模式 + 显式开关；vision 不可用时过滤读图与桌面类工具 |
| 互通节点侧 | 命令在本地 preflight 按能力集与 `security.allow_commands` 双重裁决（`07` 册） |

## 6. 反例与缺口

| 项 | 现状 | 性质 |
| --- | --- | --- |
| Tauri 死链 | `crates/wunder-desktop/README.md` 历史说明；`scripts/build-win7*.ps1`、根 `Dockerfile` 仍调 `cargo tauri build` / 装 tauri-cli，指向已不存在的目录 | 工程缺口：脚本待清；不影响「Tauri 壳停止维护」口径 |
| Electron | 全仓零残留 | 文档不得再为其保留任何入口描述 |
| `args.rs` 帮助文案仍提 Tauri、根 `package.json` 的 `build:desktop` | 指向不存在产物 | 同上，待清 |
| 蛹 Linux 形象浮窗 | 浮窗是 Win32 分层窗口实现，Linux 无等价浮窗 | 平台边界，不得写成全平台能力 |
| LAN 对端认证强度 | 未证实 | 不书写为「强认证」，列入 `12` 册未证实项 |

## 7. 验收判据

- 蛹无参数启动进程数 = 1（无 bridge 子进程），且网络断开时可完成本地线程全流程。
- 非 TTY 环境自动落 exec 模式；`exec --json` 输出通过既有契约测试。
- 目标平台产物（win7-x86 / ubuntu18）构建 + 真机运行验收通过；构建成功不替代运行验收。
- 重启后恢复扫描把中断会话收敛到一致状态，无死锁残留；LAN 宣告载荷不含 `user_world` 之外能力。

## 8. 相关文档

- 事实层：`../技术说明书/08-部署、启动与运行模式.md`、`../技术说明书/02-仓库结构与模块职责.md`
- 相邻：`09-用户侧前端设计.md`（Slint/TUI 交互纪律）、`07-接入、网关与渠道系统设计.md`（互通协议）、`12-安全-权限与隔离设计.md`（本地策略与审批）
