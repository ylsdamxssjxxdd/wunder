---
title: 会话线程控制
summary: `会话线程控制` 负责线程树、主线程和派生会话管理，是 Wunder 会话结构层的正式工具，而不是前端私有状态。
read_when:
  - 你要管理线程树、主线程和派生会话
  - 你要区分线程结构控制和多智能体协作
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - src/services/tools/thread_control_tool.rs
  - src/services/tools/catalog.rs
---

# 会话线程控制

在 Wunder 里，会话树不是“前端自己记一下当前对话是哪个”，而是正式工具能力。

## `会话线程控制`

这是线程树的主工具。

当前核心动作包括：

- `list`
- `info`
- `create`
- `switch`
- `back`
- `update_title`
- `archive`
- `restore`
- `set_main`

它解决的是：

- 新建子线程
- 在线程树里切换
- 返回父线程
- 归档或恢复线程
- 把某条线程设为主线程

## 它真正改变什么

它不是简单的聊天接口包装，而是直接改变：

- 线程结构
- 会话归属
- 主线程映射

## 和其他协作工具的区别

- 这页只讲线程树和主线程。
- [子智能体控制](/docs/zh-CN/tools/subagent-control/) 讲单个子会话运行。
- [智能体蜂群](/docs/zh-CN/tools/agent-swarm/) 讲多智能体并发协作。

如果你的目标是“把哪条线程设为当前主线”，看这页。

如果你的目标是“把任务扔给另一个智能体去跑”，看后两页。

## 实施建议

- 线程控制是会话树工具，不只是“切对话”。
- `set_main` 会影响系统对主线程的映射。
- 子会话和蜂群协作是相关但独立的另外两类工具。

## 延伸阅读

- [子智能体控制](/docs/zh-CN/tools/subagent-control/)
- [智能体蜂群](/docs/zh-CN/tools/agent-swarm/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [蜂群协作](/docs/zh-CN/concepts/swarm/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
