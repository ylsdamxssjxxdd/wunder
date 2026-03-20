---
title: 配置说明
summary: 配置不生效时，先确认你改的是哪一层，而不是先怀疑程序没读到。
read_when:
  - 你在查具体配置应该写到哪里
  - 你要分清基础配置、运行时覆盖和外部 MCP 配置
source_docs:
  - config/wunder-example.yaml
  - docs/API文档.md
  - docs/系统介绍.md
---

# 配置说明

Wunder 的配置不是一个文件包打天下，而是分层组织的。

先记一条：排查配置问题时，先分清“基础配置、运行时覆盖、外部 MCP 配置”三层。

## 这页解决什么

- 具体配置应该改到哪里
- 哪些配置是 server 在读，哪些是 extra_mcp 在读
- 为什么你改了配置却可能没生效

## 先记住这几个配置入口

- `config/wunder.yaml`
- `config/wunder-example.yaml`
- `data/config/wunder.override.yaml`
- `extra_mcp/mcp_config.json`

## 它们分别负责什么

### `config/wunder.yaml`

- 正式基础配置
- 部署 server 时通常先看这里

### `config/wunder-example.yaml`

- 示例配置和兜底模板
- 正式配置缺失时，系统会按当前逻辑回退到这里

### `data/config/wunder.override.yaml`

- 运行时覆盖配置
- 通常对应管理端保存后的内容
- 不建议手工和基础配置混着改

### `extra_mcp/mcp_config.json`

- 独立 MCP 服务配置
- 尤其是数据库和知识库相关工具

## 按问题找配置

- 服务监听和并发限制，看 `server.*`
- 鉴权、命令和路径控制，看 `security.*`
- 外部 MCP 服务接入，看 `mcp.*`
- A2A 服务接入，看 `a2a.*`
- 存储与向量能力，看 `storage.*` 和 `vector_store.*`

## 你最可能先改这些项

- 服务基础：`server.host`、`server.port`、`server.chat_stream_channel`、`server.max_active_sessions`
- 安全控制：`security.api_key`、`security.external_auth_key`、`security.allow_commands`、`security.allow_paths`、`security.deny_globs`
- MCP：`mcp.timeout_s`、`mcp.servers[]`
- A2A：`a2a.timeout_s`、`a2a.services[]`

## 配置不生效时先查这三件事

1. 你改的是基础配置还是 override 配置
2. 当前运行实例实际读取的是哪个路径
3. 这个配置到底是 server、desktop 还是 extra_mcp 在消费

## 最容易搞错的点

- 把 `wunder-example.yaml` 当正式配置长期改。
- 管理端写入的 override 和手工改的基础配置相互打架。
- 以为所有配置都由 server 进程读取，实际上 `extra_mcp` 有自己的配置文件。

## 相关文档

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)
- [A2A 接口](/docs/zh-CN/integration/a2a/)
