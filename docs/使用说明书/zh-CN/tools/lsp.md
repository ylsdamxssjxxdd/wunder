---
title: LSP 查询
summary: `lsp_query` 的操作类型、位置参数与返回结构。
read_when:
  - 你要查定义、引用、悬停、实现、调用层级或符号
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# LSP 查询

`lsp_query` 不是读文件工具，而是符号级查询工具。  
先用它的正确顺序通常是：

1. 用文件工具定位路径
2. 再用 `lsp_query` 查定义/引用

## 最小参数

查定义：

```json
{
  "operation": "definition",
  "path": "src/services/tools.rs",
  "line": 120,
  "character": 18
}
```

查工作区符号：

```json
{
  "operation": "workspaceSymbol",
  "path": "src/services/tools.rs",
  "query": "build_model_tool_success"
}
```

## 支持的主要操作

- `definition`
- `references`
- `hover`
- `documentSymbol`
- `workspaceSymbol`
- `implementation`
- `callHierarchy`

其中这些操作需要位置：

- `definition`
- `references`
- `hover`
- `implementation`
- `callHierarchy`

## 成功返回

```json
{
  "ok": true,
  "action": "lsp_query",
  "state": "completed",
  "summary": "Ran LSP definition on src/services/tools.rs across 2 servers.",
  "data": {
    "operation": "definition",
    "path": "src/services/tools.rs",
    "results": [
      {
        "server_id": "rust-analyzer",
        "server_name": "rust-analyzer",
        "result": [ ... ]
      }
    ],
    "server_count": 2
  }
}
```

如果没有任何 LSP 服务器返回结果，可能还会有：

```json
{
  "next_step_hint": "No LSP servers returned a result for this file..."
}
```

## 注意点

- `line` 和 `character` 在输入里是 1-based
- 内部发给 LSP 时会转换成 0-based
- `path` 必须是真实存在的文件

## 什么时候别用它

- 只是想看文件内容：用 [工作区文件](/docs/zh-CN/tools/workspace-files/)
- 只是全文搜索文本：用 [工作区文件](/docs/zh-CN/tools/workspace-files/) 里的 `search_content`
