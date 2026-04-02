---
title: FAQ
summary: wunder 的高频问题快速答复，适合在进入详细排障前先做判断。
read_when:
  - 你有高频使用疑问
  - 你想先快速判断是否属于故障
source_docs:
  - docs/系统介绍.md
  - docs/API文档.md
  - docs/设计方案.md
---

# FAQ

## `/wunder` 的 `user_id` 必须是注册用户吗？

不必须。`user_id` 是隔离与归属标识，可以是业务侧传入的虚拟用户标识。

## 我做聊天产品时，应该优先接 `/wunder` 还是 `/wunder/chat/*`？

优先 `/wunder/chat/*`，并配合 `/wunder/chat/ws`。`/wunder` 更适合能力调用型接入。

## SSE 和 WebSocket 怎么选？

实时聊天优先 WebSocket；SSE 作为兜底。

## token 统计为什么和账单不一致？

要分两层看：

- `round_usage.total_tokens` 是单轮请求完成后的实际上下文占用，也是当前统一口径。
- 实际总消耗按每次请求的 `round_usage.total_tokens` 逐次累加。

## 线程 system prompt 会在每轮重算吗？

不会。线程首次确定后会冻结，后续轮次不会改写该线程 system prompt。

## 长期记忆会在每轮自动重新注入吗？

不会。长期记忆只在线程初始化阶段注入一次。

## `temp_dir` 可以当长期存储目录吗？

不建议。`temp_dir` 是临时目录，业务长期数据应放数据库或工作区持久目录。

## 为什么工具在某个会话里看不到？

通常与工具挂载策略、运行形态能力、会话级参数或 MCP/A2A 启用状态有关。

## Desktop 模式是否必须先部署 Server？

不必须。Desktop 可本地独立运行；需要多租户治理和统一接入时再部署 Server。

## `web_fetch` 和浏览器工具有什么区别？

`web_fetch` 用于正文抓取；浏览器工具用于真实页面交互（点击、输入、导航）。

## 渠道接入失败通常先查哪里？

先查渠道运行态、Webhook 验签、outbox 投递，再看模型链路。

## 出现问题时先看哪一页？

先看 [故障排查](/docs/zh-CN/help/troubleshooting/)。
