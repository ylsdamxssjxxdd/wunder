---
title: 稳定性
summary: 出错不扩散、失败能恢复、问题可复盘。稳定性靠的是有界、预算、熔断与单写者这四类硬约束，不是靠重试次数。
read_when:
  - 长时间无响应、卡住、反复失败，要判断系统正在做什么
  - 要解释「为什么被限流 / 被熔断 / 被排队拒绝」
  - 配置并发、超时、轮次与预算上限
  - 排查队列堆积、慢客户端或长持锁
source_docs:
  - crates/wunder-runtime/src/core/bounded_queue.rs
  - crates/wunder-runtime/src/core/long_task.rs
  - crates/wunder-runtime/src/orchestrator/constants.rs
  - crates/wunder-runtime/src/orchestrator/limiter.rs
  - crates/wunder-runtime/src/orchestrator/retry_governor.rs
  - crates/wunder-runtime/src/orchestrator/stream_timeout.rs
  - crates/wunder-runtime/src/orchestrator/empty_output_guard.rs
  - crates/wunder-runtime/src/orchestrator/execute_support.rs
  - crates/wunder-runtime/src/orchestrator/thread_log_committer.rs
  - crates/wunder-runtime/src/api/errors.rs
updated_at: 2026-10-13
---

# 稳定性

设计哲学三句话：**控扩散**（一个环节出错不带崩别的）、**能恢复**（失败有正式恢复链路）、**可复盘**（错误结构化、能归类）。落地成四类硬约束：有界、预算、熔断、单写者。

## 一、一切队列都有界，满了报 busy

通用有界队列 `core/bounded_queue.rs`：容量固定，`try_send` 失败 + 超时后返回 `bounded queue <name> busy`（`:48-81`），并记一条队列 busy 指标。**没有无界缓冲，也没有静默丢弃**。

| 队列 | 边界 | 出处 |
| --- | --- | --- |
| 单订阅事件写队列 | 256 | `orchestrator/constants.rs:26` |
| 慢客户端水位 | 剩余容量 ≤ 2 | `constants.rs:32` |
| 单次事件拉取 | 200 | `constants.rs:34` |
| 线程变更 feeder 页 | 200 | `services/thread_change_feeder.rs:20` |
| 渠道入站 | 512 / 并发 8 / 入队 80 ms | `channels/inbound_queue.rs:10-12` |
| 工作区写队列 | 2048 | `services/workspace.rs:40` |

后台任务统一经 `core/long_task.rs`（`spawn` / `spawn_with_warn_after`）启动：带标签、带长运行告警、panic 被兜住，不会因为某个循环崩了拖垮进程。排查时看日志里的任务标签就能认出是哪条循环。

## 二、限额与预算（数值都有出处）

| 项 | 值 | 出处 |
| --- | --- | --- |
| 用户输入上限 | `1 << 20` 字符；图片附件不计入 | `constants.rs:40`、校验在 `orchestrator/request.rs:363-380`、计量在 `wunder-core/src/request_limits.rs:3` |
| LLM 默认超时 | 120 秒 | `constants.rs:35` |
| 工具默认超时 | 120 秒（下限 1 秒） | `constants.rs:36-37` |
| 工具并行度 | ≤ 4 | `constants.rs:39` |
| 轮次上限 | 非管理员 `max_rounds` 默认 1000（下限 2） | `orchestrator/execute_support.rs:114-116` |
| 工具调用预算 | 每轮 10000 | `execute_support.rs:118` |
| 记忆召回预算 | 每轮 2000 | `execute_support.rs:121` |
| 会话锁 TTL / 心跳 | 120 秒 / 5 秒 | `constants.rs:14-15` |
| 活跃会话上限 | 默认 300（非管理员受此约束） | `wunder-core/src/config.rs:432/448`、`orchestrator/execute.rs:96-100` |
| 审批批准缓存 | 600 秒 | `core/exec_policy.rs:18` |

`agent_queue.enabled` 默认 true；关掉后运行中再发消息会被判「用户忙」且不进排队条（舰体启动会告警）。这是配置陷阱，不是故障。

## 三、会话锁与单写者

**同一任务线程串行执行，不同线程互不阻塞**：调度、锁与实时状态都按线程隔离。`orchestrator/limiter.rs` 用持久锁 + TTL 120 秒 + 心跳 5 秒续约，等待方带退避而不是固定轮询（`constants.rs:16-22`：2 次以后开始退避，上限 3 秒 + 抖动，等待超 30 秒告警）。这里刻意避免固定 0.2 秒轮询——否则整个满负荷系统会把自己变成存储风暴。

**单写者原则**：线程日志的唯一写出口是 `ThreadLogCommitter`（`orchestrator/thread_log_committer.rs:5-22`）：

1. 每会话 gate 串行化提交，保证变更顺序 == 提交顺序；
2. 存储调用是一个短的阻塞事务；
3. **事务成功后才发布游标**给变更 hub（hub 只是唤醒信号，不是真相）；
4. 幂等空操作（重放受理、状态未变、重复反馈）不产生变更也不发通知；失败的提交永不发布游标。

调用方不得自己拼游标、不得在事务完成前通知 hub。这条原则保证「客户端看到的顺序」与「落库顺序」一致，也是断线补水能无损的前提。

长持锁与慢客户端是两类被点名治理的问题：锁靠 TTL + 心跳兜住持有者消失；慢客户端靠水位检测先发 `resume_recommended` 再脱钩订阅（见 [实时性](/docs/zh-CN/concepts/core-realtime/)）。

