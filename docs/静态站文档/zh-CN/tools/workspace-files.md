---
title: 文件与工作区工具
summary: Wunder 最常用的一组内置工具就是文件工具，它们覆盖列目录、搜索、切片读取和直接写文件。
read_when:
  - 你想知道模型为什么优先用工具，而不是直接猜文件内容
  - 你要区分文件工具和工作区 HTTP API 的职责
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - src/services/tools/catalog.rs
---

# 文件与工作区工具

Wunder 的高频工具首先不是浏览器，而是文件工具。

## 这页解决什么

这页只讲这一组内置工具：

- `列出文件`
- `搜索内容`
- `读取文件`
- `写入文件`

它们是模型处理代码、配置、日志和 Markdown 的第一入口。

## 什么时候优先用它们

- 先看目录结构：`列出文件`
- 先定位关键词：`搜索内容`
- 再读局部代码或配置：`读取文件`
- 最后直接写入单文件：`写入文件`

这套顺序比“上来整文件硬读”更稳定，也更省上下文。

## `列出文件`

适合：

- 快速看某个目录是否为空
- 看文件树结构
- 给后续搜索或读取确定路径

常用参数：

- `path`
- `max_depth`

## `搜索内容`

适合：

- 在工作区里按关键词定位代码、配置或日志
- 先缩小候选文件，再决定读哪一段

常用参数：

- `query`
- `path`
- `file_pattern`
- `query_mode`
- `case_sensitive`
- `max_depth`
- `max_files`
- `max_matches`
- `context_before`
- `context_after`

这一工具还支持预算和预演参数，例如：

- `dry_run`
- `time_budget_ms`
- `output_budget_bytes`

## `读取文件`

适合：

- 读一小段代码
- 读多段不连续范围
- 按缩进块读结构化片段

常见用法不只是一刀切读取，还包括：

- `start_line/end_line`
- `line_ranges`
- `mode=indentation`

所以它不是普通的“cat 文件”，而是偏面向模型消费的切片读取工具。

## `写入文件`

适合：

- 直接生成一个新文件
- 全量覆盖一个较短文件

如果你要做多位置修改，通常不该先选它，而应看：

- [应用补丁](/docs/zh-CN/tools/apply-patch/)

## 文件工具和工作区 API 的区别

可以这样记：

- 文件工具：给模型在推理链路里直接调用
- 工作区 API：给前端或外部系统做文件面板和上传下载

两者都围绕工作区，但使用者不同。

## 你最需要记住的点

- 先列目录，再搜索，再切片读取，是更稳定的文件工作流。
- `读取文件` 强调局部读取，不鼓励大文件整段硬塞上下文。
- 单文件直接覆盖用 `写入文件`，多位置编辑用 `应用补丁`。

## 相关文档

- [应用补丁](/docs/zh-CN/tools/apply-patch/)
- [工作区 API](/docs/zh-CN/integration/workspace-api/)
- [工作区路由参考](/docs/zh-CN/reference/workspace-routing/)
