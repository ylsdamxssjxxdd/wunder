---
title: "概念概览"
summary: "如果你还没分清线程、工作区、工具、提示词和运行时状态，先从这里建立 Wunder 的基础模型。"
read_when:
  - "你第一次系统性理解 Wunder"
  - "你不知道该先读哪一个概念页"
source_docs:
  - "docs/系统介绍.md"
  - "docs/设计方案.md"
  - "docs/API文档.md"
---

# 概念概览

如果你只想先看一页概念入口，就看这里。

## 先看这些

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/concepts/architecture/">
    <strong>系统架构</strong>
    <span>先分清 server、调度层、工具层和多端界面。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/workspaces/">
    <strong>工作区与容器</strong>
    <span>理解 user_id、container_id 和文件隔离。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/sessions-and-rounds/">
    <strong>会话与轮次</strong>
    <span>分清用户轮次和模型轮次，不要把长执行看成一步。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/streaming/">
    <strong>流式执行</strong>
    <span>理解事件流不是“边打字”，而是整个运行态投影。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/tools/">
    <strong>工具体系</strong>
    <span>内置工具、MCP、技能和用户工具如何共同挂载。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/prompt-and-skills/">
    <strong>提示词与技能</strong>
    <span>线程冻结、模板包和 skill_call 的最短入口。</span>
  </a>
</div>

## 按问题找页面

### 你在做接入

- [流式执行](/docs/zh-CN/concepts/streaming/)
- [运行时与在线状态](/docs/zh-CN/concepts/presence-and-runtime/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)

### 你在做文件与产物

- [工作区与容器](/docs/zh-CN/concepts/workspaces/)
- [工具体系](/docs/zh-CN/concepts/tools/)

### 你在做治理与稳定性

- [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/)
- [长期记忆](/docs/zh-CN/concepts/memory/)
- [额度与 Token 占用](/docs/zh-CN/concepts/quota-and-token-usage/)

### 你在看多智能体

- [蜂群协作](/docs/zh-CN/concepts/swarm/)
- [系统架构](/docs/zh-CN/concepts/architecture/)

## 你最需要记住的点

- 工作区、会话、线程和智能体不是同一个概念。
- 线程 system prompt 一旦在首次确定后冻结，就不会在后续轮次被重写。
- 流式接入时，终结语义看 `turn_terminal`，运行态看 `thread_status` 或会话 `runtime`。
- Token 统计记录的是上下文占用，不是账单总消耗量。

## 相关文档

- [接入概览](/docs/zh-CN/integration/)
- [运维概览](/docs/zh-CN/ops/)
- [参考概览](/docs/zh-CN/reference/)
