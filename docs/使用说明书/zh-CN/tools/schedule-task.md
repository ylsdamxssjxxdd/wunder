---
title: 定时任务
summary: `schedule_task` 的扁平参数、调度动作与压缩返回。
read_when:
  - 你要新增、更新、查询或触发定时任务
source_docs:
  - src/services/tools/dispatch.rs
  - src/services/cron.rs
updated_at: 2026-04-10
---

# 定时任务

`schedule_task` 是当前工具体系里一个明确的例外：

- 输入侧支持模型友好的扁平字段
- 成功返回侧是压缩调度结果
- **不走统一成功骨架**

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

## 推荐输入风格

优先用扁平字段：

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

只有在需要精确表达时再用 `schedule`：

```json
{
  "action": "add",
  "job_id": "job_cron_demo",
  "schedule": {
    "kind": "cron",
    "cron": "*/5 * * * *",
    "tz": "Asia/Shanghai"
  },
  "message": "执行巡检"
}
```

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
    "next_run_at_text": "2026-04-10T10:00:00+08:00"
  },
  "user_jobs": {
    "total": 3,
    "enabled": 2,
    "running": 1,
    "next_run_at": 1760000000,
    "next_run_at_text": "2026-04-10T10:00:00+08:00"
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
    "next_run_at": 1760000000,
    "next_run_at_text": "2026-04-10T10:00:00+08:00",
    "last_run_at": null,
    "last_status": null
  },
  "deduped": false
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
      "next_run_at": "2026-04-10T10:00:00+08:00",
      "last_run_at": null
    }
  ]
}
```

## 重点

- 这是少数仍未统一到 `ok/action/state/summary/data` 的工具
- 模型侧写参数时优先扁平字段
- 结果里真正重要的是 `job`、`jobs`、`scheduler`、`user_jobs`
