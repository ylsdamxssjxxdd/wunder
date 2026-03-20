---
title: 接入概览
summary: Wunder 的接入面包含统一执行入口、聊天域、工作区、渠道、A2A 与 MCP；先分层选型，接入会更稳。
read_when:
  - 你准备把 Wunder 接入业务系统
  - 你不确定该从 `/wunder`、聊天域还是工作区接口开始
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# 接入概览

接入 Wunder 的关键不是“记住所有接口”，而是先选对接口域。

## 三个高频入口

1. **统一执行入口：**[wunder API](/docs/zh-CN/integration/wunder-api/)
2. **聊天控制入口：**[聊天会话](/docs/zh-CN/integration/chat-sessions/) + [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)
3. **文件产物入口：**[工作区 API](/docs/zh-CN/integration/workspace-api/)

## 接入面总览

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/integration/wunder-api/"><strong>wunder API</strong><span>最短路径执行入口。</span></a>
  <a class="docs-card" href="/docs/zh-CN/integration/chat-sessions/"><strong>聊天会话</strong><span>会话生命周期与消息接口。</span></a>
  <a class="docs-card" href="/docs/zh-CN/integration/chat-ws/"><strong>聊天 WebSocket</strong><span>实时会话主通道。</span></a>
  <a class="docs-card" href="/docs/zh-CN/integration/workspace-api/"><strong>工作区 API</strong><span>文件、上传、写入、路由。</span></a>
  <a class="docs-card" href="/docs/zh-CN/integration/temp-dir/"><strong>临时目录接口</strong><span>文档转换与下载链路。</span></a>
  <a class="docs-card" href="/docs/zh-CN/integration/user-world/"><strong>用户世界接口</strong><span>系统内用户消息通路。</span></a>
  <a class="docs-card" href="/docs/zh-CN/integration/channel-webhook/"><strong>渠道 Webhook</strong><span>外部渠道入站统一入口。</span></a>
  <a class="docs-card" href="/docs/zh-CN/integration/external-login/"><strong>外部登录</strong><span>外链免登和嵌入。</span></a>
  <a class="docs-card" href="/docs/zh-CN/integration/a2a/"><strong>A2A 接口</strong><span>智能体系统互联协议。</span></a>
  <a class="docs-card" href="/docs/zh-CN/integration/mcp-endpoint/"><strong>MCP 入口</strong><span>MCP 服务挂载与发现。</span></a>
</div>

## 选型判断

- 单次执行或轻接入：优先 `/wunder`
- 聊天产品化接入：优先 `/wunder/chat/*` + `/wunder/chat/ws`
- 文件产物平台化：优先工作区与 temp-dir 相关接口
- 外部智能体协同：优先 A2A
- 工具生态扩展：优先 MCP

## 常见误区

- `/wunder` 不是聊天域的完整替代。
- 聊天 UI 只接 SSE 往往不够，建议 WS 优先。
- `workspaces` 和 `temp_dir` 是不同职责，不要混用存储策略。

## 延伸阅读

- [API 索引](/docs/zh-CN/reference/api-index/)
- [流式事件参考](/docs/zh-CN/reference/stream-events/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
