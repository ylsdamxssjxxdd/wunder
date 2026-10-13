---
title: 实时性
summary: 执行过程持续推送，靠游标补水而不是靠重连碰运气。WebSocket 是唯一实时通道，事件名散在发送处、没有集中枚举，这页给出真实清单与背压口径。
read_when:
  - 客户端断线后不确定怎么补齐漏掉的内容
  - 要接 WebSocket，需要知道端点、事件名与游标语义
  - 界面停在旧状态、或出现「慢客户端」提示
  - 要解释蜂窝/蜂巢/渠道多端为什么状态能对齐
source_docs:
  - crates/wunder-runtime/src/api/chat_ws.rs
  - crates/wunder-runtime/src/api/core_ws.rs
  - crates/wunder-runtime/src/api/gateway_ws.rs
  - crates/wunder-runtime/src/api/ws_helpers.rs
  - crates/wunder-runtime/src/api/chat/events.rs
  - crates/wunder-runtime/src/api/chat/view.rs
  - crates/wunder-runtime/src/orchestrator/constants.rs
  - crates/wunder-runtime/src/orchestrator/event_stream.rs
  - crates/wunder-runtime/src/services/thread_change_feeder.rs
  - crates/wunder-core/src/thread_view.rs
updated_at: 2026-10-13
---

# 实时性

实时性不是「字出得快」，而是三件事：**过程持续可见**、**断线能补齐**、**客户端看到的与后端真相一致**。wunder 的实现是「持久事件 + 游标补水 + 有界投影」，不是「重连后祈祷」。

## 一条铁律：后端是真相，前端是投影

线程日志是唯一真相源。每一次事件推送在发出前**先同步 commit 到持久层**（`orchestrator/event_stream.rs` 的 `EventEmitter`），之后才走实时通道；变更唤醒信号（`orchestrator/thread_change_hub.rs`）只是唤醒，**不是数据源**，也不是真相。

所以：

- 推送丢了可以补，真相不会丢。
- 界面显示异常时以实际执行状态为准，不要按界面判断任务是否结束。
- 判断「一轮结束了没有」只看终态事件，不看「好像没动静了」。

## 六个 WebSocket 端点

| 端点 | 用途 | 出处 |
| --- | --- | --- |
| `/wunder/chat/ws` | 线程运行时主通道：`connect` / `ping` / `start` / `resume` / `watch` / `cancel` / `goal.*` / `approval` | `api/chat_ws.rs:49` |
| `/wunder/ws` | core 请求面：WunderRequest 提交与流式返回 | `api/core_ws.rs:37` |
| `/wunder/gateway/ws` | 网关三角色 `operator` / `node` / `channel` 的 req/res/event/ping | `api/gateway_ws.rs:25` |
| `/wunder/interlink/ws`（配套 `POST /wunder/interlink/channel_ticket`） | 本地节点反向隧道 | `api/interlink_ws.rs:73-76` |
| `/wunder/interlink/remote_ws` | 浏览器旁观远端单线程，纯转发不落库 | `api/interlink_remote.rs:40` |
| `/wunder/user_world/ws`、`/wunder/desktop/lan/ws` | 世界会话 watch/send；LAN mesh | `api/user_world_ws.rs:28`、`api/desktop_lan.rs:26` |

聊天流式**只走 WebSocket**：HTTP 侧请求聊天流会被拒并回提示 `/wunder/chat/ws`（`api/chat.rs:305` 附近）。SSE 只用在外部工作流（`api/external_workflows.rs`），**不是聊天的兜底通道**，别写成「WS 断了走 SSE」。

握手与鉴权：先 `connect` 信封，或用 `access_token` / `token` query 参数（`api/ws_helpers.rs:33`）。未完成握手的其它消息会被拒。

## 事件名没有集中枚举

这是个已知工程缺口：**全仓没有一份事件名常量表**，事件名字符串散在各发送处。下表是当前实际存在的名字（按发送位置核实），新增名字请直接补发送处而不是假设这里有一份枚举。

| 类别 | 事件名 |
| --- | --- |
| 投影与补水 | `thread_view_change`、`thread_view_required`、`thread_snapshot_required`、`thread_status` |
| 线程内容 | `thread_item_tail`、`thread_item_delta`（`chat_ws.rs:641-645`） |
| 模型输出 | `llm_output_delta`、`llm_request` |
| 工具 | `tool_call`、`tool_result` |
| 排队 | `queue_enter`、`queue_update`、`queue_start`、`queue_finish`、`queue_fail` |
| 用量与额度 | `model_request_usage` |
| 终态 | `final`、`turn_terminal` |
| 流控与错误 | `heartbeat`、`slow_client`、`stream_overflow`、`resume_recommended`（作为载荷字段随 `slow_client`/`stream_overflow` 发出）、`error` |

发送处主要在 `api/ws_helpers.rs`、`api/chat_ws.rs`、`orchestrator/event_stream.rs`。

## 补水：游标 + HTTP 端点

断线重连的正确做法不是「重新拉全量」，而是带游标补齐。

| 游标 | 含义 |
| --- | --- |
| `change_seq` | 线程变更序列（持久层的变更顺序）；请求带 `after_change_seq` |
| `view_seq` | 展示投影序列，客户端按它判断本地投影是否落后 |
| `event_seq` / `event_id` | 单条事件的序号，随事件注入载荷（`ws_helpers.rs:919-928`） |

补齐端点（HTTP，鉴权同用户面）：

