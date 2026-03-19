---
title: 接入概览
summary: Wunder 的接入面不是只有 `/wunder`，还包括聊天域、工作区、渠道、A2A、MCP 和临时文件入口。
read_when:
  - 你准备把 Wunder 接到自己的系统里
  - 你不确定该从 `/wunder`、聊天域还是工作区接口开始
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# 接入概览

如果你把 Wunder 当成外部能力服务接入，先不要急着翻完整 API 文档，先从这个入口页确定“你要接哪一层”。

## 先看这些

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/integration/wunder-api/">
    <strong>wunder API</strong>
    <span>统一执行入口，最短路径接入。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/chat-ws/">
    <strong>聊天 WebSocket</strong>
    <span>聊天主实时通道，适合工作台和桌面会话。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/chat-sessions/">
    <strong>聊天会话</strong>
    <span>会话创建、消息发送、事件、恢复、取消都在这里。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/workspace-api/">
    <strong>工作区 API</strong>
    <span>文件浏览、上传、写入、归档和容器路由入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/channel-webhook/">
    <strong>渠道 Webhook</strong>
    <span>飞书、微信、QQBot、WhatsApp、XMPP 等入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/mcp-endpoint/">
    <strong>MCP 入口</strong>
    <span>自托管 MCP 与外部工具接入方式。</span>
  </a>
</div>

## 按目标找页面

### 你要接一个通用调用方

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [流式执行](/docs/zh-CN/concepts/streaming/)

### 你要做聊天界面

- [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)

### 你要做文件与产物流转

- [工作区 API](/docs/zh-CN/integration/workspace-api/)
- [临时目录与文档转换](/docs/zh-CN/integration/temp-dir/)

### 你要接外部系统或平台

- [用户世界接口](/docs/zh-CN/integration/user-world/)
- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)
- [外部登录与免登嵌入](/docs/zh-CN/integration/external-login/)
- [A2A 接口](/docs/zh-CN/integration/a2a/)

## 你最需要记住的点

- `/wunder` 是统一执行入口，但不是完整聊天域。
- 真正做聊天 UI 时，通常要一起使用聊天会话接口和 WebSocket。
- 文件路径治理要先分清工作区和 `temp_dir`。
- 渠道接入、A2A、MCP 都是正式接入面，不是附属功能。

## 相关文档

- [参考概览](/docs/zh-CN/reference/)
- [API 索引](/docs/zh-CN/reference/api-index/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
