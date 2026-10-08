---
title: MCP 入口
summary: wunder 同时支持自托管 MCP 服务 `/wunder/mcp` 与外部 MCP 服务接入。
read_when:
  - 用户要把 wunder 作为 MCP 服务暴露出去
  - 用户要理解 wunder 内部 MCP 与 extra_mcp 的关系
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/services/mcp.rs
  - config/wunder-example.yaml
---

# MCP 入口

当前 Rust MCP 使用 rmcp 3.5.0，内置服务通过 Streamable HTTP 支持 MCP 2026-07-28 的请求级元数据协商。外部服务接入也统一使用 SDK 的 Streamable HTTP 客户端；旧版 SSE 传输和自写初始化客户端已移除。

MCP 在 wunder 里是正式接入面。

先分清两件事：

1. wunder 自己暴露的 MCP 服务是 `/wunder/mcp`
2. wunder 也可以作为 MCP 客户端去接别的服务，比如 `extra_mcp`

## 自托管 MCP 端点

- `POST /wunder/mcp`
- 传输方式：Streamable HTTP
- 协议协商：SDK 自动优先使用 2026-07-28；与旧服务互通时由 SDK 按支持列表协商，不要手写 `initialize`

当前 Rust 端内置暴露两个工具：

- `excute`
- `doc2md`

注意，工具名当前实际就是 `excute`。这是现有公开工具标识，调用方应按该名称传递。

## 自托管 MCP 的适用场景

适合这些场景：

- 让外部系统通过 MCP 方式调用 wunder 的能力
- 把 wunder 挂入另一个 MCP 编排体系
- 在同一套协议下暴露内部任务执行与文档解析能力

## 配置示例

```yaml
mcp:
  servers:
    - name: wunder
      endpoint: http://127.0.0.1:8000/wunder/mcp
      enabled: false
      transport: streamable-http
```

启用后，wunder 会把这个 MCP 服务视为一个可调用的 MCP server。

## external MCP 和 extra_mcp

仓库里还保留了一个典型外部 MCP 服务：

- `extra_mcp`

它通常用来承载：

- `db_query`
- `db_export`
- `kb_query`

`extra_mcp` 默认也使用 Streamable HTTP。Python 服务的 SDK 依赖由其独立运行环境管理；它会按 Python SDK 支持的版本与客户端协商，不能假定其具备 Rust rmcp 3.5.0 的全部 2026-07-28 扩展。

也就是说：

- `/wunder/mcp` 偏“wunder 自己暴露出去”
- `extra_mcp` 偏“wunder 去接进来的外部能力”

## 舰桥查看 MCP

舰桥已有一套 MCP 配置和调试入口，用来：

- 配置 `mcp.servers`
- 刷新工具清单
- 调试远端工具调用

因此文档接入顺序通常是：

1. 先在配置里声明 MCP server
2. 再在舰桥确认工具规格是否能拉到
3. 最后在 agent 或工具目录里开放给模型使用

## MCP 和工具目录的关系

wunder 不会把 MCP 当成“旁路系统”。

它会把 MCP 工具和这些能力一起汇总进工具视图：

- 内置工具
- A2A 工具
- Skills
- 知识库工具
- 用户自建工具

所以对模型来说，MCP 最终还是工具目录的一部分。

## 延伸阅读

- [工具体系](/docs/zh-CN/concepts/tools/)
- [A2A 接口](/docs/zh-CN/integration/a2a/)
- [配置说明](/docs/zh-CN/reference/config/)
