---
title: 定时任务
summary: `schedule_task` 的推荐写法、线程投递语义与调度行为。
read_when:
  - 用户要新增、更新、查询或触发定时任务
source_docs:
  - src/services/tools/dispatch.rs
  - src/services/cron.rs
updated_at: 2026-10-02
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
  在任务记录的 `session_id` 所绑定的线程执行；默认绑定创建任务的线程。切换页面、创建其它线程或其它线程变活跃都不会改变绑定。忙碌时进入统一任务队列，可从绑定线程停止。

- `isolated`
  在绑定线程下新建干净子线程执行。完成结果以独立的已完成轮次投递到绑定线程，不再把结果作为用户指令调用模型。子线程与结果轮次均有独立标识，错误不会写入上一轮助手气泡。

绑定线程被归档、删除或所属智能体不匹配时，任务明确失败，不猜测其它线程。`get/list` 返回 `session_id`、`session_target`、`running`、`last_status` 和 `last_error`。

`run` 返回 `queued: true` 只表示异步执行已受理，不代表完成。不要在创建任务的模型轮次里反复查询等待；结束当前回复，让队列继续执行。

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

## 离线回归验收

`npm run test:chat` 包含定时任务后台轮次场景：旧轮次运行、新轮次拒绝、停止、排队、独立结果投递及刷新恢复。HTTP/WebSocket 使用合成协议对端，页面操作和渲染使用真实浏览器，不启动正式服务或调用真实模型。

查看 `temp_dir/chat-scheduled-review/conversation/index.html` 的分屏截图，以及同目录上一级的 `thread-export.jsonl`、`thread-changes.jsonl`、`performance.json` 和 `analysis.json`。后端定向运行 `cargo test -p wunder-runtime cron --lib`，覆盖真实 SQLite 与任务队列准入、取消和结果投递。两类测试分别验证后端状态和浏览器呈现，不宣称覆盖真实模型行为。

### 调度状态与页面统计

服务端需启用 `cron.enabled`。关闭时创建、更新、启用和手动运行会返回明确错误，列表仍可读取并显示调度器状态；已经运行的服务需重新加载配置或重启才能采用修改。工具详情显示本地时区时间、执行规则、上次状态与绑定线程，原始结果仍保留在线程日志中。

常规 `npm run test:chat --workspace wunder-frontend` 覆盖切到设置页期间完成、返回聊天、刷新后保留生成速度及定时工具摘要截图。`cargo test -p wunder-runtime cron --lib` 覆盖真实调度循环到期领取与失败结算，不连接真实模型；成功结果回传另由持久化回归覆盖。截图位于 `temp_dir/chat-scheduled-review/`。

定时执行使用所属用户及智能体的审批模式、模型、工具能力和工作区配置。独立执行线程保留绑定线程的显式工具限制，并冻结自己的初始工具基线；仍受用户工具权限约束。需要审批的智能体不会因定时触发而自动获得全自动权限。
