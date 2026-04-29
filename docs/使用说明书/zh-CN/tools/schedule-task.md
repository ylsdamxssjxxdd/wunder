---
title: 定时任务
summary: `schedule_task` 的推荐写法、线程投递语义与调度行为。
read_when:
  - 你要新增、更新、查询或触发定时任务
source_docs:
  - src/services/tools/dispatch.rs
  - src/services/cron.rs
updated_at: 2026-04-29
---

# 定时任务

`schedule_task` 用来创建、更新、查询、立即执行和启停定时任务。

它是当前工具体系里的一个特例：

- 输入支持模型更容易调用的扁平字段
- 成功结果返回压缩后的调度信息
- 不走统一的 `ok/action/state/summary/data` 成功骨架

## 支持动作

- `add`
- `update`
- `remove`
- `enable`
- `disable`
- `get`
- `list`
- `run`
- `status`

## 推荐写法

优先用扁平字段，不必先手写完整 `job` 对象：

```json
{
  "action": "add",
  "job_id": "job_daily_report",
  "name": "日报提醒",
  "schedule_text": "every 5 minutes",
  "message": "请生成日报",
  "session": "main",
  "enabled": true
}
```

只有在需要精确控制时，再使用嵌套的 `schedule`：

```json
{
  "action": "add",
  "job_id": "job_cron_demo",
  "schedule": {
    "kind": "cron",
    "cron": "*/5 * * * *",
    "tz": "Asia/Shanghai"
  },
  "message": "执行巡检",
  "session": "isolated"
}
```

## 字段说明

- `action`：本次要执行的动作。
- `job_id`：任务标识。更新、删除、启停、立即执行、查询时复用它。
- `name`：任务的展示名称。
- `schedule_text`：推荐填写。可用自然语言或 cron 文本。
- `schedule`：仅在需要精确表达 `at/every/cron` 时填写。
- `message`：到点后发给智能体的消息。
- `session`：执行线程策略，只支持 `main` 或 `isolated`。
- `enabled`：创建后是否立即启用。

## `session` 语义

- `main`
  触发时把消息发送到该智能体**当前主线程**。
  不是任务创建时记住的旧线程。

- `isolated`
  触发时先新建干净线程执行任务，再把结果回送到该智能体**当前主线程**。

如果任务没有绑定智能体主线程，则回退到任务记录里的 `session_id`。

## 循环任务是否会堆积

不会按“漏了多少次就补跑多少次”去堆积。

当前行为是：

- 同一个循环任务同一时刻最多只有一个活跃执行
- 如果 `every 1s`，但单次执行耗时远超 1 秒，不会并发堆出很多份相同任务
- 错过的间隔会被跳过或折叠，下一次执行时间会基于当前时间重新推进

这意味着它更接近“保最新节奏”，而不是“补齐全部历史 tick”。

## 成功返回

### `status`

```json
{
  "action": "status",
  "scheduler": {
    "enabled": true,
    "poll_interval_ms": 1000,
    "running_jobs": 1,
    "next_run_at": 1760000000,
    "next_run_at_text": "2026-04-29T10:00:00+08:00"
  },
  "user_jobs": {
    "total": 3,
    "enabled": 2,
    "running": 1,
    "next_run_at": 1760000000,
    "next_run_at_text": "2026-04-29T10:00:00+08:00"
  }
}
```

### `add` / `update` / `get`

```json
{
  "action": "add",
  "job": {
    "job_id": "job_daily_report",
    "name": "日报提醒",
    "enabled": true,
    "schedule": {
      "kind": "every",
      "every_ms": 300000
    },
    "next_run_at": "2026-04-29T10:00:00+08:00",
    "last_run_at": null,
    "last_status": null
  }
}
```

### `list`

```json
{
  "action": "list",
  "jobs": [
    {
      "job_id": "job_daily_report",
      "name": "日报提醒",
      "enabled": true,
      "schedule": { "kind": "every", "every_ms": 300000 },
      "next_run_at": "2026-04-29T10:00:00+08:00",
      "last_run_at": null,
      "last_status": null
    }
  ]
}
```

## 注意

- `schedule_text` 和 `schedule` 同时传入时，以 `schedule` 为准。
- `schedule.every_ms` 最小为 `1000`。
- 参数必须是完整 JSON 对象；如果 JSON 没闭合，工具会直接报参数无效。
