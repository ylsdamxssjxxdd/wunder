---
title: wunder 心舰
summary: wunder 是会执行任务的智能体系统。用户在蜂巢里描述目标，智能体拆解任务、调用工具、交付结果。
read_when:
  - 第一次了解 wunder
  - 需要快速判断从哪里开始
source_docs:
  - README.md
  - docs/设计文档/01-系统总体设计.md
---

# wunder 心舰

<p class="docs-eyebrow">会执行任务的智能体系统</p>

## 蜂巢：用户的工作台

用户通过**蜂巢**使用 wunder。蜂巢是用户侧的工作台，覆盖对话、文件、智能体、工具与设置。打开蜂巢，描述目标，智能体拆解任务、调用工具、交付结果。

蜂巢有两种获取方式：

| 方式 | 适合 | 说明 |
|------|------|------|
| **桌面端** | 个人用户 | 本地安装，开箱即用，可操作本机文件与桌面 |
| **网页端** | 团队/组织 | 浏览器访问，多人共用，统一管理 |

两种方式背后是同一套工作台，能力一致。个人用户安装桌面端即可；团队由管理员部署服务端后，成员通过浏览器访问网页端。开发者与自动化场景还可使用[命令行](/docs/zh-CN/start/cli/)。

## 能做什么

- **文件与代码**：读取文件、编辑代码、执行命令、重构项目
- **办公自动化**：整理文档、生成报告、处理表格、做会议纪要
- **多智能体协作**：一个查资料、一个写稿、一个复核，并行加速
- **持续任务**：定时巡检、周期提醒、跨渠道消息处理
- **系统集成**：连接外部服务，把常用流程固化成技能

## 工作台与系统结构

蜂巢采用三栏布局：左栏导航、中栏列表、右栏工作区。日常对话、文件管理、智能体配置、工具使用都在这里完成。详见 [认识蜂巢](/docs/zh-CN/surfaces/frontend/)。

系统的组织结构自上而下：

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
  <a class="docs-card" href="/docs/zh-CN/start/quickstart/">
    <strong>第一次使用</strong>
    <span>跑通第一个任务。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/frontend/">
    <strong>认识蜂巢</strong>
    <span>工作台的对话、文件、智能体与工具。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/desktop/">
    <strong>个人用户</strong>
    <span>下载桌面端，本地安装即用。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/server/">
    <strong>团队管理员</strong>
    <span>部署服务端，统一管理用户与权限。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/web-admin/">
    <strong>管理端</strong>
    <span>系统配置、用户与渠道治理。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/cli/">
    <strong>开发者</strong>
    <span>命令行驱动、脚本与自动化。</span>
  </a>
</div>

## 核心特性

| 特性 | 说明 |
|------|------|
| **统一工作台** | 桌面端与网页端共用同一套蜂巢，能力一致 |
| **多用户与权限管理** | 用户、单位、Token 额度、权限分层管控 |
| **智能体协作** | 多个智能体分工协作，并行执行，结果汇总 |
| **丰富的工具生态** | 内置工具 + MCP 外部工具 + 技能包 + 知识库 |
| **开放接口** | WebSocket 实时通信、RESTful API、A2A 互操作标准 |

## 快速导航

- **第一次使用** → [快速开始](/docs/zh-CN/start/quickstart/)
- **深入理解系统** → [核心概览](/docs/zh-CN/concepts/)
- **接入现有系统** → [接入概览](/docs/zh-CN/integration/)
- **遇到问题** → [故障排查](/docs/zh-CN/help/troubleshooting/) 或 [FAQ](/docs/zh-CN/help/faq/)

## 延伸阅读

- [说明书总览](/docs/zh-CN/start/hubs/)
- [API 索引](/docs/zh-CN/reference/api-index/)
- [系统介绍](/docs/设计文档/01-系统总体设计.md)
