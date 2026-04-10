---
title: wunder 心舰
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

# wunder 心舰

<p class="docs-eyebrow">Agent Orchestration Kernel | server / cli / desktop</p>

## 什么是 wunder？

wunder 是一个**会执行任务的智能体系统**。你告诉它目标，它会自动拆解任务、调用工具、并行协作，最终交付结果。

它不是一个简单的聊天机器人——而是一个**可编程的智能体执行引擎**，支持三种运行形态：

| 形态 | 面向人群 | 核心价值 |
|------|----------|----------|
| **Desktop** | 个人用户 | 下载即用，本地智能体工作台 |
| **Server** | 团队/组织 | 多租户治理、统一接入、权限管控 |
| **CLI** | 开发者/自动化 | 终端驱动、脚本化、流水线集成 |

## 它能做什么？

想象一下，你有一个智能助手团队：

- **文件与代码**：读取文件、编辑代码、执行命令、重构项目
- **办公自动化**：整理文档、生成报告、处理表格、做会议纪要
- **多智能体协作**：一个查资料、一个写稿、一个复核，并行加速
- **持续任务**：定时巡检、周期提醒、跨渠道消息处理
- **系统集成**：连接 MCP/外部系统，组合 Skills 固化流程

## 系统结构（1 分钟理解）

![wunder 系统结构示意图：心舰到用户、蜂群、智能体、线程的分层关系](/docs/assets/manual/08-hierarchy-structure.svg)

抓住这条主线：

```
心舰（wunder）
  └─ 用户（资源隔离边界）
      └─ 蜂群（协作单元）
          └─ 智能体（执行角色）
              └─ 线程（连续上下文与状态承载体）
```

一句话总结：**请求先落到用户域，再由蜂群编排智能体，最终在线程中持续执行。**

## 按你的角色选入口

<div class="docs-card-grid">
  <a class="docs-card" href="/docs/zh-CN/start/desktop/">
    <strong>个人用户</strong>
    <span>直接下载 desktop，5 分钟上手。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/server/">
    <strong>团队管理员</strong>
    <span>部署 server，统一管理多用户与权限。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/cli/">
    <strong>开发者</strong>
    <span>使用 CLI，脚本化与自动化集成。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/">
    <strong>系统集成</strong>
    <span>接入 /wunder 接口，嵌入到你的系统。</span>
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

## 核心特性一览

### 五维能力框架

| 维度 | 说明 |
|------|------|
| **形态协同** | desktop / server / cli 共享同一套核心引擎 |
| **租户治理** | 多用户、单位树、配额管理、权限控制 |
| **智能体协作** | 蜂群分工、并行执行、结果归并 |
| **工具生态** | 内置工具 + MCP + Skills + 知识库 |
| **接口开放** | WebSocket/SSE 流式、RESTful、A2A 标准 |

### 技术亮点

- **流式优先**：WebSocket 默认，SSE 兜底，支持断线恢复
- **线程冻结**：system prompt 首次确定后锁定，避免缓存失效
- **长期记忆**：结构化记忆碎片，支持手动/自动提炼
- **上下文压缩**：智能摘要 + 预算控制，应对长对话
- **原子写入**：文件操作采用临时文件 + rename 策略
- **工具防爆**：双层裁剪 + 预算控制，防止上下文爆炸

## 最容易搞错的点

在继续之前，先澄清几个常见误区：

| 误区 | 正确理解 |
|------|----------|
| `/wunder` 的 `user_id` 必须是已注册用户 | 不需要，可以是任意虚拟标识 |
| token 统计就是账单消耗 | ❌ 当前上下文占用看 `round_usage.total_tokens`，总消耗按各请求 `round_usage.total_tokens` 累加 |
| 线程每次都会重写 system prompt | 首次确定后会**冻结**，后续轮次不再改写 |
| 长期记忆每轮都会注入 | 只在线程**初始化时注入一次** |

## 快速导航

### 第一次使用？

先看 [快速开始](/docs/zh-CN/start/quickstart/)，10 分钟内跑通第一条链路。

### 想深入理解？

从 [核心概览](/docs/zh-CN/concepts/) 开始，建立系统运行模型。

### 需要接入开发？

去 [接入概览](/docs/zh-CN/integration/)，找到适合你的接入方式。

### 遇到问题？

查 [故障排查](/docs/zh-CN/help/troubleshooting/)，或看 [FAQ](/docs/zh-CN/help/faq/)。

## 延伸阅读

- [说明书总览](/docs/zh-CN/start/hubs/)
- [API 索引](/docs/zh-CN/reference/api-index/)
- [系统介绍](/docs/系统介绍.md)
- [设计方案](/docs/设计方案.md)
