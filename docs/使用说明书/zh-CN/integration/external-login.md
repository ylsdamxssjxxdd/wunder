---
title: 外部登录与免登嵌入
summary: 把外部系统里已登录的用户直接带进 wunder，免去再输一次密码；既能跳聊天页，也能跳到指定智能体。
---

# 外部登录与免登嵌入

如果用户已经在别的系统里登录了，想让他们不重复输密码就进入 wunder，用这组接口：

- `/wunder/auth/external/*`

它跟你自己的管理员后台登录是两回事。

## 适用范围

- 外部系统嵌入 wunder、免登跳转 → 用这里。
- 要把外部身份对齐到 wunder、发一个 wunder 自己的登录态 → 用这里。
- 普通的管理员登录或账号密码登录 → 不走这里。

不同外部系统的接法不一样：有的适合先要一次性凭证，再换取登录态；有的适合直接跳转。所以 wunder 保留了几种入口，你不必强行套同一条流程。

## 最短链路：外部身份换 wunder 会话

最常用的是用令牌换登录态：

1. 外部系统带来一个令牌和一个用户标识，也可以附带一个目标智能体名字；
2. wunder 换回自己的访问令牌，同时返回绑定的智能体 ID；
3. 如果带的名字正好命中当前用户能访问的已有智能体，会额外进入聚焦模式；
4. 前端统一进嵌入页：默认进消息页，命中智能体就进聚焦页，左侧栏隐藏。

如果不想在外部系统里先生成令牌，还有一个更短的入口，直接把用户标识带进来即可。

这条链路可用的入口：

- `POST /wunder/auth/external/login`
- `POST /wunder/auth/external/code`
- `POST /wunder/auth/external/launch`
- `POST /wunder/auth/external/token_launch`
- `POST /wunder/auth/external/token_login`
- `POST /wunder/auth/external/exchange`

## 会跳到哪

外链统一落在嵌入页里：默认消息页至少带“消息”分区，默认智能体会额外带“默认入口”，聚焦模式则带上命中的智能体 ID。要嵌智能体页也可以，同样不显示左侧栏。

wunder 不是只把令牌递回来，它还会根据是否命中指定智能体，决定你最终停在哪一页。嵌入用户在自己的会话里同样只有一个智能体实例。

## 关键设置与常见坑

- 这条链路有单独的密钥；没单独配时会回退到平台通用密钥，所以默认不是裸开的。
- 常见坑：只配了外部令牌却忘了密钥回退；只拿到令牌没处理返回的智能体 ID；想改当前线程的提示词，但外链只影响新线程；把免登入口当成了公开接口。

## 延伸阅读

- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [用户世界接口](/docs/zh-CN/integration/user-world/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
