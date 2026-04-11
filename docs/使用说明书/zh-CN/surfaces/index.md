---
title: 界面概览
summary: Wunder 当前同时维护用户侧前端、管理端前端和桌面端三块界面表面，它们共享同一套后端能力，但职责不同。
read_when:
  - 你想快速分清用户端、管理端和 desktop 各自做什么
  - 你在找某一个功能应该去哪个界面看
source_docs:
  - docs/系统介绍.md
  - docs/API文档.md
  - docs/设计方案.md
---

# 界面概览

wunder 不是单页聊天产品，而是三块界面一起工作。

如果你在找“某一个功能应该去哪里看”，先从这里分流。

## 快速入口

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/surfaces/frontend/">
    <strong>用户侧前端</strong>
    <span>聊天、用户世界、工作区、工具和设置的统一工作台。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/web-admin/">
    <strong>管理端界面</strong>
    <span>模型、用户、渠道、工具、监控和 benchmark 治理入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/desktop-ui/">
    <strong>Desktop 界面</strong>
    <span>本地主交付形态，强调本地优先和桌面工作台体验。</span>
  </a>
</div>

## 三块界面的职责

- 用户端：偏会话、文件、联系人和工具操作
- 管理端：偏治理、配置、监控和评估
- Desktop：偏本地优先和桌面能力

## 按角色看

### 普通用户

优先看：

- [用户侧前端](/docs/zh-CN/surfaces/frontend/)
- [Desktop 界面](/docs/zh-CN/surfaces/desktop-ui/)

### 管理员

优先看：

- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
- [管理端面板索引](/docs/zh-CN/reference/admin-panels/)

### 集成开发者

除了界面页，还应该一起看：

- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [工作区 API](/docs/zh-CN/integration/workspace-api/)

## 常见误区

- 用户端强调会话体验和文件智能体循环。
- 管理端强调治理、监控、配置和评估。
- Desktop 是当前主交付形态，但现在只维护本地模式，不再在桌面端内部切换到 server。

## 延伸阅读

- [开始](/docs/zh-CN/start/desktop/)
- [运维概览](/docs/zh-CN/ops/)
- [帮助](/docs/zh-CN/help/)
