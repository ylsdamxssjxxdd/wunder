---
title: 工作区文件
summary: `list_files`、`search_content`、`read_file`、`write_file` 的最新参数与返回结构。
read_when:
  - 用户要浏览工作区、搜索代码、读文件或写文件
source_docs:
  - src/services/tools.rs
  - src/services/tools/search_content_tool.rs
updated_at: 2026-04-10
---

# 工作区文件

这一页覆盖四个最常用的本地工具：

- `list_files`
- `search_content`
- `read_file`
- `write_file`
- `edit_file2`

它们都已经走统一成功/失败骨架。

## 优先使用场景

- 先看目录：`list_files`
- 先定位关键词：`search_content`
- 已经知道路径，读具体片段：`read_file`
- 整文件创建或覆盖：`write_file`
- 一次精确文本替换：`edit_file2`

## `list_files`

### 最小参数

```json
{
  "path": ".",
  "max_depth": 2,
  "limit": 200
}
```

### 成功返回

```json
{
  "ok": true,
  "action": "list_files",
  "state": "completed",
  "summary": "Listed 120 entries from frontend/src.",
  "data": {
    "path": "frontend/src",
    "items": ["views/", "components/", "main.ts"],
    "offset": 0,
    "limit": 200,
    "returned": 120,
    "has_more": false,
    "next_offset": null,
    "next_cursor": null,
    "max_depth": 2
  }
}
```

重点字段：

- `items`：目录项列表，目录会带 `/`
- `next_cursor`：目录很多时继续翻页用
- `has_more`：是否还有下一页

## `search_content`

### 最小参数

```json
{
  "query": "spawn_agent",
  "path": "src",
  "max_matches": 50
}
```

### 常用参数

- `query` / `pattern`
- `path`
- `file_pattern`
- `query_mode`
- `context_before`
- `context_after`
- `max_matches`
- `max_files`
- `max_candidates`
- `budget`
- `dry_run`

### 成功返回

```json
{
  "ok": true,
  "action": "search_content",
  "state": "completed",
  "summary": "Found 8 hits in 3 files.",
  "data": {
    "query": "spawn_agent",
    "query_used": "spawn_agent",
    "path": "src",
    "query_mode": "literal",
    "matched_file_count": 3,
    "returned_match_count": 8,
    "truncated": false,
    "truncation_reasons": [],
    "elapsed_ms": 132,
    "hits": [
      {
        "path": "services/tools/subagent_control.rs",
        "line": 240,
        "content": "spawn(context, args).await",
        "segments": [
          { "text": "spawn", "matched": true }
        ],
        "matched_terms": ["spawn_agent"],
        "before": [],
        "after": []
      }
    ]
  }
}
```

重点字段：

- `hits`：真正给模型读的搜索命中
- `truncated` / `truncation_reasons`：结果被预算截断时必须看
- `dry_run` 时会返回搜索计划而不是命中结果

## `read_file`

### 最小参数

```json
{
  "path": "src/services/tools.rs",
  "start_line": 1,
  "end_line": 120
}
```

也支持批量：

```json
{
  "files": [
    {
      "path": "src/services/tools.rs",
      "line_ranges": [[1, 120]]
    }
  ]
}
```

### 成功返回

```json
{
  "ok": true,
  "action": "read_file",
  "state": "completed",
  "summary": "Read 1 files.",
  "data": {
    "content": ">>> src/services/tools.rs\n1: use ...",
    "files": [
      {
        "path": "src/services/tools.rs",
        "mode": "slice",
        "requested_ranges": [[1, 120]],
        "loaded_ranges": [[1, 120]],
        "read_lines": 120,
        "total_lines": 3000,
        "complete": false,
        "truncated_by_size": false,
        "used_default_range": false
      }
    ],
    "dry_run": false,
    "requested_files": 1,
    "processed_files": 1,
    "budget_file_limit_hit": false,
    "timeout_hit": false,
    "output_budget_hit": false,
    "output_budget_omitted_bytes": 0,
    "content_bytes_before_budget": 4096,
    "budget": {
      "time_budget_ms": null,
      "output_budget_bytes": null,
      "max_files": null
    },
    "continuation_required": true,
    "continuation_hint": "..."
  }
}
```

