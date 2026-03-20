---
title: 帮助
summary: 如果你想更快地修问题，先从这里选正确的排障入口，而不是到处翻整站文档。
read_when:
  - 你想知道出问题时先看哪一页
  - 你想快速找到 FAQ、术语和常见修复路径
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - docs/静态站文档/zh-CN/help/troubleshooting.md
---

# 帮助

如果你的目标是“尽快定位问题”，从这里开始。

这页不负责解释所有原理，只负责把你送到正确的排障入口。

## 先看这些

- **故障排查：**[从这里开始](/docs/zh-CN/help/troubleshooting/)
- **常见问题：**[FAQ](/docs/zh-CN/help/faq/)
- **术语说明：**[术语表](/docs/zh-CN/help/glossary/)

## 最先判断什么

- 这是使用疑问，还是系统故障
- 这是接入问题，还是运行问题
- 这是文件链路问题，还是聊天/渠道问题

## 按问题找入口

### 接口或状态机异常

- [流式事件参考](/docs/zh-CN/reference/stream-events/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)

### 文件或下载链路异常

- [工作区 API](/docs/zh-CN/integration/workspace-api/)
- [临时目录与文档转换](/docs/zh-CN/integration/temp-dir/)

### 渠道接入异常

- [渠道运行态](/docs/zh-CN/ops/channel-runtime/)
- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)

### 配置或治理问题

- [配置说明](/docs/zh-CN/reference/config/)
- [管理端面板索引](/docs/zh-CN/reference/admin-panels/)

## 最容易搞错的点

- 先判断问题属于概念问题、接入问题还是运行问题。
- 很多“模型异常”其实是渠道、工作区或状态机问题。
- FAQ 适合快速判断，troubleshooting 适合按链路排查。

## 相关文档

- [文档总览](/docs/zh-CN/start/hubs/)
- [参考概览](/docs/zh-CN/reference/)
