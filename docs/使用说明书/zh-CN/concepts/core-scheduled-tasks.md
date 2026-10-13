---
title: 定时任务
summary: 定时任务让智能体在指定时间自动执行：at 定点、every 固定间隔、cron 表达式三种排程，带租约、退避与自动禁用等调度保障。
read_when:
  - 想让智能体定时汇报、巡检、提醒
  - 定时任务不触发、重复触发或被自动禁用
  - 判断该用定时任务还是会话内等待
source_docs:
  - crates/wunder-runtime/src/services/cron.rs
  - crates/wunder-runtime/src/services/cron_schedule.rs
  - crates/wunder-runtime/src/services/cron/policy.rs
  - crates/wunder-runtime/src/api/cron.rs
  - crates/wunder-core/src/config.rs
updated_at: 2026-10-13
---

# 定时任务

## 定位

定时任务是**引擎级调度**：任务定义持久化在存储里，后台循环按到期时间领取执行，与「你正在看着的那个会话」无关。它回答三个治理问题：跑了没有、成功还是失败、下次什么时候跑——每次执行都有记录（`/wunder/cron/runs`），失败有退避和自动禁用，进程重启不丢任务定义。

与之区分：会话内等待（`会话让出`）只影响当前线程；定时任务创建的是未来的新执行。

## 三种排程类型

| kind | 语义 | 输入 | 边界 |
| --- | --- | --- | --- |
| `at` | 一次性定点 | RFC3339 时间戳 | 最远 365 天内；执行完即自动禁用（失败则带原因禁用） |
| `every` | 固定间隔 | `every_ms`（可选起始 `at`） | 1,000 ms ≤ 间隔 ≤ 86,400,000 ms（24 小时） |
| `cron` | 表达式周期 | 5–7 字段表达式 + 可选 `tz` | 字段只允许数字与 `* / - , ?`；时区按 `chrono_tz` 具名时区解析，缺省用本地时区 |

自然语言写法在工具与快捷输入中被识别并折算：`every 5 minutes` → every、`daily at 9am` / `weekdays at 6pm` → 对应 cron 表达式；无法识别时给出上述格式提示。

内容上限：任务名 ≤ 128 字符，触发消息 ≤ 16,384 字符。

## 投递到哪里：session_target

| 取值 | 行为 |
| --- | --- |
| `main`（缺省） | 结果投递到绑定的当前会话，续写同一段对话 |
| `isolated` | 每次执行新建独立线程会话，并保留 `parent_session_id` 溯源 |

需要留痕、不打扰主对话的周期性任务选 `isolated`；希望任务结果直接接在现有上下文后面的选 `main`。

## 执行保障：租约、并发与退避

多实例部署时靠**租约**防重复执行：领取任务写入 `runner_id` / `run_token` / `lease_expires_at`，运行中心跳续租；进程崩溃后租约过期，任务可被其他实例重新领取。

默认调度参数（`cron.*` 配置节）：

| 参数 | 默认值 |
| --- | --- |
| 并发执行上限 `max_concurrent_runs` | 1 |
| 轮询间隔 `poll_interval_ms` | 1000 ms |
| 空闲最长休眠 `max_idle_sleep_ms` | 5000 ms |
| 租约 TTL / 心跳 | 300 s / 60 s |
| 连续失败自动禁用阈值 `max_consecutive_failures` | 5 |

失败退避阶梯按连续失败次数取档：**30s → 60s → 5m → 15m → 1h**（之后停留最后一档）。达到失败上限后任务自动禁用并写明 `auto_disabled_reason`；重新 `enable` 会清零失败计数。

## 总开关

`cron.enabled=false` 时，**add / update / enable / run 四类写操作全部被拒绝**（返回明确错误提示）；status / list / get / runs / remove / disable 仍可用，便于查看与清理存量任务。

## 入口与形态

| 面 | 情况 |
| --- | --- |
| REST | `/wunder/cron/*` 共 11 个端点（status/list/runs/add/update/remove/enable/disable/get/run/action），属**用户面**，按用户归属隔离 |
| 工具 | 内置工具 `定时任务`（别名 `schedule_task`），转同一处理器，模型可自建任务 |
| 蜂巢 | 设置内定时任务分类页 |
| 蜂窝 | 定时任务页 + 原生 façade（同进程调用同一处理器） |
| 舵机 | **无定时任务**：本地引擎启动时显式置 `cron.enabled = false`，也没有相关斜杠命令 |
| 舰桥 | 无用户级任务管理页；仅在「预设智能体」内作为模板配置存在 |

## 常见误解

- **「关掉总开关任务就没了」**——不是。禁用只挡新增与启用执行，存量任务仍可查看、禁用和删除。
- **「定时任务失败了会一直重试」**——不会。按阶梯退避，连续失败 5 次自动禁用，需要人工排查后重新启用。
- **「一次任务 = 一个 at 执行后还能留着」**——`at` 是一次性的，跑完即禁用（无论成败）。
- **「isolated 任务是无头孤线程」**——每次新建的会话保留 parent 溯源，且结果仍投递回配置的交付会话。
- **「舵机也能建定时任务」**——舵机显式关闭调度器；需要定时执行请在蜂巢/蜂窝/舰体侧配置。
- **「多个实例会各跑一遍」**——不会，租约互斥保证同一任务同一时刻只有一个 runner。

## 相关页

- [定时任务工具](/docs/zh-CN/tools/schedule-task/) — 工具参数与用法
- [稳定性与边界](/docs/zh-CN/concepts/core-stability/) — 退避与有界设计的全局视角
- [智能体循环](/docs/zh-CN/concepts/core-agent-loop/) — 任务触发后如何进入执行链
