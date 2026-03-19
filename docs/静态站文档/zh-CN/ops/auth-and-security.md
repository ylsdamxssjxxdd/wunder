---
title: 认证与安全
summary: Wunder 需要同时区分 API Key、用户 Token、外链鉴权和工具执行边界。
read_when:
  - 你要把 Wunder 放到可长期运行环境
  - 你要判断哪些入口该用哪种凭证
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - src/core/auth.rs
  - config/wunder-example.yaml
---

# 认证与安全

Wunder 不是只有一种身份和一种入口，所以认证不能只看“有没有 Token”。

## 先分清四类凭证

### API Key

主要给这些入口用：

- `/wunder`
- `/a2a`
- `/wunder/mcp`
- 管理端接口

传法通常是：

- `X-API-Key`
- `Authorization: Bearer <api_key>`

### 用户 Bearer Token

主要给用户侧接口用：

- `/wunder/chat/*`
- `/wunder/user_world/*`
- `/wunder/workspace/*`
- `/wunder/user_tools/*`

### 外链嵌入鉴权

主要给外部系统嵌入 Wunder 时用：

- `/wunder/auth/external/*`

对应配置项：

- `security.external_auth_key`

### 登录后用户身份

这层是注册用户、单位、组织治理对应的真实业务身份。

它和 `/wunder` 调用里传入的任意 `user_id` 不是一个概念。

## 一个容易混淆但很重要的约定

`/wunder` 传入的 `user_id` 不要求一定是已注册用户。

它可以只是：

- 线程隔离标识
- 工作区隔离标识
- 外部系统映射标识

真实注册用户管理，走的是用户体系接口。

## 路径保护边界

当前实现里，大致可以这样理解：

- `/a2a` 按受保护入口处理
- `/wunder/mcp` 按受保护入口处理
- 用户聊天和工作区路径属于用户态接口
- `/docs/` 作为静态文档站独立于这些业务鉴权路径

把文档放到 `/docs/`，就是为了避免把它卷进 `/wunder/*` 的鉴权逻辑里。

## 工具执行安全

Wunder 的安全边界不只在 HTTP 层，还在工具层。

关键约束包括：

- `allow_commands`
- `allow_paths`
- `deny_globs`
- sandbox 下沉执行

也就是说，即使模型能调用工具，真正能做什么仍受配置约束。

## 线程安全约定

这两个约定非常重要：

- 线程的 system prompt 一旦首次确定后必须冻结
- 长期记忆只允许在线程初始化时注入一次

这样做不是形式要求，而是为了保证：

- 提示词缓存稳定
- 线程行为可预测
- 长任务不会因系统提示词被反复改写而漂移

## WebSocket 与审批隔离

当前系统已经把待审批请求统一放到共享注册表里，但不同入口仍按 `source` 隔离消费。

这意味着：

- chat/ws 的审批不会误清理渠道审批
- 渠道侧审批也不会误操作 WebSocket 会话

## 你应该优先检查哪些安全配置

- `security.api_key`
- `security.external_auth_key`
- `allow_commands`
- `allow_paths`
- `deny_globs`
- sandbox 是否启用

## 相关文档

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [配置说明](/docs/zh-CN/reference/config/)
