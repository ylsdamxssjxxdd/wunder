---
title: 参考概览
summary: 参考页提供稳定查表内容：接口索引、配置项、事件语义、工作区路由和管理面板映射。
read_when:
  - 你需要字段级、事件级、配置级参考
  - 你已经知道问题在哪一层，想快速查标准答案
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# 参考概览

这组页面面向“查表”和“落实现”，不是概念导读。

## 参考入口

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/reference/api-index/"><strong>API 索引</strong><span>按接口域快速定位端点。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/config/"><strong>配置说明</strong><span>运行配置、模型、存储、安全项。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/stream-events/"><strong>流式事件参考</strong><span>事件语义与终态判断。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/workspace-routing/"><strong>工作区路由</strong><span>`user_id/container_id/agent_id` 路由规则。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/prompt-templates/"><strong>提示词模板</strong><span>模板分段与生效时机。</span></a>
  <a class="docs-card" href="/docs/zh-CN/reference/admin-panels/"><strong>管理端面板索引</strong><span>功能到面板的映射。</span></a>
</div>

## 何时优先读参考页

- 你要确认字段和参数，而不是了解产品背景。
- 你在做联调，需要稳定口径。
- 你在排障，需要精确事件和配置依据。

## 常见误区

- 参考页不代替概念页，两者职责不同。
- 流式状态机不要只看 `final`，要结合事件语义。
- 工作区路由和提示词模板都应单独核对，不建议凭经验推断。

## 延伸阅读

- [概念概览](/docs/zh-CN/concepts/)
- [接入概览](/docs/zh-CN/integration/)
- [帮助中心](/docs/zh-CN/help/)
