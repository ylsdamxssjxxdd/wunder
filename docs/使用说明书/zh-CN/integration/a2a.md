---
title: A2A 接口
summary: wunder 既能把能力按 A2A 协议暴露给别的智能体系统，也能把外部 A2A 服务挂成工具供模型调用。
---

# A2A 接口

如果对方是“另一套智能体系统”，想按统一协议互相调用，用 A2A；只是你自己业务系统内部的调用，用 [wunder API](/docs/zh-CN/integration/wunder-api/) 更简单。

A2A 在 wunder 里有两个方向：

- 把 wunder 的能力暴露出去，让外部智能体平台调用；
- 把外部的 A2A 服务接进来，作为模型可用的工具。

## A2A 的适用条件

适合：外部智能体平台把 wunder 当成远端协作体；跨系统协同，需要明确的协议边界。

不适合：只在本系统内执行的普通文件操作、单机内建能力、没必要跨系统的同步调用。这些留在本机就好。

## 最短链路：让外部调 wunder

对外提供这几个入口：

- `POST /a2a`：主调用入口
- `GET /.well-known/agent-card.json`
- `GET /a2a/agentCard`
- `GET /a2a/extendedAgentCard`

外部系统按 JSON-RPC 2.0 发请求，协议版本 1.0，支持流式返回。它先读 AgentCard 了解 wunder 是谁、入口在哪、有哪些技能、支持哪些工具类别、是否支持流式；配置了 API Key 时，AgentCard 里也会一并声明对应的安全方案。

## 最短链路：让 wunder 调外部

在配置里声明外部 A2A 服务，给它一个名字和一个服务地址，然后打开开关。启用后，这个服务会作为一个可调用的工具出现在模型面前，名字由服务名决定，形如 `a2a@服务名`。

系统另有两个辅助工具，分别用来观察任务状态和等待结果收敛。

## 关键设置与常见坑

- 服务名会成为工具名的一部分，起名时先想清楚。
- 内部类型的外部服务，通常需要固定一个用户标识。
- 是否允许 wunder 调用自己，可以单独开关。
- 调用有超时上限，远端比较慢时要提前评估。

## 延伸阅读

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)
