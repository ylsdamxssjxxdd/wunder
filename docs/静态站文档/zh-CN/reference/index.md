---
title: 参考概览
summary: 参考页不是再讲一遍产品故事，而是给你稳定的接口、配置、事件和面板索引。
read_when:
  - 你要快速定位稳定参考文档
  - 你不想直接翻完整 API 文档
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# 参考概览

如果你已经知道 Wunder 是什么，现在需要的是“稳定查表页”，从这里开始。

## 先看这些

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/reference/api-index/">
    <strong>API 索引</strong>
    <span>先按接口族找入口，再决定是否翻完整 API 手册。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/reference/config/">
    <strong>配置说明</strong>
    <span>运行配置、模型、存储和安全项的总入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/reference/stream-events/">
    <strong>流式事件参考</strong>
    <span>流式状态机最容易依赖的一页。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/reference/workspace-routing/">
    <strong>工作区路由</strong>
    <span>明确 `container_id`、`agent_id` 和 scoped user_id 的优先级。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/reference/prompt-templates/">
    <strong>提示词模板</strong>
    <span>模板包、分段文件和生效时机的参考页。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/reference/admin-panels/">
    <strong>管理端面板索引</strong>
    <span>按面板快速找管理端能力。</span>
  </a>
</div>

## 按问题找页面

### 你在找接口

- [API 索引](/docs/zh-CN/reference/api-index/)
- [流式事件参考](/docs/zh-CN/reference/stream-events/)

### 你在找配置或模板

- [配置说明](/docs/zh-CN/reference/config/)
- [提示词模板参考](/docs/zh-CN/reference/prompt-templates/)

### 你在找管理入口

- [管理端面板索引](/docs/zh-CN/reference/admin-panels/)
- [工作区路由参考](/docs/zh-CN/reference/workspace-routing/)

## 你最需要记住的点

- 参考页偏“稳定查表”，不是概念页。
- 接入状态机优先配合流式事件参考阅读。
- 工作区路由和提示词模板都值得单独看，不要只靠概念页理解。

## 相关文档

- [概念概览](/docs/zh-CN/concepts/)
- [接入概览](/docs/zh-CN/integration/)
- [帮助](/docs/zh-CN/help/)