- `/wunder/chat/sessions/{id}/thread-log/changes`、`/snapshot`、`/events`（`api/chat/events.rs:30-56`）
- `/wunder/chat/sessions/{id}/view/bootstrap|chat|trajectory|changes|search`（`api/chat/view.rs:19-35`）

消费侧 feeder 每页固定 200 条变更，读满页立刻继续（`services/thread_change_feeder.rs:19-20`）。

**提交幂等**：一次提交带 `client_message_id`，重复提交直接判「已接受」并返回原结果（`orchestrator/request.rs:136`、`thread_log_committer.rs:246`）。重连后重发同一条消息不会写两份。

## 背压与有界（数值都有出处）

| 项 | 值 | 出处 |
| --- | --- | --- |
| 单订阅写队列 | 256 | `orchestrator/constants.rs:26` |
| 慢客户端水位 | 队列剩余 ≤ 2 即判慢 | `constants.rs:32` |
| 单次拉取上限 | `FETCH_LIMIT = 200` | `constants.rs:34` |
| 心跳 | 15 秒 | `api/ws_helpers.rs:29` |
| 单条 WS 消息上限 | 512 KiB | `ws_helpers.rs:28` |
| 互通远观消息上限 | 256 KiB | `api/interlink_remote.rs:35` |
| 用户输入上限 | `1 << 20` 字符（图片附件不计入） | `constants.rs:40`、`wunder-core/src/request_limits.rs:3` |

慢客户端的处理是**先通知再脱钩**：写队列越过水位时，先发 `slow_client`（或 `stream_overflow`）并带 `resume_recommended: true`，随后断开该订阅（`ws_helpers.rs:219-232`、`877-897`）。这不是故障，是保护：宁让一个落后订阅退出走补水，也不让慢客户端把整条流和引擎拖住。

看到 `resume_recommended` 的正确反应：走上面的补水端点重建投影，然后带新游标重新 `watch`。不要继续等同一个连接把漏掉的内容补回来。

## 展示读模型是分页有界的

大数据量线程的时间线与轨迹不靠「一次拉全」，读模型常量在 `wunder-core/src/thread_view.rs:5-11`：

| 项 | 值 |
| --- | --- |
| 聊天页大小 / 上限 | 12 / 20 |
| 轨迹页大小 / 上限 | 80 / 100 |
| 响应预算 | 聊天 128 KB、轨迹 192 KB |
| 正文分块 | 8192 字节 |

投影未就绪时，读路径回落到源表的**有界窗口**：页大小按上限 clamp，再按响应预算截（`storage/thread_view_source.rs:142-151`）。后台重建有占空比上限（≤1%，失败退避 5s→300s，`services/thread_view_rebuild.rs:36-45`），所以刚导入或刚被大量改动的线程可能短时间仍走源表读——表现是首屏稍慢，不是数据缺失。

线程目录分页上限 100，关键词搜索的扫描上限是 10 页 × 100 = 1000 条（`services/thread_catalog.rs:16-21`）；超出部分不会被搜到，这是刻意的延迟上限。

## 互通：另一条实时链路（只出不入）

本地设备（蜂窝/舵机）与云端之间的实时面是互通（interlink），与上面的客户端 WS 不同：

- **本地节点只出不入**：不开监听端口，自己发起一条长连接 WSS，**三平面（控制、数据、命令）都复用这一条 socket**（`services/interlink/client/mod.rs:1-16`）。
- **影子投影**：云端持有本地线程与工作区目录的投影（`shadow_full` / `shadow_delta` 帧），落库有活通道门禁，通道过期不写新影子。
- **命令台账与审批**：命令、ACK、事件、结果都记账（`interlink_commands` / `interlink_approvals` / 审计表）；风险按 L0–L3 分层，`tool.exec`、`agent.spawn` 属 L3，默认设备能力集不含 L3，L3 审批不可记忆。
- **舰桥互通面板当前不存在**：互通治理现在是后端 API only，舰桥没有互通舰队/命令/审计面板。任何写「舰桥互通治理面板」的说法都是错的。

细节与协议见 [互通](/docs/zh-CN/integration/interlink/)。

## 常见误解

- **「断线重连就会自动补回来」** — 不会自动。要带游标走补水端点，或响应 `resume_recommended`。
- **「SSE 是聊天的兜底通道」** — 不是。聊天流只在 `/wunder/chat/ws`；SSE 属于外部工作流。
- **「看到 `final` 就是成功」** — 终态还包括失败、取消、拒绝；判断要看终态事件与线程状态，不是只看有没有 `final`。
- **「事件名有一份枚举」** — 没有，散在发送处，这是工程缺口。
- **「`slow_client` 是服务端故障」** — 它是流控保护，服务端继续跑，客户端走补水即可。
- **「界面能滚动到最早的消息 = 后端一次返回了全部」** — 不是，读模型分页且有响应预算。
- **「唤醒信号就是数据」** — `thread_change_hub` 只发唤醒，内容一律从持久层读。

## 延伸阅读

- [流式执行](/docs/zh-CN/concepts/streaming/) —— token 增量与合并节奏
- [稳定性](/docs/zh-CN/concepts/core-stability/) —— 超时、熔断与资源治理
- [会话状态说明](/docs/zh-CN/reference/stream-events/) —— 状态与事件详细含义
- [聊天 WebSocket 接入](/docs/zh-CN/integration/chat-ws/) —— 握手与消息格式
- [聊天会话接口](/docs/zh-CN/integration/chat-sessions/) —— 补水与会话端点
- [互通](/docs/zh-CN/integration/interlink/) —— 隧道、影子、命令与审批
