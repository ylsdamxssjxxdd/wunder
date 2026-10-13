---
title: 会话线程控制
summary: `会话线程控制`（别名 `thread_control` / `session_thread`）用于列出、创建、切换与归档任务线程。
read_when:
  - 智能体需要同时推进多个任务、开新线程或切回父线程
  - 需要查看线程树关系、归档或恢复某个线程
source_docs:
  - crates/wunder-runtime/src/services/tools/thread_control_tool.rs
  - crates/wunder-runtime/src/services/tools/catalog.rs
  - crates/wunder-runtime/src/services/default_tool_profile.rs
  - crates/wunder-runtime/src/services/goal/tools.rs
updated_at: 2026-10-13
---

# 会话线程控制

规范名：`会话线程控制`　别名：`thread_control`、`session_thread`（`crates/wunder-runtime/src/services/tools/thread_control_tool.rs:12-14`，别名注册在 `crates/wunder-runtime/src/services/tools/catalog.rs:1228-1235`）。

管的是**任务线程本身**：一个智能体可同时拥有多个独立任务线程，调度、锁与实时状态都按线程隔离。子智能体那种"临时派生、跑完回收"的工作单元不归它管，见 [子智能体控制](/docs/zh-CN/tools/subagent-control/)。

## 什么时候用

- 用户提的是另一件独立的事，需要新线程承载，而不是在当前线程里继续。
- 需要在线程之间切换、回到父线程、给线程改名。
- 需要归档/恢复线程，或查看某条线程的祖先与子线程。

## 形态差异

桌面模式（蜂窝）把本工具列为**系统级隐藏工具**：既不进默认集，也不在桌面可选集里（`crates/wunder-runtime/src/services/default_tool_profile.rs:22-31`）。蜂巢/舰体/舵机形态下按管理员勾选结果暴露。

同一份过滤名单里还有 `get_goal` / `create_goal` / `update_goal`：目标三件套**不在选择面里**，由运行时按会话自动注入（`crates/wunder-runtime/src/services/goal/tools.rs:16-18`），也不该由文档描述成用户可勾选的工具。`会话让出` 同样在桌面隐藏，见 [面板与会话让出](/docs/zh-CN/tools/panels-and-a2ui/)。

## 动作

`action` 是唯一必填参数，枚举值固定八个（`catalog.rs:945-980`）；运行时另有归一别名，含中文（`thread_control_tool.rs:83-105`）：

| 规范动作 | 归一别名举例 | 作用 |
| --- | --- | --- |
| `list` | `ls`、`threads`、`会话列表`、`列表` | 按范围列出线程 |
| `info` | `get`、`detail`、`详情` | 单线程详情 + 父 + 直接子线程 |
| `create` | `new`、`spawn`、`新建` | 新建线程，默认继承父线程的智能体与工具覆盖 |
| `switch` | `goto`、`open`、`focus`、`切换` | 切到指定线程 |
| `back` | `return`、`parent`、`返回` | 切回当前线程的父线程 |
| `update_title` | `rename`、`title`、`重命名` | 改标题 |
| `archive` | `archived`、`归档` | 置为 `archived` |
| `restore` | `unarchive`、`恢复` | 置回 `active` |

未识别的动作直接报错 `unknown thread_control action: <原值>`（`thread_control_tool.rs:79`）。

## 参数

模型侧 schema（`catalog.rs:945-980`）：

| 字段 | 类型 | 必填 | 取值 / 上限 | 默认 |
| --- | --- | --- | --- | --- |
| `action` | string | 是 | 上表八个枚举 | — |
| `session_id` | string | 否 | 目标线程 id | 当前线程 |
| `parent_session_id` | string | 否 | 仅 `create` 用 | 当前线程 |
| `title` | string | 否 | `create` / `update_title` 用 | `create` 缺省为 `新会话` |
| `scope` | string | 否 | `branch` / `children` / `roots` / `all` | `branch` |
| `status` | string | 否 | `active` / `archived` / `all` | `active` |
| `limit` | integer | 否 | 1–200 | 20 |

参数结构体还接受 `agent_id`、`label`、`switch`（`thread_control_tool.rs:44-58`），但它们不在模型侧 schema 中：`switch` 决定 `create` 后是否切过去，默认 `true`；`archive` / `restore` 的 `switch` 默认 `false`。

`scope` 语义（`thread_control_tool.rs:336-389`、`443-495`）：

- `branch`（默认）：祖先链（自根向下）+ 当前目标 + 其直接子线程，每条带 `relation` 标记 `ancestor` / `current` / `child`。
- `children`：目标线程的直接子线程。
- `roots`：无父线程的根线程。
- `all`：按状态与智能体范围的全部线程。

## 返回

统一骨架 `ok / action / state / summary / data`（`build_model_tool_success_with_hint`，`crates/wunder-runtime/src/services/tools.rs:153-183`），`state` 恒为 `completed`；`archive` 在同时切走时额外带 `next_step_hint`。

线程条目字段固定为（`thread_control_tool.rs:239-266`）：`id`、`title`、`status`、`created_at`、`updated_at`、`last_message_at`、`agent_id`、`tool_overrides`、`parent_session_id`、`parent_message_id`、`spawn_label`、`spawned_by`、`runtime_status`，可选 `relation`。`runtime_status` 取内存监视状态，与持久化的 `status`（`active` / `archived`）不是一个口径。

`create` 生成的 id 形如 `sess_<uuid>`，并继承父线程的 `agent_id` 与 `tool_overrides`，`spawned_by` 记为 `thread_control`（`thread_control_tool.rs:560-624`）。`archive` / `restore` / `switch` 后写库并 `touch` 会话时间。

除结果本身外，动作还会向事件通道发 `thread_control` 事件，携带 `action` / `session` / `switch_session` / `switch` / `previous_session_id`，供前端切换视图（`thread_control_tool.rs:15`、`268-288`）。

## 失败

- 用户 id 为空：报需要用户标识。
- 线程不存在：报会话未找到。
- `info` / `switch` / `archive` / `restore` 会校验智能体归属；显式传入的 `agent_id` 与目标线程不一致时判定为权限拒绝（`thread_control_tool.rs:187-197`、`525-530`）。
- `update_title` 缺 `title` 报内容必填。
- `back` 在当前线程没有父线程时报会话未找到。

## 边界

- **列表必须有界**：`limit` 上限 200，`info` 的子线程展开也用 200 上限（`thread_control_tool.rs:19-20`、`537-544`）。祖先链沿 `parent_session_id` 逐级上溯，遇到环或过深会由上层树校验拒绝。
- `archive` 的"顺手切走"只在 `switch=true` 且同智能体下存在其它 `active` 线程时发生，选中者是列表里的第一条其它线程，不保证是用户期望的那条（`thread_control_tool.rs:391-408`、`751-783`）。
- `update_title` 会把非归档线程的状态强制回 `active`（`thread_control_tool.rs:709-715`）。
- 切线程不等于切工作：调度与锁按线程隔离，切过去的线程若正在运行，其状态在 `runtime_status` 可见。
- 本工具不删除线程，归档是可逆的（`restore`）。

## 相关页面

- 临时派生工作单元：[子智能体控制](/docs/zh-CN/tools/subagent-control/)
- 线程与轮次模型：[核心概念 · 会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)
