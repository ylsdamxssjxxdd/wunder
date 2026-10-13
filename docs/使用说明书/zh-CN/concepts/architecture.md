---
title: 系统架构
summary: 代码怎么分层、一次提交穿过哪些模块、前端看到的是哪种读模型——用来定位问题在哪一层。
read_when:
  - 需要判断一个现象属于引擎、存储还是前端
  - 要改代码或加能力，先确认应该落在哪个 crate
  - 排查"界面与后端不一致"，需要理解投影与真相的边界
source_docs:
  - crates/wunder-core/src/lib.rs
  - crates/wunder-runtime/src/lib.rs
  - crates/wunder-server/src/main.rs
  - crates/wunder-runtime/src/services/runtime/thread/runtime.rs
  - crates/wunder-runtime/src/orchestrator/thread_log_committer.rs
  - crates/wunder-core/src/thread_view.rs
  - crates/wunder-core/src/storage_backend.rs
updated_at: 2026-10-13
---

# 系统架构

一句话：**四个形态共享一份核心与引擎，执行真相只在线程运行时里，界面读的是有界投影。**

## 第一层：crate 与产物

| crate | 产物 | 职责 | 关键事实 |
| --- | --- | --- | --- |
| `wunder-core` | 仅库 | 配置、鉴权、存储契约与记录类型、路径围栏、执行策略、展示读模型常量 | `src/lib.rs` 约 23 个公开模块，无 HTTP 依赖 |
| `wunder-runtime` | 库（+ 可选 bin `backend_sim`） | 引擎：编排器、线程运行时、工具、渠道、网关、互通、存储实现 | 库名是 `wunder_server`（`Cargo.toml` 的 `[lib] name`），所以其它 crate 一律以 `wunder_server::…` 引用它——**库名 ≠ 舰体进程** |
| `wunder-server` | bin | 舰体进程 | 只有 `src/main.rs`，靠依赖 `wunder-runtime` 并转发 feature 组装 |
| `wunder-desktop` | 库 + bin（兼容 bridge） | 蜂窝后端与强类型 façade | 默认同进程原生调用，不启本机 HTTP |
| `wunder-cli` | bin | 舵机 | TTY 三端满足时进 ratatui TUI，否则降级为一次性 exec |

前端三套，各自独立：

| 界面 | 目录 | 交付方式 |
| --- | --- | --- |
| 蜂窝界面 | `frontend-slint/` | 编译进桌面可执行，随蜂窝分发 |
| 蜂巢界面 | `frontend/`（Vue3+TS） | 构建出 `frontend/dist`，由 web 服务器（部署编排中为独立 nginx 容器）**只读挂载下发**；舰体本身不托管它 |
| 舰桥界面 | `web/`（原生 HTML+JS） | 无构建步骤，由舰体进程挂到 `/` |

## 第二层：一次提交穿过什么

从用户发消息到最终回复落库，链路是固定的：

| 步骤 | 位置 | 说明 |
| --- | --- | --- |
| 1. 接入 | `api/chat_ws.rs` | 聊天主通道 `connect / start / resume / watch / cancel / approval` |
| 2. 受理 | `services/runtime/thread/runtime.rs` | **线程运行时本体**：分配或排队，`ThreadSubmitOutcome` 决定立即执行还是入队 |
| 3. 落用户轮次 | `orchestrator/thread_log_committer.rs` | `accept_turn` 幂等分配用户轮次；这是线程日志的**唯一写出口** |
| 4. 准备请求 | `orchestrator/request.rs` | 组装上下文、工具面、提示词（含冻结判定） |
| 5. 主循环 | `orchestrator/execute.rs` | 每次模型调用自增一个模型轮次 |
| 6. 工具执行 | `orchestrator/execute_tools.rs` → `services/tools/dispatch.rs` | 审批、钩子、超时、预算，再分发到具体工具 |
| 7. 最终回复 | `orchestrator/tool_exec.rs` | 按 `{turn_id}:text-{model_round}` 更新同一个可见输出项 |
| 8. 实时推送 | `orchestrator/event_stream.rs` | 发送事件时**同步提交**到持久层 |

三个容易写错的定位：

- `orchestrator/thread_runtime.rs` 是**状态登记表**（供界面问"这条线程现在什么状态"），不是线程运行时本体。
- `orchestrator/thread_change_hub.rs` 的 `publish` 只是**进程内唤醒信号**；多实例收敛靠轮询，不靠它保证正确性。
- `orchestrator/tool_pipeline.rs` 只是四阶段模型描述，钩子默认关闭，不是执行主链。

## 第三层：数据与存储

