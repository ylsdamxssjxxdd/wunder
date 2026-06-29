---
title: wunder 心舰
summary: wunder 是一个智能体系统，有桌面端、服务端、命令行三种用法。先按角色选入口。
read_when:
  - 第一次了解 wunder
  - 需要快速判断先看 desktop、server 还是 cli
  - 要定位接入、运维和工具文档入口
source_docs:
  - README.md
  - docs/设计文档/01-系统总体设计.md
---

# wunder 心舰

<p class="docs-eyebrow">Agent Orchestration Kernel | server / cli / desktop</p>

## wunder 概述

wunder 是一个会执行任务的智能体系统。用户给出目标后，系统自动拆解任务、调用工具、并行协作，最终交付结果。它具备文件与代码操作、办公自动化、多智能体协作、定时任务和系统集成等能力，可以作为个人或团队的工作台。

| 用法 | 适用对象 | 说明 |
|------|----------|------|
| **Desktop** | 个人用户 | 本地安装，独立运行 |
| **Server** | 团队/组织 | 多人共用，统一管理 |
| **CLI** | 开发者 | 终端运行，便于自动化 |

## 能力范围

- **文件与代码**：读取文件、编辑代码、执行命令、重构项目
- **办公自动化**：整理文档、生成报告、处理表格、做会议纪要
- **多智能体协作**：一个查资料、一个写稿、一个复核，并行加速
- **持续任务**：定时巡检、周期提醒、跨渠道消息处理
- **系统集成**：连接外部服务，把常用流程固化成技能

## 系统结构

![wunder 系统结构示意图](/docs/assets/manual/08-hierarchy-structure.svg)

系统结构从上到下是：

```
wunder
  └─ 用户（用户空间）
      └─ 蜂群（协作小组）
          └─ 智能体（执行角色）
              └─ 线程（一次连续的对话）
```

用户发消息 → 蜂群分配给合适的智能体 → 智能体在线程里持续执行。

## 按角色选入口

<div class="docs-card-grid">
  <a class="docs-card" href="/docs/zh-CN/start/desktop/">
    <strong>个人用户</strong>
    <span>下载 desktop，本地安装即用。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/server/">
    <strong>团队管理员</strong>
    <span>部署 server，统一管理用户与权限。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/cli/">
    <strong>开发者</strong>
    <span>用 CLI 做自动化和脚本集成。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/">
    <strong>系统集成</strong>
    <span>把 wunder 接入现有系统。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/">
    <strong>运维人员</strong>
    <span>部署、监控、性能调优。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/">
    <strong>工具开发者</strong>
    <span>探索内置工具、MCP、Skills。</span>
  </a>
</div>

## 核心特性

| 特性 | 说明 |
|------|------|
| **三种形态共用一套引擎** | 桌面端、服务端、命令行底层一样，体验一致 |
| **多用户与权限管理** | 用户、单位、Token 额度、权限分层管控 |
| **智能体协作** | 多个智能体分工协作，并行执行，结果汇总 |
| **丰富的工具生态** | 内置工具 + MCP 外部工具 + 技能包 + 知识库 |
| **开放接口** | WebSocket 实时通信、RESTful API、A2A 互操作标准 |

## 快速导航

### 第一次使用

先看 [快速开始](/docs/zh-CN/start/quickstart/)，完成第一个任务。

### 深入理解

从 [核心概览](/docs/zh-CN/concepts/) 开始。

### 接入开发

去 [接入概览](/docs/zh-CN/integration/)。

### 遇到问题

查 [故障排查](/docs/zh-CN/help/troubleshooting/)，或看 [FAQ](/docs/zh-CN/help/faq/)。

## 延伸阅读

- [说明书总览](/docs/zh-CN/start/hubs/)
- [API 索引](/docs/zh-CN/reference/api-index/)
- [系统介绍](/docs/设计文档/01-系统总体设计.md)
