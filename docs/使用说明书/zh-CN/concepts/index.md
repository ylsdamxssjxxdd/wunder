---
title: 概念概览
summary: 先建立 Wunder 的基础运行模型，再阅读接口和工具细节，会明显降低接入与排障成本。
read_when:
  - 你第一次系统性理解 Wunder
  - 你在接入或排障时遇到概念混淆
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# 概念概览

如果你经常把「会话、线程、工作区、智能体」混成一个词，这页先看。

## 先建立四个锚点

在深入细节之前，先把这四个核心概念分清楚：

| 概念 | 是什么？ | 核心职责 |
|------|----------|----------|
| **会话** | 用户侧对话单元 | 承载用户与智能体的交互历史 |
| **线程** | 模型执行上下文单元 | 维护连续的执行状态和记忆 |
| **工作区** | 文件与产物隔离单元 | 管理文件、工具可操作范围 |
| **智能体** | 执行角色与策略单元 | 定义人格、提示词、可用工具 |

这四个锚点分清后，绝大多数接口都会变得直观。

---

## 核心关系图

```
用户 (user_id)
  └─ 蜂群 (hive_id) ← 协作分组
      └─ 智能体 (agent_id) ← 执行角色
          ├─ 主线程 (agent_thread) ← 默认执行上下文
          │   └─ 主会话 (session_id, is_main=true)
          └─ 历史会话 (chat_sessions)
              └─ 子线程 (可选)

工作区 (workspace)
  └─ 用户容器 (container_id=0) ← 用户私有文件
  └─ 智能体容器 (container_id=1~10) ← 智能体运行空间
```

---

## 从一个请求看概念流转

让我们通过一次完整的请求，看看这些概念如何配合：

```
1. 用户发消息 → 绑定到「用户 + 智能体」
   ↓
2. 系统找到「主线程」，或创建新「会话」
   ↓
3. 加载「工作区」文件，注入「长期记忆」
   ↓
4. 「线程」执行：调用 LLM → 调用工具 → 循环
   ↓
5. 写入「会话」历史，更新「线程」状态
   ↓
6. 通过 WebSocket/SSE 流式推送事件
```

---

## 按问题找概念

### 我在做接入开发

| 问题 | 看这个概念 |
|------|------------|
| 如何建立连接、接收流式事件？ | [流式执行](/docs/zh-CN/concepts/streaming/) |
| 如何知道智能体当前是忙还是闲？ | [运行时与在线状态](/docs/zh-CN/concepts/presence-and-runtime/) |
| 用户发了多条消息，怎么算轮次？ | [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/) |

### 我在做文件与产物

| 问题 | 看这个概念 |
|------|------------|
| 智能体能看到哪些文件？ | [工作区与容器](/docs/zh-CN/concepts/workspaces/) |
| 工具有哪些？怎么用？ | [工具体系](/docs/zh-CN/concepts/tools/) |

### 我在做治理与稳定性

| 问题 | 看这个概念 |
|------|------------|
| 提示词能动态改吗？ | [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/) |
| 如何让智能体记住长期信息？ | [长期记忆](/docs/zh-CN/concepts/memory/) |
| 怎么控制成本和配额？ | [额度与 Token 占用](/docs/zh-CN/concepts/quota-and-token-usage/) |
| 上下文超限了怎么办？ | [边界处理](/docs/zh-CN/concepts/boundary-handling/) |
| 网络中断如何恢复？ | [边界处理](/docs/zh-CN/concepts/boundary-handling/) |
| 工具失败了怎么处理？ | [边界处理](/docs/zh-CN/concepts/boundary-handling/) |

---

## 核心概念入口

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/concepts/architecture/">
    <strong>系统架构</strong>
    <span>看分层与模块边界。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/workspaces/">
    <strong>工作区与容器</strong>
    <span>理解 `user_id/container_id/agent_id` 路由关系。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/sessions-and-rounds/">
    <strong>会话与轮次</strong>
    <span>区分用户轮次和模型轮次。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/streaming/">
    <strong>流式执行</strong>
    <span>看事件流、终态和恢复机制。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/tools/">
    <strong>工具体系</strong>
    <span>内置工具、MCP、Skills 的统一视角。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/prompt-and-skills/">
    <strong>提示词与技能</strong>
    <span>线程冻结和技能挂载边界。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/memory/">
    <strong>长期记忆</strong>
    <span>结构化记忆碎片与召回机制。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/swarm/">
    <strong>蜂群协作</strong>
    <span>多智能体分工与结果归并。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/boundary-handling/">
    <strong>边界处理</strong>
    <span>上下文超限、网络中断、错误恢复。</span>
  </a>
</div>

---

## 常见误区澄清

在继续之前，先澄清几个最容易踩坑的点：

| 误区 | 正确理解 |
|------|----------|
| 工作区 = 会话 | ❌ 工作区是文件空间，会话是对话历史 |
| 线程 = 会话 | ❌ 线程是执行上下文，会话是交互记录 |
| 流式事件 = 最终结果 | ❌ 要等 `turn_terminal` 才是终态 |
| Token 统计 = 账单消耗 | ❌ 记录的是**上下文占用量**，不是总消耗量 |
| 每次都重写 system prompt | ❌ 首次确定后会**冻结**，后续不再改写 |
| 长期记忆每轮都注入 | ❌ 只在线程**初始化时注入一次** |
| 上下文超限就会崩溃 | ❌ 系统有自动压缩机制，不会崩溃 |
| 网络断开连接就丢失数据 | ❌ 有重连和事件补发机制 |
| 工具失败就任务失败 | ❌ 有重试和降级策略 |

---

## 延伸阅读

- [接入概览](/docs/zh-CN/integration/)
- [运维概览](/docs/zh-CN/ops/)
- [工具总览](/docs/zh-CN/tools/)
- [边界处理](/docs/zh-CN/concepts/boundary-handling/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
