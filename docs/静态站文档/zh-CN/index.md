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
当前产品重心是 `desktop`，但底层能力同时服务于 `server` 和 `cli`。

> 对开发者，一切皆接口。对模型，一切皆工具。

> 接入层当前分为两层：
> `POST /wunder` 是底层执行入口；
> `/wunder/chat/*` 是完整智能体会话入口。

<div class="docs-card-grid">
  <a class="docs-card" href="/docs/zh-CN/start/quickstart/">
    <strong>快速开始</strong>
    <span>先把 Wunder 跑起来，优先体验 desktop。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/architecture/">
    <strong>系统架构</strong>
    <span>理解 server、调度层、工具层、工作区和多界面的协作关系。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/wunder-api/">
    <strong>wunder API</strong>
    <span>理解 `/wunder` 和 `/wunder/chat/*` 的职责边界与推荐接入路径。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/">
    <strong>工具总览</strong>
    <span>查看 Wunder 内置工具、MCP、Skills 和知识能力。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/deployment/">
    <strong>部署与运行</strong>
    <span>按桌面、本地开发、服务端部署三条路径落地。</span>
  </a>
</div>

## wunder 能做什么

- 统一承载 `server / cli / desktop` 三种运行形态
- 提供底层执行入口 `/wunder` 和完整会话入口 `/wunder/chat/*`
- 支持 WebSocket 优先、SSE 兜底的流式执行链路
- 提供内置工具、MCP、Skills、知识库与 A2A 能力
- 支持多用户并发、工作区隔离、长期记忆与蜂群协作
- 同时提供用户侧前端、管理端前端和桌面端界面

## 从哪里开始

如果你是第一次使用 wunder，建议先从这些页面开始：

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/start/quickstart/">
    <strong>快速开始</strong>
    <span>先跑起来，再回头看原理。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/desktop/">
    <strong>Desktop 入门</strong>
    <span>当前主推形态，本地优先。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/server/">
    <strong>Server 部署</strong>
    <span>多用户、多租户与治理能力入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/cli/">
    <strong>CLI 使用</strong>
    <span>开发、调试与自动化场景入口。</span>
  </a>
</div>

如果你要理解系统结构：

- [系统架构](/docs/zh-CN/concepts/architecture/)
- [工作区与容器](/docs/zh-CN/concepts/workspaces/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [工具体系](/docs/zh-CN/concepts/tools/)
- [长期记忆](/docs/zh-CN/concepts/memory/)

如果你要做接入：

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [用户世界接口](/docs/zh-CN/integration/user-world/)
- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)
- [A2A 接口](/docs/zh-CN/integration/a2a/)
- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)

如果你要查稳定参考：

- [配置说明](/docs/zh-CN/reference/config/)
- [流式事件参考](/docs/zh-CN/reference/stream-events/)
- [API 索引](/docs/zh-CN/reference/api-index/)

## wunder 的关键约定

- `/wunder` 的 `user_id` 不要求是注册用户
- 会话轮次分为“用户轮次 / 模型轮次”
- token 统计记录的是上下文占用量，不是账单总消耗量
- 线程首次确定的 system prompt 会冻结
- 长期记忆只在线程初始化时注入一次

## 相关文档

- [文档总览](/docs/zh-CN/start/hubs/)
- [FAQ](/docs/zh-CN/help/faq/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
