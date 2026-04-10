---
title: FAQ
summary: wunder 的高频问题快速答复，适合在进入详细排障前先做判断。
read_when:
  - 你有高频使用疑问
  - 你想先快速判断是否属于故障
source_docs:
  - docs/API文档.md
  - frontend/src/views/LoginView.vue
  - frontend/src/views/MessengerView.vue
updated_at: 2026-04-10
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

- `round_usage.total_tokens` 是单轮请求完成后的实际上下文占用
- 实际总消耗按每次请求的 `round_usage.total_tokens` 逐次累加

## 登录页重置密码需要什么？

只需要：

- 用户名
- 邮箱
- 新密码

## 已登录后在哪里改用户名或密码？

去“我的概况 -> 编辑资料”。

这里可以：

- 改用户名
- 改邮箱
- 改登录密码

## 为什么新建线程按钮有时是灰的？

因为当前智能体还在运行。

前端会在运行中禁用 `新建线程`，避免主线程状态错位。等它完成，或先停止当前会话再新建。

## 蜂群和子智能体怎么选？

- 要调用已有其他智能体协作：用蜂群
- 要从当前会话临时派生一条子运行：用子智能体控制

## `temp_dir` 可以当长期存储目录吗？

不建议。`temp_dir` 是临时目录，业务长期数据应放数据库或工作区持久目录。

## 为什么工具在某个会话里看不到？

通常与工具挂载策略、运行形态能力、会话级参数或 MCP/A2A 启用状态有关。

## Desktop 模式是否必须先部署 Server？

不必须。Desktop 可本地独立运行；需要多租户治理和统一接入时再部署 Server。

## `web_fetch` 和浏览器工具有什么区别？

`web_fetch` 用于正文抓取；浏览器工具用于真实页面交互。

## 出现问题时先看哪一页？

先看 [故障排查](/docs/zh-CN/help/troubleshooting/)。
