---
title: 流式事件参考
summary: 接 Wunder 的流式链路时，最重要的不是把每个事件都背下来，而是知道哪些事件承担真正的生命周期语义。
read_when:
  - 你在接 SSE 或 WebSocket
  - 你想知道 turn_terminal、approval_resolved、thread_status 为什么重要
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - src/orchestrator/execute.rs
  - src/api/chat.rs
---

# 流式事件参考

Wunder 的流式事件很多，但真正关键的不是“数量”，而是“哪几个事件承担状态语义”。

## 核心事件优先级

- `thread_status`
- `approval_resolved`
- `turn_terminal`

如果你只消费旧的 `final` 或 `error`，很容易把线程状态判断错。

## 一类：过程事件

这些事件主要用于把执行过程可视化：

- `progress`
- `llm_output_delta`
- `round_usage`
- `tool_call`
- `tool_output_delta`
- `tool_result`

它们负责告诉你模型和工具在干什么，但不负责判定“一轮是否结束”。

### `round_usage` 要怎么理解

如果你要显示 token 统计，最该优先消费的就是 `round_usage`：

- `round_usage.total_tokens`：当前这次请求完成后的实际上下文占用
- `round_usage.context_occupancy_tokens`：和上面相同，但字段名更直接
- `round_usage.request_consumed_tokens`：当前这次请求的消耗；整段会话的累计消耗就是把它逐次相加

`token_usage` 仍然有价值，但它更偏单次模型调用明细，不再是线程级上下文占用的唯一准绳。

## 二类：排队与等待事件

典型有：

- `queued`
- `approval_request`

它们表达的是：

- 请求已进入排队
- 当前轮次在等审批

## 三类：闭环事件

### `approval_resolved`

它表示审批已经进入终态。

不论是批准、拒绝还是取消，都应该由这个事件承担“审批闭环完成”的语义。

### `turn_terminal`

这是当前一轮执行的唯一终结语义。

它的 `status` 可能包括：

- `completed`
- `failed`
- `cancelled`
- `rejected`

如果你在做状态机，请优先以它作为“一轮结束”的依据。

### `thread_status`

它描述的是线程当前运行态。

典型状态包括：

- `running`
- `waiting_approval`
- `waiting_user_input`
- `interrupting`
- `idle`
- `not_loaded`

它解决的是“线程现在活着吗、卡在哪”。

## 为什么不能只看 `final`

因为 `final` 更像“有最终回答文本”。

但真实运行里还会遇到：

- 被拒绝
- 被取消
- 等审批
- 中途失败

这些场景下，只看 `final` 根本不够。

## 接入时的最低处理建议

如果你在做一个新的客户端，至少正确处理：

- `queued`
- `thread_status`
- `approval_request`
- `approval_resolved`
- `turn_terminal`
- `error`

这样你的状态机才不会只在“最顺的路径”里成立。

## SSE 和 WS 的区别要点

两者语义尽量保持一致，但体验侧仍有差别：

- WebSocket 更适合长会话和实时控制
- SSE 更适合作为兼容兜底

所以“默认 WebSocket，SSE 兜底”不是一句宣传语，而是接入策略。

## 一个简单判断法

如果你只想知道这轮是不是结束：

- 看 `turn_terminal`

如果你想知道线程现在是什么状态：

- 看 `thread_status`

如果你想知道审批流程有没有彻底收口：

- 看 `approval_resolved`

## 延伸阅读

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [API 索引](/docs/zh-CN/reference/api-index/)
