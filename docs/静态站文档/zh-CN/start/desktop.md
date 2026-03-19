---
title: Desktop 入门
summary: wunder-desktop 是当前主推形态，适合个人用户与本地优先使用。
read_when:
  - 你要先把 wunder 用起来
  - 你更关心桌面工作台，而不是先搭整套服务
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# Desktop 入门

`wunder-desktop` 是当前 wunder 的主交付形态。

它强调本地优先、桌面工作台、低门槛启动，以及在需要时接入远端 server。

## 适合谁

- 想直接使用智能体而不是先部署整套服务的人
- 需要本地文件、桌面窗口和聊天工作台的人
- 希望未来再接远端网关的人

## Desktop 的核心特点

- 默认优先本地运行
- 提供完整用户侧界面
- 支持工作区与本地目录映射
- 支持在本地模式与远端 gateway 之间切换
- 桌面端依旧复用统一的 `/wunder` 调度内核

## 你会看到什么

Desktop 打开后，主要是一个统一消息工作台：

- 左侧：导航与功能入口
- 中间：会话、联系人、智能体或列表视图
- 右侧：聊天主区域、工作流、设置与扩展面板

这套界面不是单纯聊天，而是承载：

- 用户与智能体会话
- 用户世界通信
- 工作区与文件
- 智能体设置
- 本地模式与远端接入设置

## Desktop 模式下的重要约定

- 本地模式优先使用附带的 Python 运行时
- 本地工作目录与容器目录默认按持久目录处理，不做 24 小时自动清理
- 用户私有工作目录与智能体容器目录是不同的作用域
- 工作区文件能力会直接影响聊天中的智能体可操作范围

## 什么时候切远端

如果你只是个人使用，本地模式通常已经够用。

如果你遇到这些需求，再考虑接远端：

- 需要组织级用户管理
- 需要统一管理员后台
- 需要团队协作和多租户治理
- 需要把桌面端作为 server 的一个接入端

## 推荐下一步

- [系统架构](/docs/zh-CN/concepts/architecture/)
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
- [部署与运行](/docs/zh-CN/ops/deployment/)

## 相关文档

- [快速开始](/docs/zh-CN/start/quickstart/)
- [桌面界面](/docs/zh-CN/surfaces/desktop-ui/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
