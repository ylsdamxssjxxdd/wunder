---
title: wunder
summary: wunder 是统一承载 server、cli、desktop 三种运行形态的智能体调度系统；当前对外调用分为底层执行入口和完整智能体会话入口两层。
read_when:
  - 你第一次了解 wunder
  - 你要快速找到 desktop、server、cli、接入和运维入口
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# wunder

<p class="docs-eyebrow">统一调度内核 | server / cli / desktop</p>

wunder 是一个智能体调度系统。

它现在主推 `desktop`，但底层内核同时服务于 `server` 和 `cli`。

如果你是第一次打开文档站，这一页只解决两件事：

- wunder 到底是什么
- 你下一步应该点哪一页

<div class="docs-card-grid">
  <a class="docs-card" href="/docs/zh-CN/start/quickstart/">
    <strong>快速开始</strong>
    <span>先把 wunder 跑起来，优先体验 desktop。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/architecture/">
    <strong>系统架构</strong>
    <span>先分清 server、调度层、工具层和工作区。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/wunder-api/">
    <strong>wunder API</strong>
    <span>统一执行入口，适合最短路径接入。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/">
    <strong>工具总览</strong>
    <span>看内置工具、MCP、Skills 和知识库从哪里挂进来。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/deployment/">
    <strong>部署与运行</strong>
    <span>按桌面、本地开发、服务端部署三条路径看。</span>
  </a>
</div>

## 是什么

- 同一套调度内核同时支撑 `server / cli / desktop`
- 对外统一暴露 `/wunder`，并提供完整聊天域 `/wunder/chat/*`
- 模型可以调用内置工具、MCP、Skills、知识库和 A2A 能力
- 系统支持多用户并发、工作区隔离、长期记忆和蜂群协作
- 前端分为用户侧前端、管理端前端和桌面端三块界面

可以把 wunder 理解成一句话：

- 对开发者，一切是接口。
- 对模型，一切是工具。

## 先从哪里开始

如果你只想尽快看见可用结果，先从这些页面开始：

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/start/quickstart/">
    <strong>快速开始</strong>
    <span>最短路径把 wunder 跑起来。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/desktop/">
    <strong>Desktop 入门</strong>
    <span>当前主推形态，本地优先。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/server/">
    <strong>Server 部署</strong>
    <span>组织级部署、多用户和治理入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/cli/">
    <strong>CLI 使用</strong>
    <span>开发、调试和自动化任务入口。</span>
  </a>
</div>

## 不同人下一步看什么

### 你是普通用户

- [快速开始](/docs/zh-CN/start/quickstart/)
- [Desktop 入门](/docs/zh-CN/start/desktop/)
- [用户侧前端](/docs/zh-CN/surfaces/frontend/)

### 你是管理员或交付人员

- [Server 部署](/docs/zh-CN/start/server/)
- [部署与运行](/docs/zh-CN/ops/deployment/)
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)

### 你是开发者或集成人员

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [工具总览](/docs/zh-CN/tools/)

## 如果你要理解结构

- [系统架构](/docs/zh-CN/concepts/architecture/)
- [工作区与容器](/docs/zh-CN/concepts/workspaces/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [工具体系](/docs/zh-CN/concepts/tools/)
- [长期记忆](/docs/zh-CN/concepts/memory/)

## 如果你要做接入

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [用户世界接口](/docs/zh-CN/integration/user-world/)
- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)
- [A2A 接口](/docs/zh-CN/integration/a2a/)
- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)

## 你最需要记住的点

- `/wunder` 的 `user_id` 不要求是注册用户
- 会话轮次分为“用户轮次 / 模型轮次”
- token 统计记录的是上下文占用量，不是账单总消耗量
- 线程首次确定的 system prompt 会冻结
- 长期记忆只在线程初始化时注入一次

## 相关文档

- [文档总览](/docs/zh-CN/start/hubs/)
- [FAQ](/docs/zh-CN/help/faq/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
