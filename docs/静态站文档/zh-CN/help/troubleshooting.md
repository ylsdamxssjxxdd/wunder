---
title: 故障排查
summary: 排查 Wunder 问题时，优先从入口、鉴权、配置、依赖服务和实时通道五个方向切。
read_when:
  - Wunder 跑不起来
  - 你已经确定不是简单使用疑问，而是真故障
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - config/wunder-example.yaml
---

# 故障排查

排查 Wunder，不要一上来翻所有日志。

先按下面顺序切问题。

## 1. 入口是否通

先检查这些入口：

- `/wunder`
- `/wunder/chat/ws`
- `/a2a/agentCard`
- `/wunder/mcp`
- `/docs/`

如果入口都不通，先不要继续查上层逻辑。

## 2. 鉴权是否对

常见问题：

- 管理端接口却用了用户 Token
- 用户侧接口却用了 API Key
- `/a2a` 或 `/wunder/mcp` 没带 API Key
- 外链嵌入没有配置 `external_auth_key`

## 3. 配置是否生效

优先确认：

- 实际读取的是 `config/wunder.yaml` 还是 example
- 管理端是否写入了 `data/config/wunder.override.yaml`
- 你改的是 server 配置还是 extra_mcp 配置

## 4. 依赖服务是否就绪

服务端部署最常见的问题是依赖没起来：

- PostgreSQL 未就绪
- sandbox 不可达
- `extra_mcp` 未启动
- 外部 MCP/A2A 目标不可达

## 5. 实时通道是否异常

如果聊天收不到过程：

1. 先看 `/wunder/chat/ws` 是否能建连
2. 再看是否已自动回退 SSE
3. 再看 `resume/watch` 是否传错 `session_id` 或 `after_event_id`

## 6. 工具为什么没出现

优先检查：

- 工具是否在配置里启用
- MCP server 是否 enabled
- A2A 服务是否 enabled
- 当前智能体或会话是否真的挂载了目标工具

## 7. 为什么模型一直报权限或审批

排查这几个方向：

- 当前 `approval_mode`
- 命令与路径白名单
- sandbox 是否开启
- 工具是否进入了等待审批态但前端没正确回传

## 8. Desktop 本地模式常见问题

最常见的是这几类：

- 误以为必须先部署 server
- 本地工作目录没持久化
- 远端接入配置错误后以为本地模式也坏了
- 以为可以自定义 Python 路径，但当前默认是附带运行时优先

## 什么时候该回完整文档

如果你排查到这里还没定位，下一步通常回这几类文档：

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [配置说明](/docs/zh-CN/reference/config/)

