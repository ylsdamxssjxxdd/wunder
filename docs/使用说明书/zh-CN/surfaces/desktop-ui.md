---
title: Desktop 界面
summary: `wunder-desktop` 是当前主交付形态，强调本地优先与桌面工作台。
read_when:
  - 你要直接使用 Wunder
  - 你要理解 desktop 为什么是当前产品重心
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# Desktop 界面

如果把 Wunder 看成一个产品，而不是一个后端项目，那么当前最重要的界面就是 desktop。

## 它是什么

`wunder-desktop` 不是单纯把网页包一层壳。

它的目标是把这些东西组合成“本地优先的智能体工作台”：

- 用户侧消息工作台
- 本地 bridge 服务
- 本地工作目录
- 本地运行模式
- 桌面能力接入

## 为什么 desktop 是当前重心

因为它能一次性验证 Wunder 的核心价值链：

- 对话入口
- 工具执行
- 工作区产物
- 本地文件能力
- 可视化智能体循环

## 当前特征

- 优先本地模式
- 默认使用安装包附带的 Python 运行时
- 支持本地持久工作目录
- 复用统一 `/wunder` 调度内核
- 不再提供桌面端内部的服务端连接切换

## 它和用户前端的关系

Desktop 尽量复用用户侧前端的同构页面结构。

也就是说：

- 页面交互逻辑大量来自 `frontend/`
- 桌面端新增的是本地桥接、系统设置、目录映射与运行形态能力

## 它更适合谁

- 普通个人用户
- 需要本地文件和桌面环境的人
- 想先用起来，而不是先部署完整 server 的人

## 如果需要服务端能力怎么办

桌面端现在只维护本地模式。

如果你需要这些能力，请直接使用浏览器访问 server 形态，而不是在 desktop 里切换连接模式：

- 多用户和多租户
- 管理员统一治理
- 组织级部署
- 渠道与网关统一接入

## 延伸阅读

- [Desktop 入门](/docs/zh-CN/start/desktop/)
- [用户侧前端](/docs/zh-CN/surfaces/frontend/)
- [部署与运行](/docs/zh-CN/ops/deployment/)
