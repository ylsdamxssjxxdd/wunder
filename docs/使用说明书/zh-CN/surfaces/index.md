---
title: 界面概览
summary: Wunder 有三个界面：用户端、管理端、桌面端。用户端做日常工作，管理端做系统治理，桌面端是个人主入口。
---

# 界面概览

Wunder 有三个主要界面，各有各的职责：

| 界面 | 用途 | 适合谁 |
|------|------|--------|
| **用户端** | 日常对话、文件管理、智能体设置 | 所有用户 |
| **管理端** | 系统配置、用户管理、渠道管理 | 管理员 |
| **桌面端** | 本地优先的个人工作台 | 个人用户 |

## 界面入口

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/surfaces/frontend/"><strong>用户端界面</strong><span>聊天、文件、智能体、工具、设置。</span></a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/orchestration/"><strong>编排功能</strong><span>母蜂、工蜂、产物与时间线的专用工作台。</span></a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/web-admin/"><strong>管理端界面</strong><span>用户管理、系统配置、渠道监控。</span></a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/desktop-ui/"><strong>桌面端界面</strong><span>本地工作台，桌面端特有功能。</span></a>
</div>

## 界面关系

### 用户端 vs 管理端

- **用户端**：做具体工作——对话、处理文件、使用工具
- **管理端**：做系统治理——管理用户、配置系统、监控运行

职责不同，所以界面分开。普通用户不需要关心管理端，管理员日常工作也在用户端完成。

### 桌面端 vs 用户端

桌面端在用户端基础上增加了：
- 本地文件直接访问
- 本地运行时配置
- 桌面系统集成
- 一键重置功能

可以说桌面端是"增强版的用户端"。

### 三栏布局

用户端和桌面端都采用三栏布局：

```
┌────────┬─────────────┬──────────────────┐
│ 左栏   │ 中栏        │ 右栏             │
│ 导航   │ 列表        │ 工作区           │
│        │ 会话/文件   │ 对话/详情        │
└────────┴─────────────┴──────────────────┘
```

- **左栏**：一级导航（聊天、文件、智能体、工具、设置等）
- **中栏**：列表（会话列表、文件列表、智能体列表等）
- **右栏**：工作区（对话详情、文件预览、设置面板等）

## 关键工作台

- [用户端界面](/docs/zh-CN/surfaces/frontend/)：统一消息工作台，覆盖聊天、文件、智能体、工具与设置
- [编排功能](/docs/zh-CN/surfaces/orchestration/)：蜂群进入编排态后的专用工作台，适合连续推进、看轮次快照与分支
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)：系统治理后台
- [桌面端界面](/docs/zh-CN/surfaces/desktop-ui/)：本地优先的个人入口

## 按角色选择

### 普通用户

主要使用：
- 桌面应用（推荐）
- 或网页端用户端

### 管理员

主要使用：
- 管理端（做治理）
- 用户端或桌面端（做日常工作）

## 常见误区

- **管理端能聊天？** 不行。管理端是治理后台，不做日常对话。
- **桌面端能切换连接服务器？** 当前版本专注本地模式。如需 Server 能力请用网页端。
- **用户端和管理端共用一套界面？** 不行。职责不同，界面分开。

## 延伸阅读

- [Desktop 入门](/docs/zh-CN/start/desktop/)
- [编排功能](/docs/zh-CN/surfaces/orchestration/)
- [管理端面板指南](/docs/zh-CN/reference/admin-panels/)
- [核心概览](/docs/zh-CN/concepts/)
