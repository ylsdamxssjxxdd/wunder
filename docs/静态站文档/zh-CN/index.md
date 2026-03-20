---
title: wunder
summary: wunder 是统一承载 server、cli、desktop 三种运行形态的智能体调度系统；核心执行结构为“心舰 -> 用户 -> 蜂群 -> 智能体 -> 线程”。
read_when:
  - 你第一次了解 wunder
  - 你要先分清系统结构再决定阅读路径
  - 你要快速找到 desktop、server、cli、接入和运维入口
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# wunder

<p class="docs-eyebrow">统一调度内核 | server / cli / desktop</p>

wunder 是一个面向用户与组织的智能体调度系统。

它当前主推 `desktop`，但底层内核同时服务于 `server` 与 `cli`。

## 这页解决什么

- 用一张图看清 wunder 的核心结构
- 3 分钟内找到你该走的文档入口
- 避免一开始就读错页面

## 系统结构示意图

![wunder 系统结构示意图：心舰到用户、蜂群、智能体、线程的分层关系](/docs/diagrams/system-intro/08-hierarchy-structure.svg)

如果你在当前环境看不到图片，可直接打开原图：[/docs/diagrams/system-intro/08-hierarchy-structure.svg](/docs/diagrams/system-intro/08-hierarchy-structure.svg)

- `用户`：最顶层隔离单位，绑定会话、工作区与资源归属。
- `蜂群`：围绕目标组织的一组智能体协作单元。
- `智能体`：具体执行角色，负责模型调用、工具调用与结果产出。
- `线程`：智能体内部连续上下文，承载状态、记忆注入与执行历史。

一句话理解：**请求先进入用户域，再在蜂群内分工，由智能体在线程中持续执行。**

## 先看这些

<div class="docs-card-grid">
  <a class="docs-card" href="/docs/zh-CN/start/quickstart/">
    <strong>快速开始</strong>
    <span>先把 wunder 跑起来，优先体验 desktop。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/desktop/">
    <strong>Desktop 入门</strong>
    <span>普通用户最短路径入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/architecture/">
    <strong>系统架构</strong>
    <span>进一步看接入层、调度层、工具层与存储层。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/wunder-api/">
    <strong>wunder API</strong>
    <span>统一执行入口，适合服务端接入。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/">
    <strong>工具总览</strong>
    <span>按工具类别查看用途、配置与边界。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/deployment/">
    <strong>部署与运行</strong>
    <span>按桌面、本地开发、服务端部署分流。</span>
  </a>
</div>

## 按角色找页面

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
- [蜂群与多智能体](/docs/zh-CN/concepts/swarm/)
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

## 最容易搞错的点

- `/wunder` 的 `user_id` 不要求是注册用户。
- 会话轮次分为“用户轮次 / 模型轮次”。
- token 统计记录的是上下文占用量，不是账单总消耗量。
- 线程首次确定的 system prompt 会冻结。
- 长期记忆只在线程初始化时注入一次。

## 相关文档

- [文档总览](/docs/zh-CN/start/hubs/)
- [FAQ](/docs/zh-CN/help/faq/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
