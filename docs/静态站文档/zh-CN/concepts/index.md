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

如果你经常把“会话、线程、工作区、智能体”混成一个词，这页先看。

## 先建立这四个锚点

1. `会话`：用户侧对话单元
2. `线程`：模型执行上下文单元
3. `工作区`：文件与产物隔离单元
4. `智能体`：执行角色与策略单元

这四个锚点分清后，绝大多数接口都会变得直观。

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
</div>

## 按问题跳转

### 我在做接入

- [流式执行](/docs/zh-CN/concepts/streaming/)
- [运行时与在线状态](/docs/zh-CN/concepts/presence-and-runtime/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)

### 我在做文件与产物

- [工作区与容器](/docs/zh-CN/concepts/workspaces/)
- [工具体系](/docs/zh-CN/concepts/tools/)

### 我在做治理与稳定性

- [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/)
- [长期记忆](/docs/zh-CN/concepts/memory/)
- [额度与 Token 占用](/docs/zh-CN/concepts/quota-and-token-usage/)

## 常见误区

- 工作区不是会话，线程也不是会话。
- 流式事件不是最终结果，终态应看 `turn_terminal`。
- Token 占用统计不是账单总消耗。

## 延伸阅读

- [接入概览](/docs/zh-CN/integration/)
- [运维概览](/docs/zh-CN/ops/)
