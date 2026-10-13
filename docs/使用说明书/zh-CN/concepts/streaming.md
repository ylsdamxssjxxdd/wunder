---
title: 流式执行
summary: 执行过程怎么实时到达界面：先落库再推送、按游标续传、前端只消费有界投影。
read_when:
  - 界面输出停顿、丢字、重复气泡或刷新后内容变化
  - 断线重连后不知道该续传还是重拉
  - 自行实现客户端，需要正确的订阅与补水姿势
source_docs:
  - crates/wunder-runtime/src/orchestrator/event_stream.rs
  - crates/wunder-runtime/src/orchestrator/thread_log_committer.rs
  - crates/wunder-runtime/src/orchestrator/thread_change_hub.rs
  - crates/wunder-runtime/src/services/thread_change_feeder.rs
  - crates/wunder-runtime/src/api/ws_helpers.rs
  - crates/wunder-core/src/thread_view.rs
updated_at: 2026-10-13
---

# 流式执行

流式解决的不是"字要一个个出来"，而是三件事：**过程实时可见、看到的就是落库的、断线能接上**。

## 顺序：先 durable，再通知

```
引擎发事件 ──┬── 同步提交到线程台账（唯一写出口：每会话串行 gate + 单事务）
            └── 事务成功后才发布持久游标 → 唤醒订阅者
```

三条保证：

- **提交顺序 = 变更顺序**：每条会话一个 gate，写入在短事务里完成；失败的提交**不会**发布游标。
- **幂等不吵**：重放的受理、状态未变、重复反馈都不产生变更也不通知。
- **唤醒只是加速**：进程内唤醒通道不保证正确性；服务端续传器始终保留轮询兜底（250 毫秒起、指数退避到 4 秒），多实例靠轮询收敛。

因此"界面显示的文本"与"刷新后读到的文本"同源，不存在"流里有、库里没有"的中间态。

## 通道：只有 WebSocket

| 端点 | 用途 |
| --- | --- |
| `/wunder/chat/ws` | 聊天线程主通道：`connect / ping / start / resume / watch / cancel / approval` |
| `/wunder/ws` | 通用请求面（提交一次执行并流式接收） |
| `/wunder/interlink/remote_ws` | 浏览器旁观远端设备的单条线程，纯转发不落库 |
| `/wunder/user_world/ws`、`/wunder/desktop/lan/ws` | 站内会话与局域网协同 |

蜂窝默认形态**不经过本机 HTTP/WebSocket**：同进程原生事件通道（有界，容量 128 条）直接投递；丢订阅或关窗口不会取消已在服务端受理的轮次。

⚠️ **聊天没有 SSE 兜底**。服务端确有 SSE，但只用于对外集成面（外部工作流 `POST /wunder/external/workflows:stream`，15 秒保活）与模型代理调用，不是聊天的降级通道。

## 事件分四类，粒度不同

| 类别 | 典型事件 | 是否进时间线 | 用法 |
| --- | --- | --- | --- |
| 条目事件 | `tool_call`、`tool_result`、`llm_request`、`llm_output`、`approval_*`、`subagent_message`、`compaction`、`turn_terminal` | 是 | 按稳定条目 id upsert |
| 传输增量 | `llm_output_delta`、`thread_item_tail`、`thread_item_delta` | 否 | 只补字：助手正文走 tail，命令会话输出走 delta |
| 状态与回执 | `thread_status`、`final`、`error`、`thread_closed`、`queue_*` | 部分（排队是持久台账） | 判断生命周期，不渲染成条目 |
| 唤醒与补水 | `thread_view_change`、`thread_view_required`、`thread_snapshot_required`、`stream_overflow`、`slow_client`、`resume_recommended` | 否 | 客户端据此续传或整体重拉 |

统计类（`context_usage`、`token_usage`、`round_usage`、`model_usage`、`quota_*`）只用于面板刷新，不入时间线。

## 断线与补水

客户端保存**持久游标**（变更序号），重连时带上：

| 情况 | 服务端信号 | 客户端该做 |
| --- | --- | --- |
| 落后但不多 | 从游标续发变更行 | 就地增量应用 |
| 落后超过窗口 | `stream_overflow` + `resume_recommended` | 先收该信号再断开该订阅，按游标重连 |
| 持久窗口已被裁剪 | `thread_snapshot_required` | 放弃续传，拉一次原子快照 |
| 展示投影落后 | `thread_view_required` / `thread_view_change` | 重取分页视图，不逐条拼接 |

补水端点成套：线程变更列表、原子快照、事件分页、单条目正文（都在 `/wunder/chat/sessions/{id}/thread-log/...` 下）。

## 有界是默认设定

| 参数 | 值 | 说明 |
| --- | --- | --- |
| 每连接写队列 | 256 | 满即判慢客户端 |
| 慢客户端水位 | 2 | 队列深度超过即告警/降级 |
| 续传器每页 | 200 | 一次只投一页变更 |
| 事件读取上限 | 200 | 补水分页 |
| 心跳 | 15 秒 | 空闲期保活 |
| 单条 WS 消息 | 512 KiB | 超出即拒收（远端旁观 256 KiB） |
| 聊天页 / 轨迹页 | 12（上限 20）/ 80（上限 100） | 响应预算 128 KB / 192 KB |
| 正文分块 | 8 KB | 长文按需取块 |

前端消费的是**投影**：聊天一页只装 12 个用户轮次，每个轮次除最终回复外的中间内容折叠成"工作过程"，展开时才拉该轮条目。轨迹才是逐条目视图（每行子智能体卡片上限 8 个）。

## 实践

**判断一轮是否结束**：看 `turn_terminal`（带状态、`stop_reason`、可选错误与恢复动作），不要靠"好像不动了"。等待审批或等待补充信息时线程仍在生命周期内。

**看到重复气泡**：多半是客户端未按条目 id upsert，而是又追加了一条新消息。正确做法是用 `{turn_id}:text-{model_round}` 定位同一项。

**丢字**：检查是否把增量当作唯一来源。增量只做连续展示，正文真相在条目里；重连后应补水而不是猜。

**刷新后内容变长**：正常。刷新读的是落库正文，可能比流式尾块更完整。

## 常见误解

- **"流式和落库是两条路"** —— 不是，发事件时同步落库，游标在事务之后才发布。
- **"断线 = 执行中断"** —— 执行在服务端线程里继续，客户端重连按游标接上即可。
- **"SSE 是聊天的降级方案"** —— 不是，聊天只有 WebSocket。
- **"多拉几次就能拿全历史"** —— 读模型有意有界，全量搬运正是设计上要避免的。
- **"看到字在出 = 循环在转很多轮"** —— 文本增量与模型轮次是两种粒度，一轮内可以是成千上万个增量。

## 延伸阅读

- [会话状态说明](/docs/zh-CN/reference/stream-events/) —— 事件与字段清单
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/) —— 协议与信封
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/) —— 条目与轮次粒度
- [实时性](/docs/zh-CN/concepts/core-realtime/) —— 设计原则
- [系统架构](/docs/zh-CN/concepts/architecture/) —— 投影与真相的分层