- 存储能力由 `wunder-core/src/storage_backend.rs` 定义：按域拆成 30+ 个 trait（线程日志、会话、工作区、记忆片段等），再组合成一个 supertrait。引擎只依赖契约。
- 选型在 `storage/factory.rs`：配置为空即 SQLite；`postgres|pg|auto` 走 PostgreSQL；两者由 cargo feature 门控。
- **没有 schema 版本表、没有迁移脚本目录**。建表是幂等的 `CREATE TABLE IF NOT EXISTS`，加列靠启动时的列探测。文档里不要写"按版本号迁移"。
- 记录类型集中在 `wunder-core/src/storage_records.rs`（会话、轮次尾巴、工作区、媒体附件、记忆片段、知识向量、用户与配额、渠道绑定、互通影子）。

## 展示读模型 ≠ 执行快照

这是排查"界面少了内容 / 界面很慢"的关键分界。`wunder-core/src/thread_view.rs` 的开头注释就写着：**有界展示读，执行快照不是分页契约**。

| 读模型 | 页大小 | 上限 | 响应预算 |
| --- | --- | --- | --- |
| 聊天页（每用户轮次一行） | 12 | 20 | 128 KB |
| 线程轨迹页（逐条目） | 80 | 100 | 192 KB |
| 正文分块 | 8 KB/块 | — | 按需取块 |

配套机制：写时投影表与预览截断（`storage/thread_view_projection.rs`）、投影未就绪时走源表有界窗口（`storage/thread_view_source.rs`）、后台重建占空比 ≤1%（`services/thread_view_rebuild.rs`）、变更 feeder 每页 200 条（`services/thread_change_feeder.rs`）。

⚠️ 别把 `services/history.rs` 当分页模块——它服务的是**LLM 上下文压缩**的预算计算。

## 接入面与外部世界

| 面 | 事实 |
| --- | --- |
| HTTP | 路由前缀统一 `/wunder/*`，舰体与蜂窝兼容 bridge 共用大部分模块；错误包络唯一来源 `api/errors.rs` |
| WebSocket | 六个通道：聊天、core 请求面、网关、互通隧道、互通远端旁观、世界/LAN |
| 网关 | **不是 HTTP 网关**，是接入注册表 + 定向 RPC + presence 广播的 Hub（`gateway/mod.rs`），唯一传输层是 `/wunder/gateway/ws` |
| 渠道 | 适配器注册表模式，入站有界队列、出站持久 outbox + 退避（`channels/`） |
| 互通 | 本地节点只出不入，一条隧道承载三平面；能力按 L0–L3 分层，默认设备能力集不含 L3 |
| MCP | 舰体内置 streamable-http 服务端点 `/wunder/mcp`；外部 MCP 客户端配置在 `config.mcp.servers` |

## 运行默认值

| 项 | 默认 | 出处 |
| --- | --- | --- |
| 舰体监听 | Rust 内置 `0.0.0.0:8000`，可被 `WUNDER_HOST/WUNDER_PORT` 或配置覆盖 | `wunder-core/src/config.rs` |
| 活跃会话上限 | 300（非管理员受此约束，管理员不受） | `wunder-core/src/config.rs` |
| 会话锁 | TTL 120 秒，心跳 5 秒 | `orchestrator/constants.rs` |
| LLM / 工具超时 | 各 120 秒 | `orchestrator/constants.rs` |
| 模型轮次上限 | 非管理员 1000 | `orchestrator/execute_support.rs` |

## 常见误解

- **"蜂巢页面是舰体托管的"** —— 不是。舰体只挂舰桥与少量静态目录，蜂巢构建产物由独立 web 服务器下发。
- **"wunder_server 这个库名说明它是舰体专用"** —— 库名是引擎的历史命名，蜂窝与舵机同样链接它。
- **"前端要多拉几次才能拿全历史"** —— 前端本来就不该拿全历史；它消费分页投影，正文按需取块。
- **"蜂窝是通过本机 HTTP 调自己的后端"** —— 默认路径是同进程原生调用；独立 bridge 只为既有兼容客户端保留。
- **"蜂群编排还在架构里"** —— 引擎与调度面已无该模块，`services/swarm/` 只剩空目录，线程写权限恒为可写。

## 延伸阅读

- [智能体循环](/docs/zh-CN/concepts/core-agent-loop/) —— 主循环与刹车
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/) —— 编号口径
- [流式执行](/docs/zh-CN/concepts/streaming/) —— 推送、补水与背压
- [工作区与工作目录](/docs/zh-CN/concepts/workspaces/) —— 路径与沙盒边界
- [部署与运维](/docs/zh-CN/ops/deployment/) —— 编排、端口与构建
- [配置参考](/docs/zh-CN/reference/config/) —— 全部配置项