重点字段：

- `content`：拼好的可直接阅读文本
- `files`：每个文件的摘要
- `continuation_required`：默认窗口不够或预算截断时会出现

## `edit_file2`

### 最小参数

```json
{
  "path": "docs/demo.md",
  "old_text": "old",
  "new_text": "new"
}
```

### 适用场景

- 只做一次精确文本替换
- `old_text` 必须和文件当前内容完全一致
- 删除文本时把 `new_text` 设为空字符串
- 如果要一次替换多处，可加 `expected_count`；工具会要求匹配次数完全一致
- 如果误把 `read_file` 的文件头或行号复制进 `old_text`，工具会在匹配失败时尝试自动剥离
- 如果文件换行符是 CRLF，工具会按目标文件换行风格重试匹配
- 如果替换已经唯一完成，工具会返回 `already_applied`，避免重复调用报错

### 成功返回

```json
{
  "ok": true,
  "action": "edit_file2",
  "state": "completed",
  "summary": "Updated file docs/demo.md with 1 edit steps.",
  "data": {
    "path": "docs/demo.md",
    "dry_run": false,
    "ensure_newline": false,
    "existed": true,
    "previous_bytes": 12,
    "bytes": 12,
    "edit_count": 1,
    "already_applied_count": 0,
    "edits": [
      {
        "action": "replace",
        "changed": true,
        "already_applied": false,
        "matches": 1,
        "bytes": 3
      }
    ]
  }
}
```

### 优先使用场景

- 用户已经通过 `read_file` 拿到精确原文
- 目标修改能写成一次 `old_text` -> `new_text`
- 不需要正则、条件判断、标记间替换或多步骤编辑
- `old_text` 匹配多处时，优先扩大上下文；确实要替换全部匹配时再设置 `expected_count`

### 不适用场景

- 要做复杂替换、正则替换、跨多段编辑或带条件逻辑时，改用 `programmatic_tool_call` 写 Python 脚本。
- 要整文件生成或覆盖时，改用 `write_file`。
- 要小范围代码补丁且需要上下文审查时，改用 [应用补丁](/docs/zh-CN/tools/apply-patch/)。

## `write_file`

### 最小参数

```json
{
  "path": "docs/demo.md",
  "content": "# Demo"
}
```

### 成功返回

```json
{
  "ok": true,
  "action": "write_file",
  "state": "completed",
  "summary": "Created file docs/demo.md.",
  "data": {
    "path": "docs/demo.md",
    "bytes": 7,
    "dry_run": false,
    "existed": false,
    "previous_bytes": 0,
    "target": "C:/.../docs/demo.md",
    "lsp": {
      "enabled": true,
      "matched": true,
      "touched": true,
      "diagnostics": null,
      "error": null
    }
  }
}
```

### 不适用场景

- 小范围精确修改代码，不要用 `write_file`，改用 [应用补丁](/docs/zh-CN/tools/apply-patch/)
- 要执行脚本、跑构建，不要用它，改用 [执行命令](/docs/zh-CN/tools/exec/)

## 失败返回的阅读

这四个工具失败时都优先看：

- `error_meta.code`
- `error_meta.hint`
- `data`

常见错误码：

- `TOOL_LIST_PATH_NOT_FOUND`
- `TOOL_SEARCH_INVALID_ARGS`
- `TOOL_SEARCH_PATH_NOT_FOUND`
- `TOOL_READ_NOT_FOUND`
- `TOOL_READ_BINARY_FILE`
- `TOOL_WRITE_PATH_REQUIRED`
- `TOOL_WRITE_FAILED`
