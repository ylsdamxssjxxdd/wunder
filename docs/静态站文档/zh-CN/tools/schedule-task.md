---
title: 定时任务
summary: `定时任务` 把 cron 能力做成正式工具，支持新增、更新、禁用、运行、查看状态，并把任务持久化到系统存储里。
read_when:
  - 你要让 Wunder 延迟执行或周期执行任务
  - 你要查 `定时任务` 工具和 `/wunder/cron/*` 接口的对应关系
source_docs:
  - src/services/tools/catalog.rs
  - src/services/tools/dispatch.rs
  - src/services/cron.rs
  - docs/API文档.md
---

# 定时任务

`定时任务` 是 Wunder 的内置 cron 工具。

它不是简单的前端提醒，而是会落到系统存储、由调度器持续执行的正式任务。

## 核心动作

- `add`
- `update`
- `remove`
- `enable`
- `disable`
- `get`
- `list`
- `run`
- `status`

## 常用参数

- `action`
- `job.job_id`
- `job.name`
- `job.schedule`
- `job.schedule_text`
- `job.session`
- `job.payload.message`
- `job.deliver`
- `job.enabled`
- `job.delete_after_run`
- `job.dedupe_key`

其中：

- `schedule.kind` 支持 `at`、`every`、`cron`
- `session` 支持 `main`、`isolated`
- `payload.message` 是最关键的执行载荷

## 它怎么运行

任务会被持久化到 cron 任务表中，由调度器按时间领取并执行。

如果 `session` 设为：

- `main`：沿用主会话语境
- `isolated`：新开隔离会话执行

如果 `delete_after_run=true`，任务成功后会自动删除。

## 什么时候该用它

- 固定时间提醒
- 周期性巡检
- 延迟触发某段消息或任务
- 后台定时执行用户任务

## 最容易混淆的点

### 它不是 `休眠等待`

`休眠等待` 只是在当前链路里短暂停顿。

`定时任务` 是持久化调度。

### `schedule_text` 和结构化 `schedule`

这两个字段都在描述调度规则，但真正落地时最终都会归一到系统的调度结构中。

如果你已经能明确表达 `at/every/cron`，直接给结构化 `schedule` 通常更稳。

## 你最需要记住的点

- `定时任务` 是持久化的系统调度工具，不是一次性等待。
- 最重要的载荷是 `payload.message`。
- `isolated` 会新开运行语境，`delete_after_run` 会在成功后自动清理任务。

## 相关文档

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [聊天会话](/docs/zh-CN/integration/chat-sessions/)
- [部署与运行](/docs/zh-CN/ops/deployment/)
