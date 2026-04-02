---
title: 术语表
summary: 这页用于快速统一 Wunder 文档里的核心术语，避免把 user、session、thread、container、agent 混成一个概念。
read_when:
  - 你第一次系统性阅读 Wunder 文档
  - 你发现很多词看起来相近但不知道边界
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# 术语表

## `user_id`

Wunder 请求里的用户标识。

它可以是注册用户，也可以只是一个虚拟隔离标识。

## 注册用户

指真正存在于用户管理体系里的账号。

它和 `/wunder` 调用里传入的任意 `user_id` 不是一回事。

## `agent_id`

智能体应用标识。

它通常决定：

- 智能体配置
- 追加提示词
- 工具挂载
- 容器路由

## `session_id`

一次具体会话的标识。

它是对话恢复和继续发送消息时最重要的上下文编号。

## 线程

Wunder 文档里说“线程”时，通常不是操作系统线程，而是会话级运行单元。

它会绑定：

- 冻结后的 system prompt
- 历史消息
- 当前运行态

## 用户轮次 / 模型轮次

用户每发一条消息，记 1 轮用户轮次。

模型每执行一次动作，记 1 轮模型轮次。动作包括：

- 模型调用
- 工具调用
- 最终回复

## `container_id`

工作区容器编号。

当前约定：

- `0`：用户私有容器
- `1~10`：智能体运行容器

## 工作区

Wunder 的持久文件空间。

它按 `user_id + container_id` 隔离，而不是单纯“当前目录”。

## `skill`

面向模型的技能包。

通常包含：

- `SKILL.md`
- 脚本
- 资源文件

## `skill_call`

内置技能调用工具。

模型可用它直接读取技能正文和目录结构。

## MCP

模型上下文协议接入面。

在 Wunder 里，它既可以是 Wunder 自己暴露出去的服务，也可以是 Wunder 接入的外部服务。

## A2A

智能体之间的标准互操作协议接入面。

它更偏“系统对系统”，而不是普通业务接口。

## `channel`

外部消息渠道。

例如飞书、企业微信、QQBot、XMPP 等。

## `outbox`

渠道出站缓冲与重试层。

它让渠道发送从同步操作变成可恢复的异步链路。

## `turn_terminal`

一轮执行的终结事件。

判断一轮是否结束时，应优先看它。

## `thread_status`

线程当前运行态事件。

判断线程现在是否在运行、等待审批或空闲时，应优先看它。

## `approval_resolved`

审批闭环事件。

表示待审批请求已经进入终态。

## 延伸阅读

- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [工作区与容器](/docs/zh-CN/concepts/workspaces/)
- [流式事件参考](/docs/zh-CN/reference/stream-events/)
