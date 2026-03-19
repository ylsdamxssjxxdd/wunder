---
title: CLI 使用
summary: wunder-cli 面向开发者、终端任务与自动化场景。
read_when:
  - 你想在终端中直接使用 wunder
  - 你更关注开发调试、脚本任务和工作区驱动执行
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
---

# CLI 使用

`wunder-cli` 是 wunder 的终端形态。

它适合开发者、本地任务、脚本化调用和工作区驱动执行。

## CLI 的特点

- 更适合调试和自动化
- 使用当前目录或指定目录作为工作空间
- 能更直接地观察会话、工具与产物
- 与 server、desktop 复用同一套调度内核

## 适合的任务

- 编程类任务
- 文件处理任务
- 自动化流程
- 需要终端输出、JSONL 事件或 TUI 交互的任务

## CLI 与 Desktop 的区别

- Desktop 更适合日常使用和图形化工作台
- CLI 更适合开发、脚本与终端协作
- 两者底层能力一致，但交互壳不同

## CLI 与 Server 的区别

- Server 面向多用户与治理
- CLI 面向单用户本地执行
- Server 要求更多部署与运行环境
- CLI 更适合快速启动和局部任务

## 推荐先理解的概念

- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [工具体系](/docs/zh-CN/concepts/tools/)
- [长期记忆](/docs/zh-CN/concepts/memory/)

## 相关文档

- [快速开始](/docs/zh-CN/start/quickstart/)
- [系统架构](/docs/zh-CN/concepts/architecture/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
