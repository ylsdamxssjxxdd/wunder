---
title: "运行时与在线状态"
summary: "Wunder 不只返回消息内容，还持续维护线程和会话的运行时投影，用于表达 loaded、waiting、streaming、idle 等状态。"
read_when:
  - "你在做会话列表、状态徽标或运行面板"
  - "你在排查线程到底是卡住了还是已经结束"
source_docs:
  - "docs/API文档.md"
  - "src/api/chat.rs"
  - "src/core/session_runtime.rs"
---

# 运行时与在线状态

Wunder 的会话不是一条静态历史记录，它始终带着一层运行时状态。

## 本页重点

如果你想知道：

- 线程现在是不是还活着
- 为什么会话列表需要 `runtime`
- `running`、`thread_status`、`terminal_status` 分别表达什么

这页就是给这些问题用的。

## 两层状态不要混

### 线程事件状态

事件流里的 `thread_status` 更像“线程现在在干什么”。

常见值包括：

- `running`
- `waiting_approval`
- `waiting_user_input`
- `interrupting`
- `idle`
- `not_loaded`

### 会话运行时投影

聊天域会把状态再汇总成 `runtime` 对象，给列表页和详情页直接消费。

当前重点字段包括：

- `status`
- `loaded`
- `active`
- `streaming`
- `waiting`
- `watcher_count`
- `pending_approval_count`
- `monitor_status`
- `monitor_stage`
- `terminal_status`

## 为什么不能只保留一个 `running`

因为 `running=true/false` 太粗了。

它回答不了这些问题：

- 是在持续输出，还是在等审批
- 会话是否已经加载到内存
- 有没有观察者挂在这个线程上
- 这轮是不是已经进入 `completed/failed/cancelled/rejected`

所以 Wunder 现在保留 `running` 兼容字段，但真正稳定的接入应该看 `runtime`。

## 会话页最常见的状态判断

如果你在做列表或详情页，通常按这个思路就够了：

- 是否还在持续执行：看 `runtime.streaming`
- 是否处于等待态：看 `runtime.waiting`
- 是否已经有终态：看 `runtime.terminal_status`
- 是否还能被继续观察：看 `runtime.loaded` 和 `runtime.active`

## 什么时候最容易用错

### 把“等待审批”当成“已经结束”

这会导致前端错误地恢复输入框或隐藏审批区。

### 只在详情页读状态

会话列表、事件页和线程面板都应该复用同一套运行时语义。

### 把事件流和摘要状态混成一层

事件负责时间线，`runtime` 负责当前快照。

## 实施建议

- `thread_status` 适合表达过程中的线程状态。
- `runtime` 适合给会话列表、详情页和控制面板直接消费。
- `running` 只是兼容字段，不应该再承担完整状态机。

## 延伸阅读

- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [流式执行](/docs/zh-CN/concepts/streaming/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
