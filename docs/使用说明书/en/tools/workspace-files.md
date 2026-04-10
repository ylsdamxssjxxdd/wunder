---
title: Workspace Files
summary: The latest arguments and return structures for `list_files`, `search_content`, `read_file`, and `write_file`.
read_when:
  - You need to browse the workspace, search code, read files, or write files
source_docs:
  - src/services/tools.rs
  - src/services/tools/search_content_tool.rs
updated_at: 2026-04-10
---

# Workspace Files

This page covers the four most commonly used local tools:

- `list_files`
- `search_content`
- `read_file`
- `write_file`

All four now use the unified success and failure envelope.

## When to prefer them

- To inspect a directory first: `list_files`
- To locate a keyword first: `search_content`
- To read a known path and a specific range: `read_file`
- To create or overwrite a whole file: `write_file`

## `list_files`

### Minimum arguments

```json
{
  "path": ".",
  "max_depth": 2,
  "limit": 200
}
```

### Success result

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

Important fields:

- `items`: the directory entries, with `/` appended to directories
- `next_cursor`: used to continue pagination when a directory is large
- `has_more`: whether another page exists

## `search_content`

### Minimum arguments

```json
{
  "query": "spawn_agent",
  "path": "src",
  "max_matches": 50
}
```

### Common arguments

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

### Success result

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

Important fields:

- `hits`: the actual search matches meant for the model to read
- `truncated` and `truncation_reasons`: mandatory to inspect when the result was cut by budget
- in `dry_run` mode, the tool returns the search plan rather than real matches

## `read_file`

### Minimum arguments

```json
{
  "path": "src/services/tools.rs",
  "start_line": 1,
  "end_line": 120
}
```

Batch mode is also supported:

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

### Success result

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

Important fields:

- `content`: the assembled text block ready to read directly
- `files`: the per-file summary
- `continuation_required`: appears when the default window was not enough or output was truncated by budget

## `write_file`

### Minimum arguments

```json
{
  "path": "docs/demo.md",
  "content": "# Demo"
}
```

### Success result

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

### When not to use it

- Do not use `write_file` for small, precise code edits. Use [Apply Patch](/docs/en/tools/apply-patch/) instead.
- Do not use it to run scripts or builds. Use [Execute Command](/docs/en/tools/exec/) instead.

## How to read failures

For all four tools, inspect these first on failure:

- `error_meta.code`
- `error_meta.hint`
- `data`

Common error codes:

- `TOOL_LIST_PATH_NOT_FOUND`
- `TOOL_SEARCH_INVALID_ARGS`
- `TOOL_SEARCH_PATH_NOT_FOUND`
- `TOOL_READ_NOT_FOUND`
- `TOOL_READ_BINARY_FILE`
- `TOOL_WRITE_PATH_REQUIRED`
- `TOOL_WRITE_FAILED`
