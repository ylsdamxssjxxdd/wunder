---
title: 快速开始
summary: 用最短路径跑通 wunder 的第一条主链路，优先推荐 desktop。
read_when:
  - 你第一次使用 wunder
  - 你想在 desktop、server、cli 中先跑通一条可用链路
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# 快速开始

这页只解决一件事：

- 用最短路径把 wunder 跑起来

如果你不想先研究架构，直接按这里走。

## 推荐顺序

1. 普通用户优先从 [Desktop 入门](/docs/zh-CN/start/desktop/) 开始
2. 团队部署或接口接入从 [Server 部署](/docs/zh-CN/start/server/) 开始
3. 开发者调试或脚本任务从 [CLI 使用](/docs/zh-CN/start/cli/) 开始

## 最短可用路径：Desktop

如果你只是想尽快用起来，建议走这条路径：

1. 启动 `wunder-desktop`
2. 确认本地模式可以打开聊天界面
3. 选择或配置一个可用模型
4. 在输入框里直接提出任务
5. 观察模型回复、工具调用、工作区产物与会话状态

为什么优先推荐 desktop：

- 模型调用链路
- 用户侧前端交互
- 工作区与本地文件能力
- 智能体设置与对话执行主链路

## 如果你要部署 Server

Server 更适合这些场景：

- 多用户、多单位、多租户
- 网页端统一访问
- 需要管理员侧治理能力
- 需要渠道、蜂群、用户管理、MCP 统一接入

直接阅读：

- [Server 部署](/docs/zh-CN/start/server/)
- [部署与运行](/docs/zh-CN/ops/deployment/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)

## 如果你要用 CLI

CLI 适合这些场景：

- 本地终端任务
- 脚本化调用
- 开发者调试
- 工作区驱动任务执行

直接阅读：

- [CLI 使用](/docs/zh-CN/start/cli/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [工具体系](/docs/zh-CN/concepts/tools/)

## 运行前你最需要知道的点

- wunder 有三种运行形态：`server / cli / desktop`
- `server` 是核心能力底座
- `desktop` 是当前主交付产品
- `cli` 适合开发与自动化
- 用户请求优先走 WebSocket，SSE 作为兜底

## 跑起来之后看什么

- [Desktop 入门](/docs/zh-CN/start/desktop/)
- [系统架构](/docs/zh-CN/concepts/architecture/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)

## 相关文档

- [Desktop 入门](/docs/zh-CN/start/desktop/)
- [Server 部署](/docs/zh-CN/start/server/)
- [CLI 使用](/docs/zh-CN/start/cli/)
