---
title: MCP 入口
summary: Wunder 同时支持自托管 MCP 服务 `/wunder/mcp` 与外部 MCP 服务接入。
read_when:
  - 你要把 Wunder 作为 MCP 服务暴露出去
  - 你要理解 Wunder 内部 MCP 与 extra_mcp 的关系
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - src/services/mcp.rs
  - config/wunder-example.yaml
---

# MCP 入口

MCP 在 Wunder 里不是附属能力，而是正式接入面。

当前你需要先分清两件事：

1. Wunder 自己暴露的 MCP 服务是 `/wunder/mcp`
2. Wunder 也可以作为 MCP 客户端去接别的服务，比如 `extra_mcp`

## 自托管 MCP 端点

- `POST /wunder/mcp`
- 传输方式：Streamable HTTP

当前 Rust 端内置暴露两个工具：

- `excute`
- `doc2md`

注意，工具名当前实际就是 `excute`，文档需要按代码现状理解，不要自行改写成 `execute`。

## 自托管 MCP 适合什么

适合这些场景：

- 让外部系统通过 MCP 方式调用 Wunder 的能力
- 把 Wunder 挂入另一个 MCP 编排体系
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

启用后，Wunder 会把这个 MCP 服务视为一个可调用的 MCP server。

## external MCP 和 extra_mcp

仓库里还保留了一个典型外部 MCP 服务：

- `extra_mcp`

它通常用来承载：

- `db_query`
- `db_export`
- `kb_query`

也就是说：

- `/wunder/mcp` 偏“Wunder 自己暴露出去”
- `extra_mcp` 偏“Wunder 去接进来的外部能力”

## 管理端怎么看 MCP

管理端已有一套 MCP 配置和调试入口，用来：

- 配置 `mcp.servers`
- 刷新工具清单
- 调试远端工具调用

因此文档接入顺序通常是：

1. 先在配置里声明 MCP server
2. 再在管理端确认工具规格是否能拉到
3. 最后在 agent 或工具目录里开放给模型使用

## MCP 和工具目录的关系

Wunder 不会把 MCP 当成“旁路系统”。

它会把 MCP 工具和这些能力一起汇总进工具视图：

- 内置工具
- A2A 工具
- Skills
- 知识库工具
- 用户自建工具

所以对模型来说，MCP 最终还是工具目录的一部分。

## 相关文档

- [工具体系](/docs/zh-CN/concepts/tools/)
- [A2A 接口](/docs/zh-CN/integration/a2a/)
- [配置说明](/docs/zh-CN/reference/config/)
