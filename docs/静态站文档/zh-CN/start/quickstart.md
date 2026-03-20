---
title: 快速开始
summary: 用最短路径跑通 wunder 的第一条可用链路；默认推荐 desktop，其次按 server 或 cli 分流。
read_when:
  - 你第一次使用 wunder
  - 你需要在 10 分钟内跑通一个可验证结果
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# 快速开始

这页只做一件事：帮你选对起步路径，并快速跑通第一条主链路。

## 第一步：选运行形态

1. 个人直接使用：走 [Desktop 入门](/docs/zh-CN/start/desktop/)
2. 团队服务部署：走 [Server 部署](/docs/zh-CN/start/server/)
3. 终端与自动化：走 [CLI 使用](/docs/zh-CN/start/cli/)

## 最短路径：Desktop

1. 启动 `wunder-desktop`
2. 打开聊天界面，确认本地模式可用
3. 配置可用模型
4. 发一条测试消息
5. 检查是否能看到：中间过程、工具调用、最终回复

如果这 5 步都通过，说明你的核心执行链路已经可用。

## 团队路径：Server

适合这些场景：

- 多用户与多单位
- 管理员治理、应用发布、权限控制
- 渠道接入（Webhook/长连接）

建议阅读顺序：

1. [Server 部署](/docs/zh-CN/start/server/)
2. [部署与运行](/docs/zh-CN/ops/deployment/)
3. [认证与安全](/docs/zh-CN/ops/auth-and-security/)

## 开发路径：CLI

适合这些场景：

- 本地研发和脚本任务
- 工具链调试
- 自动化执行

建议阅读顺序：

1. [CLI 使用](/docs/zh-CN/start/cli/)
2. [工具总览](/docs/zh-CN/tools/)
3. [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)

## 验收清单

- 可以成功发起一次执行
- 可以看到流式过程与终态
- 能区分当前运行形态是 `desktop / server / cli`
- 能定位下一步要看的文档入口

## 延伸阅读

- [文档总览](/docs/zh-CN/start/hubs/)
- [接入概览](/docs/zh-CN/integration/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
