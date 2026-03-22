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

接入 Wunder 的关键不是「记住所有接口」，而是**先选对接口域**。

---

## 先做选型：你要做什么？

| 你的场景 | 推荐入口 | 为什么？ |
|----------|----------|----------|
| 单次执行、轻量接入 | [wunder API](/docs/zh-CN/integration/wunder-api/) | 最短路径，一次请求出结果 |
| 聊天产品化、会话管理 | [聊天会话](/docs/zh-CN/integration/chat-sessions/) + [聊天 WebSocket](/docs/zh-CN/integration/chat-ws/) | 完整会话生命周期 + 实时推送 |
| 文件管理、产物落地 | [工作区 API](/docs/zh-CN/integration/workspace-api/) | 上传、下载、路由统一管理 |
| 文档转换、临时下载 | [临时目录接口](/docs/zh-CN/integration/temp-dir/) | doc/pdf 转 md、临时文件分享 |
| 外部渠道接入 | [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/) | 飞书、微信、QQ、XMPP 统一入口 |
| 微信渠道选型（新旧区分） | [微信 iLink 渠道](/docs/zh-CN/integration/weixin-channel/) | 快速区分 `weixin` 与 `wechat/wechat_mp`，并给出扫码接入路径 |
| 系统嵌入、免登 | [外部登录](/docs/zh-CN/integration/external-login/) | 免登录嵌入到你的系统 |
| 智能体互联互通 | [A2A 接口](/docs/zh-CN/integration/a2a/) | 标准协议，跨系统协同 |
| 扩展工具生态 | [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/) | 挂载第三方 MCP 服务 |

---

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

---

## 三个高频入口详解

### 1. 统一执行入口：`/wunder`

**适合场景**：
- 单次任务执行
- 不需要会话管理
- 快速验证能力

**特点**：
- 输入：`user_id` + `question`
- 输出：流式事件 + 最终结果
- 不需要先建立会话

**示例**：
```bash
curl -X POST http://localhost:18000/wunder \
  -H "Content-Type: application/json" \
  -d '{
    "user_id": "test-user",
    "question": "帮我写一个 Hello World"
  }'
```

---

### 2. 聊天控制入口：`/wunder/chat/*` + WebSocket

**适合场景**：
- 完整的聊天产品
- 需要会话历史
- 实时交互体验

**特点**：
- 会话生命周期管理
- WebSocket 优先，SSE 兜底
- 支持断线恢复

**接入流程**：
```
1. 创建/获取会话 → /wunder/chat/sessions
2. 建立 WebSocket → /wunder/chat/ws
3. 发送消息 → 通过 WS 发送
4. 接收事件 → 通过 WS 接收
```

---

### 3. 文件产物入口：工作区 API

**适合场景**：
- 文件上传/下载
- 产物管理
- 工作区路由

**特点**：
- 按 `user_id` + `container_id` 隔离
- 支持路径映射
- 统一的文件操作接口

---

## 传输协议：WebSocket 优先，SSE 兜底

| 协议 | 优点 | 缺点 | 适用场景 |
|------|------|------|----------|
| **WebSocket** | 低延迟、双向、实时 | 需要保持连接 | 聊天 UI、实时协作 |
| **SSE** | 简单、兼容好 | 单向、延迟较高 | 兜底方案、简单消费 |

**建议**：优先实现 WebSocket，SSE 作为降级方案。

---

## 关键设计约定

### user_id 不要求注册

- 可以传任意虚拟标识
- 注册用户仅用于登录和权限管理
- 外部系统可以自由传入 user_id

### token 统计是上下文占用量

- 不是账单总消耗量
- 记录的是实际入模的上下文
- 用于评估和优化，不是计费

### 线程 system prompt 会冻结

- 首次确定后锁定
- 后续轮次不再改写
- 避免破坏大模型 API 的提示词缓存

### 长期记忆只注入一次

- 仅在线程初始化时注入
- 后续轮次不再自动改写
- 需要时主动调用记忆管理工具

---

## 常见误区澄清

| 误区 | 正确理解 |
|------|----------|
| `/wunder` 可以替代聊天域 | ❌ `/wunder` 是执行入口，不是完整聊天方案 |
| 只接 SSE 就够了 | ❌ 建议 WebSocket 优先，体验更好 |
| `workspaces` 和 `temp_dir` 可以混用 | ❌ 职责不同，存储策略不同 |
| 必须先注册用户才能调用 | ❌ user_id 可以是任意虚拟标识 |

---

## 下一步

- 想看所有 API？→ [API 索引](/docs/zh-CN/reference/api-index/)
- 想了解事件？→ [流式事件参考](/docs/zh-CN/reference/stream-events/)
- 遇到问题？→ [故障排查](/docs/zh-CN/help/troubleshooting/)
