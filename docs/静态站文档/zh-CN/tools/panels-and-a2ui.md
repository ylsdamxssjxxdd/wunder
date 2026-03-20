---
title: 界面协同工具
summary: `a2ui`、`计划面板`、`问询面板` 负责把模型执行过程变成前端可展示、可交互的界面状态，而不是只输出一段文本。
read_when:
  - 你要理解 Wunder 前端为什么能展示计划和问询分流
  - 你要知道哪些工具专门用来驱动界面
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools.rs
  - docs/API文档.md
---

# 界面协同工具

这组工具和文件、命令、网页抓取不同。

它们的目标不是直接“完成外部动作”，而是把模型当前状态显式投到界面层。

## `a2ui`

`a2ui` 用于把结构化 UI 消息发给前端。

常用字段：

- `uid`
- `a2ui`
- `content`

它更像界面层载荷，而不是普通文本回复。

## `计划面板`

`计划面板` 用来展示步骤化执行计划。

常用字段：

- `explanation`
- `plan`

其中每个 `plan` 项都包含：

- `step`
- `status`

系统会自动规范状态值，并且只保留一个 `in_progress` 项，其余会回落为 `pending`。

## `问询面板`

`问询面板` 用来让前端展示一个可选路线面板。

常用字段：

- `question`
- `routes`
- `multiple`

其中每个 `route` 主要包含：

- `label`
- `description`
- `recommended`

## 这组工具适合什么

- 展示执行计划
- 在多个实现路径之间让用户选路
- 把结构化界面状态推给前端

这也是 Wunder 前端不只是“聊天框”的原因之一。

## 它们和最终回复的区别

- `最终回复` 负责结束本轮并返回最终答案。
- 这组工具负责在过程里给界面补结构化状态。

所以它们更像“过程可视化工具”。

## 实施建议

- `a2ui`、计划面板、问询面板都属于界面驱动工具。
- 它们的目标是让前端展示状态和选择，而不是替代最终回复。
- 这类工具最适合配合用户侧前端和管理端调试面板理解执行过程。

## 延伸阅读

- [用户侧前端](/docs/zh-CN/surfaces/frontend/)
- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
