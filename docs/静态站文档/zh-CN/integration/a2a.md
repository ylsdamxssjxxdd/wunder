---
title: A2A 接口
summary: Wunder 通过 `/a2a` 提供 A2A JSON-RPC 标准接入，并通过 AgentCard 暴露能力发现入口。
read_when:
  - 你要让 Wunder 被别的智能体系统调用
  - 你要把外部 A2A 服务接进 Wunder 的工具体系
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - src/api/a2a.rs
  - config/wunder-example.yaml
---

# A2A 接口

A2A 解决的是“智能体系统之间如何标准化互通”。

在 Wunder 里，这条链路有两层含义：

1. Wunder 自己对外暴露 A2A 服务。
2. Wunder 也能把外部 A2A 服务挂成 `a2a@service` 工具供模型调用。

## 对外暴露的端点

- `POST /a2a`
- `GET /.well-known/agent-card.json`
- `GET /a2a/agentCard`
- `GET /a2a/extendedAgentCard`

## 目前的协议特征

- 请求协议：JSON-RPC 2.0
- A2A protocol version：`1.0`
- 支持流式返回
- 支持 AgentCard 能力发现
- 当配置了 `api_key` 时，AgentCard 会同时声明 API Key 安全方案

## AgentCard 有什么用

AgentCard 主要用于告诉外部系统：

- 你的服务叫什么
- 它的入口 URL 是什么
- 它支持哪些技能
- 它支持哪些工具类别
- 它是否支持流式

也就是说，AgentCard 是“发现和介绍自己”的那一层。

## `POST /a2a` 适合什么场景

适合这些场景：

- 让外部智能体平台把 Wunder 当成一个远端协作体
- 让外部系统通过统一标准接入 Wunder 的模型和工具能力
- 做跨系统智能体编排，而不是只做单系统工具调用

## 与 `/wunder` 的区别

- `/wunder` 更像 Wunder 自己的统一执行入口
- `/a2a` 更像对外互操作协议入口

如果你接的是业务系统内部调用，通常先用 `/wunder`。

如果你接的是“另一套智能体系统”，再优先考虑 `/a2a`。

## 在 Wunder 内部怎么使用外部 A2A

配置文件里可以声明外部 A2A 服务：

```yaml
a2a:
  services:
    - name: wunder
      endpoint: http://127.0.0.1:8000/a2a
      enabled: false
```

启用后，这个服务会在工具侧以 `a2a@wunder` 的形式出现。

系统还内置了两个辅助工具：

- `a2a观察`
- `a2a等待`

它们分别用于观察任务状态和等待结果收敛。

## 接入时要注意的点

- A2A 服务名最终会变成工具名的一部分，例如 `a2a@service_name`
- `service_type=internal` 的服务通常需要固定 `user_id`
- 是否允许自调用由 `allow_self` 控制
- 超时由 `a2a.timeout_s` 控制

## 适合 A2A 的任务

适合放到 A2A 的，通常是这些：

- 远端专业能力
- 跨系统智能体协同
- 需要明确协议边界的外部代理调用

不适合放到 A2A 的，通常是：

- 只在本系统内执行的普通文件工具
- 单机内建能力
- 没有必要跨系统的同步调用

## 延伸阅读

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)
- [蜂群协作](/docs/zh-CN/concepts/swarm/)
