---
title: LSP Query
summary: The operation types, position arguments, and return structure of `lsp_query`.
read_when:
  - You need definitions, references, hover info, implementations, call hierarchies, or symbols
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# LSP Query

`lsp_query` is not a file-reading tool. It is a symbol-level query tool.  
The correct order is usually:

1. use file tools to locate the path first
2. then use `lsp_query` for definitions or references

## Minimum arguments

Definition lookup:

```json
{
  "operation": "definition",
  "path": "src/services/tools.rs",
  "line": 120,
  "character": 18
}
```

Workspace symbol lookup:

```json
{
  "operation": "workspaceSymbol",
  "path": "src/services/tools.rs",
  "query": "build_model_tool_success"
}
```

## Main supported operations

- `definition`
- `references`
- `hover`
- `documentSymbol`
- `workspaceSymbol`
- `implementation`
- `callHierarchy`

These operations require a position:

- `definition`
- `references`
- `hover`
- `implementation`
- `callHierarchy`

## Success result

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

If no LSP server returned anything, the result may also include:

```json
{
  "next_step_hint": "No LSP servers returned a result for this file..."
}
```

## Notes

- `line` and `character` are 1-based in tool input
- they are converted to 0-based positions before the actual LSP request
- `path` must point to a real file

## When not to use it

- If you only need file content, use [Workspace Files](/docs/en/tools/workspace-files/)
- If you only need full-text search, use `search_content` from [Workspace Files](/docs/en/tools/workspace-files/)
