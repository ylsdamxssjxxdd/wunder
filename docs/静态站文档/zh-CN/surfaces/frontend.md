---
title: 用户侧前端
summary: 用户侧前端位于 `frontend/`，已经收敛为统一 Messenger 工作台。
read_when:
  - 你要理解 Wunder 面向普通用户的主要界面
  - 你要知道聊天、用户世界、工具、文件和设置如何组织
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - README.md
---

# 用户侧前端

Wunder 的用户侧前端不是“一个聊天页”，而是一个统一消息工作台。

它当前已经按 Messenger 形态组织，核心目标是把这些东西放进同一套壳里：

- 智能体聊天
- 用户世界通信
- 文件与工作区
- 工具与知识配置
- 系统设置与个人资料

## 代码位置

- `frontend/`

## 当前的主要入口

- `/app/home`
- `/app/chat`
- `/app/user-world`
- `/app/workspace`
- `/app/tools`
- `/app/settings`
- `/app/profile`
- `/app/channels`
- `/app/cron`

这些页面不是彼此割裂的，它们共享同一套 Messenger 壳。

## 界面组织方式

用户侧当前采用三栏结构：

- 左栏：导航与一级入口
- 中栏：会话、联系人、群聊、智能体或资源列表
- 右栏：聊天主区与扩展面板

这意味着 Wunder 的“用户侧前端”已经不是传统控制台，而是面向持续协作的工作台。

## 这套前端承载什么能力

- 普通对话
- 长任务流式回放
- 工具调用工作流
- 用户世界实时通信
- 文件预览和工作区操作
- 智能体设置
- 记忆碎片管理
- 渠道与定时任务入口

## 它和 Desktop 的关系

Desktop 并没有重新发明一套页面，而是尽量复用这套用户侧前端壳。

所以可以把关系理解成：

- `frontend/` 是核心交互壳
- `desktop/` 是把这套壳带到本地桌面并加上本地桥接能力

## 什么时候你应该先看这部分文档

适合这些场景：

- 你要改用户体验或页面结构
- 你要理解普通用户究竟看到什么
- 你要判断某个能力应该落在聊天区、工作区还是设置区

## 相关文档

- [Desktop 界面](/docs/zh-CN/surfaces/desktop-ui/)
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
