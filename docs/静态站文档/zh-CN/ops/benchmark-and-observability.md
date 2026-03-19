---
title: 监控与 Benchmark
summary: Wunder 的观测面不是单一监控页，而是由会话监控、工具统计、性能采样、吞吐压测和 benchmark 共同组成。
read_when:
  - 你在排查线程、工具或模型链路问题
  - 你要区分 monitor、throughput、performance 和 benchmark 的职责
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - src/api/admin.rs
---

# 监控与 Benchmark

Wunder 已经把“能不能看到问题”和“能不能量化问题”分成了几条独立链路。

## 这页解决什么

这页只讲清楚：

- 线上线程和工具问题去哪看
- 性能采样和吞吐压测有什么区别
- benchmark 为什么不是简单压测

## 先分四类能力

### 会话监控

主要入口：

- `GET /wunder/admin/monitor`
- `GET /wunder/admin/monitor/{session_id}`
- `POST /wunder/admin/monitor/{session_id}/cancel`
- `POST /wunder/admin/monitor/{session_id}/compaction`

这条链路解决的是：

- 线程现在在干什么
- 最近发生了哪些事件
- 当前 token 占用、阶段和耗时是多少

### 工具使用统计

主要入口：

- `GET /wunder/admin/monitor/tool_usage`

它解决的是：

- 某个工具最近被谁用得最多
- 工具调用与线程状态之间是否相关

### 性能与吞吐

主要入口：

- `/wunder/admin/throughput/*`
- `/wunder/admin/performance/sample`

它们不是一回事：

- throughput 更偏并发压测
- performance sample 更偏链路基线采样，不涉及模型能力评分

### Benchmark

主要入口：

- `/wunder/admin/benchmark/*`

它更接近“能力评估”，关注任务完成质量与结果结构，而不是只看速度。

## 为什么要分开

因为这几类问题本来就不同：

- 线上线程异常：看 monitor
- 某工具是否成了热点或瓶颈：看 tool_usage
- 服务在高并发下能不能扛住：看 throughput
- 某次改动是否让能力退化：看 benchmark

## 观测时最值得记住的字段

- `trace_id`：跨模块追踪
- `log_profile`：`normal` 或 `debug`
- `token_usage.input_tokens`：实际上下文占用
- `prefill_*` / `decode_*`：速度与耗时拆分

## 最容易犯的错

### 用 benchmark 代替线上监控

benchmark 不能替代真实线程监控。

### 用吞吐压测代替能力评估

throughput 只告诉你“扛不扛得住”，不告诉你“答得好不好”。

### 只看一层日志

很多问题需要把 monitor、tool_usage 和渠道运行态一起看。

## 你最需要记住的点

- `monitor` 看线上线程。
- `tool_usage` 看工具热点和调用面。
- `throughput/performance` 看系统链路承压能力。
- `benchmark` 看任务质量与能力回归。

## 相关文档

- [渠道运行态](/docs/zh-CN/ops/channel-runtime/)
- [流式事件参考](/docs/zh-CN/reference/stream-events/)
- [管理端面板索引](/docs/zh-CN/reference/admin-panels/)
