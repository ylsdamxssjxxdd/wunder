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

## 用循环定时任务实现“心跳式巡检”

如果你想要的是类似 heartbeat 的效果，当前在 Wunder 里可以直接用：

- 一个 `schedule.kind=every` 的循环定时任务
- 智能体工作目录里的一个巡检清单文件
- 一条固定提示词，让智能体每次被唤醒时先检查清单，再决定是否继续执行、记录结果或提醒用户

这是一种很实用的实现方式：

- 不需要额外引入专门的 heartbeat 机制
- 直接复用现有 `定时任务` 调度能力
- 清单状态放在工作目录里，用户和智能体都能直接查看和更新

### 推荐做法

建议把任务配置成：

- `schedule.kind = every`
- `session = isolated`
- `payload.message` 写成固定巡检提示

例如：

```json
{
  "action": "add",
  "job": {
    "name": "每30分钟巡检一次清单",
    "session": "isolated",
    "schedule": {
      "kind": "every",
      "every_ms": 1800000
    },
    "payload": {
      "message": "检查工作目录中的巡检清单文件。先判断哪些事项已经完成，哪些事项仍未完成；已完成的不要重复执行，未完成的继续处理；如果没有新事项，不要输出冗长回复。"
    }
  }
}
```

对应的工作目录清单可以保持简短、稳定、可持续更新，例如：

```md
# 巡检清单

- 检查是否有新的待办文件需要整理
- 检查是否有失败的批处理任务需要重试
- 检查是否有需要提醒用户的阻塞项
- 已完成的事项要从执行角度跳过，不要反复做同样的动作
```

### 为什么推荐 `isolated`

如果这类巡检任务总是跑在 `main` 会话里，长期下来会不断累积无意义的巡检轮次，增加上下文占用。

因此更推荐：

- 周期巡检跑在 `isolated`
- 真正需要通知用户时，再输出简洁结果

这样更接近 heartbeat 的用途：定期检查，而不是不断污染主聊天上下文。

### 这种方式适合什么

- 周期性检查某个目录、文件或状态清单
- 每隔一段时间做一次“是否还有未完成事项”的回看
- 用工作目录里的 Markdown 文件驱动智能体做持续性维护

### 这种方式不等于专门 heartbeat

这种方案已经可以实现 heartbeat 风格的巡检，但它本质上仍然是：

- `cron` 调度
- 固定提示词
- 文件驱动状态

它不是独立的系统级 heartbeat runner，因此：

- 更简单
- 更容易理解
- 也更依赖你写给智能体的提示词和清单质量

如果你的目标只是“让智能体周期性看一眼清单并继续推进”，这种做法通常已经够用。

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
