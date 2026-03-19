---
title: 外部登录与免登嵌入
summary: Wunder 保留 `/wunder/auth/external/*` 作为外部系统嵌入和免登接入面。
read_when:
  - 你要从外部系统直接进入 Wunder
  - 你想分清 external login、launch 和 token_login 的用途
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - config/wunder-example.yaml
  - src/api/auth.rs
---

# 外部登录与免登嵌入

Wunder 当前保留了一整组外部接入接口：

- `/wunder/auth/external/*`

它们解决的不是普通管理员登录，而是：

- 外部系统嵌入
- 免登跳转
- 对齐用户身份
- 发放 Wunder 自己的登录态

## 为什么不能直接复用普通登录

因为外部系统接入通常有这些特点：

- 用户已经在外部系统里登录了
- Wunder 只负责承接会话，不适合再让用户输一遍密码
- 登录后还要直接跳到指定聊天或嵌入页面

所以 Wunder 专门保留了 external 接入面。

## 常见接口

当前代码里至少有这些入口：

- `POST /wunder/auth/external/login`
- `POST /wunder/auth/external/code`
- `POST /wunder/auth/external/launch`
- `POST /wunder/auth/external/token_launch`
- `POST /wunder/auth/external/token_login`
- `POST /wunder/auth/external/exchange`

如果你只记一个最常见场景，优先记：

- `token_login`

## `token_login` 现在适合什么

当前最典型的用法是：

- 外部系统拿 `token + user_id`
- Wunder 直接换出自己的 `access_token`
- 同时返回 `agent_id`
- 前端直接跳嵌入聊天页

也就是说，它更像“外部身份换 Wunder 会话”的桥接接口。

## 为什么还会有 launch / code

因为不同外部系统的接法不一样。

有些系统适合：

- 先申请一次性 code
- 再交换登录态

有些系统适合：

- 直接 launch
- 直接跳目标页面

Wunder 保留这些入口，是为了兼容不同嵌入方式，而不是要求所有接入方都走一条固定流程。

## 安全边界靠什么

这条链路的关键配置是：

- `security.external_auth_key`

如果它没显式配置，当前会自动回退到：

- `security.api_key`

所以默认并不是“裸开”。

## 接入后会跳到哪里

当前嵌入聊天最典型的目标路由是：

- `/app/embed/chat`
- `/desktop/embed/chat`

也就是说，Wunder 不是只返回 token，还会给出一个适合前端直接进入的落点。

## 这套链路适合什么场景

适合：

- 统一门户嵌入 Wunder
- 外部系统单点进入指定智能体
- 团队系统把用户身份带进 Wunder

不适合：

- 替代管理员后台登录
- 替代普通用户账号密码体系

## 最容易忽略的问题

- 只配了外部 JWT，却没配 external_auth_key 回退
- 只拿到 token，没有处理返回的 `agent_id`
- 想改当前线程提示词，却忘了外链只会影响新线程
- 把外部免登当成了普通开放接口

## 相关文档

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [用户世界接口](/docs/zh-CN/integration/user-world/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
