---
title: 接入概览
summary: 把 wunder 接进现有系统前，先选对接口域：单次执行、聊天会话、工作区、临时转换、外部渠道、免登嵌入、A2A 与 MCP，各有各的入口。
---

# 接入概览

把 wunder 接进你自己的系统，先选对接口域，再记具体接口。

## 接口域对照

| 你要做的事 | 用哪个入口 | 为什么 |
| --- | --- | --- |
| 单次执行、轻量接入 | [wunder API](/docs/zh-CN/integration/wunder-api/) | 一步到位，一次请求出结果 |
| 做聊天产品、要会话管理 | [聊天会话](/docs/zh-CN/integration/chat-sessions/) + [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/) | 完整会话生命周期 + 实时推送 |
| 管理文件与产物 | [工作区 API](/docs/zh-CN/integration/workspace-api/) | 上传、下载、目录统一管理 |
| 文档转换、临时下载 | [临时目录接口](/docs/zh-CN/integration/temp-dir/) | doc/pdf 转 Markdown、临时文件分享 |
| 接外部渠道 | [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/) | 飞书、微信、QQ、XMPP 统一入口 |
| 接微信、要分辨新旧 | [微信 iLink 渠道](/docs/zh-CN/integration/weixin-channel/) | 分清新微信与旧微信，扫码接入 |
| 嵌入现有系统、免登 | [外部登录](/docs/zh-CN/integration/external-login/) | 免登录嵌入 |
| 智能体互调 | [A2A 接口](/docs/zh-CN/integration/a2a/) | 标准协议，跨系统协同 |
| 挂第三方工具 | [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/) | 挂载外部 MCP 服务 |

## 接入面总览

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/integration/wunder-api/">
    <strong>wunder API</strong>
    <span>最短路径执行入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/chat-sessions/">
    <strong>聊天会话</strong>
    <span>会话生命周期与消息接口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/chat-ws/">
    <strong>聊天 WebSocket</strong>
    <span>实时会话主通道。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/workspace-api/">
    <strong>工作区 API</strong>
    <span>文件、上传、写入、路由。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/temp-dir/">
    <strong>临时目录接口</strong>
    <span>文档转换与下载链路。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/user-world/">
    <strong>用户世界接口</strong>
    <span>系统内用户消息通路。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/channel-webhook/">
    <strong>渠道 Webhook</strong>
    <span>外部渠道入站统一入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/weixin-channel/">
    <strong>微信 iLink 渠道</strong>
    <span>新微信渠道接入、扫码登录与故障定位。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/external-login/">
    <strong>外部登录</strong>
    <span>外链免登和嵌入。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/a2a/">
    <strong>A2A 接口</strong>
    <span>智能体系统互联协议。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/mcp-endpoint/">
    <strong>MCP 入口</strong>
    <span>MCP 服务挂载与发现。</span>
  </a>
</div>

## 三个最常用的入口

### 单次执行：`/wunder`

适合单次任务、不需要会话、想先快速试一下能力。给它一个用户标识和一个问题，就直接拿结果，不用先开会话。

```bash
curl -X POST http://localhost:18000/wunder \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "test-user",
    "question": "帮我写一个 Hello World"
  }'
```

### 聊天控制：`/wunder/chat/*` + WebSocket

要做完整的聊天产品、需要历史记录和实时交互时用这一套。会话的创建、发消息、断线恢复都在这里。

接通顺序：

```
1. 创建或找到会话 → /wunder/chat/sessions
2. 建立 WebSocket → /wunder/chat/ws
3. 通过 WebSocket 发消息
4. 通过 WebSocket 收事件
```

### 文件产物：工作区 API

上传、下载、目录浏览、产物回传都走工作区。每个用户对应唯一一个固定的云端目录，路径一律是工作区相对路径，不会暴露服务器上的真实目录。

## 实时通道选 WebSocket

聊天和桌面实时会话统一用 `/wunder/chat/ws`。SSE 留给 A2A、管理员诊断这类非聊天的接口；聊天不要再做 SSE 降级。

## 几处容易忽略的约定

- **用户标识不用先注册**。`user_id` 可以是任意标识，注册用户只在登录和权限场景才需要。
- **token 数字是上下文占用，不等于账单**。看当前上下文占用和看累计消耗是两个不同的数。
- **线程的提示词首次确定后就固定**。后续轮次不再改写，这样大模型的提示词缓存才不会被反复打破。
- **长期记忆只在线程初始化时注入一次**，之后不会自动改写，需要时主动调记忆工具。

## 容易搞混的地方

- `/wunder` 是执行入口，不是完整的聊天方案。
- 聊天的实时状态只走 WebSocket；断线后先重连，再补事件或重新观察会话。
- 工作区和临时目录职责不同，不要混用。
- 调用前不必先注册用户。

## 下一步

- 想看所有接口 → [API 索引](/docs/zh-CN/reference/api-index/)
- 想了解事件 → [流式事件参考](/docs/zh-CN/reference/stream-events/)
- 遇到问题 → [故障排查](/docs/zh-CN/help/troubleshooting/)
