---
title: wunder
summary: wunder 是统一承载 server、cli、desktop 三种运行形态的智能体调度系统；入口建议先按角色和目标分流，再进入具体文档。
read_when:
  - 你第一次了解 wunder
  - 你需要快速判断先看 desktop、server 还是 cli
  - 你要定位接入、运维和工具文档入口
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# wunder

<p class="docs-eyebrow">Agent Orchestration Kernel | server / cli / desktop</p>

wunder 的核心不是“一个聊天页面”，而是一套可运行在多形态下的智能体调度内核。

你可以把它理解为：同一套执行系统，对外提供 `desktop`、`server`、`cli` 三条使用路径。

## 先判断你的入口

- 个人用户先看 [Desktop 入门](/docs/zh-CN/start/desktop/)
- 团队部署先看 [Server 部署](/docs/zh-CN/start/server/)
- 终端自动化先看 [CLI 使用](/docs/zh-CN/start/cli/)
- 系统接入先看 [接入概览](/docs/zh-CN/integration/)
- 上线与治理先看 [运维概览](/docs/zh-CN/ops/)

## 系统结构（3 分钟版）

![wunder 系统结构示意图：心舰到用户、蜂群、智能体、线程的分层关系](/docs/diagrams/system-intro/08-hierarchy-structure.svg)

看图时重点抓这条主线：

- `用户`：资源隔离边界
- `蜂群`：协作单元
- `智能体`：执行角色
- `线程`：连续上下文与状态承载体

一句话：**请求先落到用户域，再由蜂群编排智能体，最终在线程中持续执行。**

## 按任务进入文档

<div class="docs-card-grid">
  <a class="docs-card" href="/docs/zh-CN/start/quickstart/">
    <strong>快速开始</strong>
    <span>先跑通一条可用链路，再补概念。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/concepts/">
    <strong>概念概览</strong>
    <span>补齐线程、工作区、工具和运行态模型。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/">
    <strong>工具总览</strong>
    <span>按任务选工具，不靠猜。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/">
    <strong>接入概览</strong>
    <span>判断该接 `/wunder`、聊天域、MCP 还是 A2A。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/">
    <strong>运维概览</strong>
    <span>部署、存储、安全、观测和渠道运行态。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/help/">
    <strong>帮助中心</strong>
    <span>按症状定位问题并快速回收。</span>
  </a>
</div>

## 常见误判

- `/wunder` 的 `user_id` 不要求是已注册用户。
- token 统计记录的是上下文占用量，不是账单总消耗量。
- 线程首次确定的 system prompt 会冻结，后续轮次不会重写。
- 长期记忆只在线程初始化时注入一次。

## 延伸阅读

- [文档总览](/docs/zh-CN/start/hubs/)
- [API 索引](/docs/zh-CN/reference/api-index/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
