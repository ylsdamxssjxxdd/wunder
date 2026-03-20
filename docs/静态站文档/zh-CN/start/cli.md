---
title: CLI 使用
summary: 需要终端执行、脚本化接入或开发调试时，再看 `wunder-cli`。
read_when:
  - 你想在终端中直接使用 wunder
  - 你更关注开发调试、脚本任务和工作区驱动执行
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
---

# CLI 使用

如果你主要在终端里工作，这页先看。

`wunder-cli` 是 Wunder 的命令行形态，适合开发者、本地任务、脚本化调用和工作区驱动执行。

## 本页重点

- 什么时候优先用 CLI
- CLI 最适合哪些任务
- CLI 和 desktop、server 的分工差别

## 关键结论

- CLI 适合开发调试、批处理、脚本自动化和工作区驱动执行。
- 它通常直接使用当前目录或指定目录作为工作空间。
- 它和 desktop、server 复用同一套调度内核，差别主要在交互壳。

## 什么时候先用 CLI

- 你要做编程类任务或文件处理
- 你要在终端里观察事件、工具调用和产物
- 你要接脚本、CI、批处理或自动化流程
- 你需要 JSONL 事件流或 TUI 交互

## CLI 最适合的场景

- 一次性的本地任务
- 以目录为中心的工作区操作
- 调模型、调工具、看运行细节
- 不需要完整图形界面的开发工作流

## 什么时候不要先看这页

- 你只是想直接日常使用 Wunder，不想碰终端
- 你在做团队部署、管理员后台或统一对外接口
- 你需要桌面窗口、本地 GUI 和消息工作台

## 常见误区

- CLI 不是“功能更少的 desktop”，它更适合开发和自动化。
- CLI 适合单用户本地执行，不负责多用户治理。
- 工作目录就是上下文的一部分，换目录往往就换了任务作用域。

## 延伸阅读

- [快速开始](/docs/zh-CN/start/quickstart/)
- [系统架构](/docs/zh-CN/concepts/architecture/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