## 四、恢复与熔断

| 场景 | 机制 |
| --- | --- |
| 流式卡住 | 空闲看门狗 `StreamActivity::with_idle_timeout`（`orchestrator/stream_timeout.rs`），按「无活动时长」判定，不是按总时长掐 |
| 模型返回空/不可用输出 | `empty_output_guard.rs`：关闭 thinking 后**一次性重试 1 次**；仍失败报 `LLM_OUTPUT_LOOP`（`max_recovery_attempts: 1`） |
| 上下文超限 | 渐进压缩（history / overflow / pre_sampling 三触发面），overflow 恢复尝试上限 8 次（`execute_support.rs:117`） |
| 同类失败反复 | `retry_governor.rs` 按失败指纹熔断 |
| 网络/订阅断开 | 游标补水，不重放整段历史 |

### 重试熔断阈值（同指纹计数）

| 条件 | 阈值 |
| --- | --- |
| 非重试类失败 | 5 次 |
| 参数/Schema 类失败 | **2 次**（确定性错误，等待不会变好，两次就给出改道提示） |
| 可重试失败 / 同工具连续失败 | 5 次 |
| 有成功但无进展 | 4 次 |

指纹细节最多记 240 字符（`retry_governor.rs:15`）。意义在于：**熔断看的是「同一问题重复几次」，不是「总共调了几次」**，10000 的工具预算不会被一个死循环白烧完。

## 五、错误对外是什么样

所有 HTTP 错误走同一个包络（唯一定义处 `api/errors.rs:132-160`）：

```json
{
  "ok": false,
  "error": {
    "code": "USER_QUOTA_INSUFFICIENT",
    "message": "…",
    "status": 429,
    "hint": "Retry later or reduce request frequency.",
    "trace_id": "err_…",
    "timestamp": 1700000000.0
  },
  "detail": {}
}
```

并回写响应头 `x-trace-id` 与 `x-error-code`（`:9-10`）。code 与 status/hint 双向映射（`:62-126`），稳定 code 举例：`AUTH_REQUIRED`、`PERMISSION_DENIED`、`SESSION_NOT_FOUND`、`USER_BUSY`、`USER_QUOTA_INSUFFICIENT`、`RATE_LIMITED`、`CONTEXT_WINDOW_EXCEEDED`、`PAYLOAD_TOO_LARGE`、`LLM_UNAVAILABLE`、`LLM_OUTPUT_LOOP`、`UPSTREAM_TIMEOUT`、`CONNECTION_CLOSED`、`INTERNAL_ERROR`。

**按 code 判断，不要按 message 文案判断**——文案可能因语言与版本变化，code 与 status 映射才是契约。

## 什么时候该等什么，什么时候该动手

| 现象 | 系统正在做什么 | 该做 |
| --- | --- | --- |
| 显示压缩中 / 排队中 | 正常处理 | 等 |
| 出现 `slow_client` / `resume_recommended` | 你的订阅被脱钩保护 | 走补水端点重建投影 |
| 报 `USER_BUSY` | 该任务线程仍在执行（会话锁被持有） | 排队、或让出/取消该线程 |
| 反复同一工具失败后被停 | 指纹熔断已触发 | 改参数或换路径，重复提交无用 |
| 报 `LLM_OUTPUT_LOOP` | 一次性恢复已用完 | 换模型配置或改问法 |
| 报 `USER_QUOTA_INSUFFICIENT` | 准入阶段扣减未过 | 见 [额度与用量](/docs/zh-CN/concepts/quota-and-token-usage/) |
| 界面与后端状态不一致 | 投影落后 | 刷新/重进；仍不一致看运行态 |
| 需要彻底重来 | — | 「重置工作状态」（`services/work_state_reset.rs`：按用户取消会话与任务、清工作区，返回 fresh_sessions） |

## 常见误解

- **「重试次数多就是稳」** — 稳定性来自有界、预算与熔断；同类失败最多 5 次（参数类 2 次）就停，是刻意的。
- **「队列会无限排队保证不丢」** — 队列都有界，满了报 busy，靠调用方或平台重投。
- **「超时按总时长掐」** — LLM 侧是空闲看门狗，长任务只要持续有活动就不会被掐。
- **「前端看到的状态就是后端真相」** — 前端是投影；线程日志才是真相，见实时性页。
- **「审批等很久系统会自己继续」** — 审批是暂停不是终态，批准缓存 600 秒只对重复同类动作有效。
- **「卡住就是死锁」** — 会话锁有 TTL 120 秒与心跳续约，持有者消失会被回收。
- **「关掉会话队列能减少排队」** — 会把排队变成直接判忙，通常不是你想要的。
- **「错误 message 可以拿去判断分支」** — 用 `error.code` 与响应头 `x-error-code`。

## 延伸阅读

- [边界处理](/docs/zh-CN/concepts/boundary-handling/) —— 逐个边界情况的行为与数值
- [实时性](/docs/zh-CN/concepts/core-realtime/) —— 补水、游标与背压
- [上下文压缩](/docs/zh-CN/concepts/core-context-compression/) —— 超限压缩细节
- [稳定性与观测](/docs/zh-CN/concepts/core-observability/) —— 指标与日志
- [故障排查](/docs/zh-CN/help/troubleshooting/) —— 实际问题处理步骤
- [认证与安全运维](/docs/zh-CN/ops/auth-and-security/) —— 权限与风险治理
