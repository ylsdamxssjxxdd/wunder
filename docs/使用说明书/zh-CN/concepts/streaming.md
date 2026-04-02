---
title: "流式执行"
summary: "Wunder 的流式不是简单的字符流，而是把线程执行、工具调用和终态一起暴露出来。"
read_when:
  - "你在接 SSE 或 WebSocket"
  - "你在给聊天界面设计状态机"
source_docs:
  - "docs/API文档.md"
  - "docs/设计方案.md"
  - "src/api/chat.rs"
  - "src/api/core.rs"
---

# 流式执行

在 Wunder 里，流式的重点不是“文字一点点出来”，而是“整个线程状态被连续投影出来”。

## 本页重点

这页只回答三件事：

- 为什么流式链路要同时暴露过程事件和终态事件
- 为什么聊天优先用 WebSocket，通用执行入口仍保留 SSE
- 客户端应该用什么信号判断“还在跑”与“已经结束”

## 什么时候看

如果你正在做这些事，就应该先看这页：

- 聊天窗口流式输出
- 工具调用中间过程展示
- 断线续传或恢复播放
- 一轮执行的状态机设计

## 流式不是只有一种入口

Wunder 当前有两条主流式路径：

- `/wunder`：统一执行入口，`stream=true` 时返回 SSE
- `/wunder/chat/ws`：聊天主实时通道，支持 start、resume、watch、cancel、approval

所以正确理解不是“WS 和 SSE 二选一”，而是：

- 聊天场景优先 WS
- 通用执行和兼容场景走 SSE

## 你真正要关心的不是所有事件名

事件很多，但语义不平权。

### 过程事件

它们用于展示发生了什么：

- `progress`
- `llm_output_delta`
- `tool_call`
- `tool_output_delta`
- `tool_result`

### 状态事件

它们用于表达线程当前跑到哪：

- `queued`
- `approval_request`
- `thread_status`

### 终态事件

它们用于表达是否已经闭环：

- `approval_resolved`
- `turn_terminal`

## 客户端最容易犯的错

### 只看 `final`

这会漏掉失败、拒绝、取消和等待审批。

### 只看 `running`

这在聊天域里只是兼容字段，不足以表达完整运行态。

### 把 SSE 当成完整聊天协议

SSE 可以看结果，但 WebSocket 才更适合会话控制。

## 实施建议

- 判断一轮是否结束，看 `turn_terminal`。
- 判断线程当前处于什么状态，看 `thread_status` 或会话 `runtime`。
- 审批是否彻底闭环，看 `approval_resolved`。
- 聊天面板默认应优先接 WebSocket，SSE 作为兜底。

## 延伸阅读

- [流式事件参考](/docs/zh-CN/reference/stream-events/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
