---
title: 参考概览
summary: 参考页提供稳定查表内容，也收纳按主题拆开的运行模型参考页，适合在核心总览之后深入细节。
read_when:
  - 你需要字段级、事件级、配置级参考
  - 你已经知道问题在哪一层，想快速查标准答案
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# 参考概览

这组页面面向“查表”和“落实现”。核心页负责建立主骨架，这里负责把字段、事件、配置和拆开的运行模型讲清楚。

## 参考入口

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/reference/api-index/"><strong>API 索引</strong><span>按接口域快速定位端点。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/config/"><strong>配置说明</strong><span>运行配置、模型、存储、安全项。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/stream-events/"><strong>流式事件参考</strong><span>事件语义与终态判断。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/workspace-routing/"><strong>工作区路由</strong><span>`user_id/container_id/agent_id` 路由规则。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/prompt-templates/"><strong>提示词模板</strong><span>模板分段与生效时机。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/admin-panels/"><strong>管理端面板索引</strong><span>功能到面板的映射。</span></a>
</div>

## 运行模型参考

这组页面来自原先的概念部分，适合在看完 [核心概览](/docs/zh-CN/concepts/) 后按主题深入。

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/concepts/architecture/"><strong>系统架构</strong><span>分层、模块边界和核心链路。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/workspaces/"><strong>工作区与容器</strong><span>文件隔离、路由和容器语义。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/sessions-and-rounds/"><strong>会话与轮次</strong><span>用户轮次、模型轮次与线程关系。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/streaming/"><strong>流式执行</strong><span>事件流、终态和恢复机制。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/presence-and-runtime/"><strong>运行时与在线状态</strong><span>忙闲、在线状态与运行态。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/tools/"><strong>工具体系</strong><span>内置工具、MCP、Skills 的统一视角。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/prompt-and-skills/"><strong>提示词与技能</strong><span>线程冻结、技能挂载和生效边界。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/memory/"><strong>长期记忆</strong><span>记忆注入、提炼与召回约束。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/quota-and-token-usage/"><strong>额度与 Token 占用</strong><span>成本、配额与上下文预算。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/swarm/"><strong>蜂群协作</strong><span>母蜂、工蜂和结果归并。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/boundary-handling/"><strong>边界处理</strong><span>超限、故障和恢复路径。</span></a>
</div>

## 何时优先读参考页

- 你要确认字段和参数，而不是了解产品背景。
- 你在做联调，需要稳定口径。
- 你在排障，需要精确事件和配置依据。

## 常见误区

- 参考页不代替核心页，两者职责不同。
- 流式状态机不要只看 `final`，要结合事件语义。
- 工作区路由和提示词模板都应单独核对，不建议凭经验推断。

## 延伸阅读

- [核心概览](/docs/zh-CN/concepts/)
- [接入概览](/docs/zh-CN/integration/)
- [帮助中心](/docs/zh-CN/help/)
