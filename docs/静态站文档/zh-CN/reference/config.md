---
title: 配置说明
summary: Wunder 的稳定配置入口主要集中在 `config/wunder.yaml`、override 文件和 extra_mcp 配置。
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

## 先看这几个文件

- `config/wunder.yaml`
- `config/wunder-example.yaml`
- `data/config/wunder.override.yaml`
- `extra_mcp/mcp_config.json`

## 它们分别负责什么

### `config/wunder.yaml`

正式基础配置。

如果你在部署 server，这通常是第一入口。

### `config/wunder-example.yaml`

示例配置和兜底模板。

如果正式配置缺失，系统会按当前逻辑回退到这里。

### `data/config/wunder.override.yaml`

运行时覆盖配置。

这通常对应管理端保存后的内容，不建议手工和基础配置混着改。

### `extra_mcp/mcp_config.json`

独立 MCP 服务配置，尤其是数据库和知识库相关工具。

## 最常见的配置块

- `server`
- `security`
- `mcp`
- `a2a`
- `channels`
- `storage`
- `vector_store`

## 你最可能先改哪些项

### 服务基础

- `server.host`
- `server.port`
- `server.chat_stream_channel`
- `server.max_active_sessions`

### 安全

- `security.api_key`
- `security.external_auth_key`
- `security.allow_commands`
- `security.allow_paths`
- `security.deny_globs`

### MCP

- `mcp.timeout_s`
- `mcp.servers[]`

### A2A

- `a2a.timeout_s`
- `a2a.services[]`

## 一个实际建议

如果你在排查“配置改了怎么没生效”，优先确认三件事：

1. 你改的是基础配置还是 override 配置
2. 当前运行实例实际读取的是哪个路径
3. 这个配置到底是 server、desktop 还是 extra_mcp 在消费

## 相关文档

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)
- [A2A 接口](/docs/zh-CN/integration/a2a/)
