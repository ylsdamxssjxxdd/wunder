---
title: 执行命令
summary: `execute_command` 的预算、dry-run、输出守卫与返回结构。
read_when:
  - 你要运行 shell 命令、编译、测试或调用现成 CLI
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# 执行命令

`execute_command` 现在不是“随便扔一串 shell 文本然后读 stdout”那么简单了。它的关键变化有三点：

- 支持 `dry_run`
- 支持命令预算与输出守卫
- 会把误传入的 patch 文本自动拦截到 `apply_patch`

## 最小参数

```json
{
  "content": "cargo check --release"
}
```

## 常用参数

- `content`
- `workdir`
- `timeout_s`
- `budget`
- `dry_run`

`budget` 常见字段：

- `time_budget_ms`
- `output_budget_bytes`
- `max_commands`

## 成功返回

```json
{
  "ok": true,
  "action": "execute_command",
  "state": "completed",
  "summary": "Executed 1 commands.",
  "data": {
    "results": [
      {
        "command": "cargo check --release",
        "command_index": 0,
        "command_session_id": "cmd_xxx",
        "returncode": 0,
        "stdout": "...",
        "stderr": "",
        "output_meta": {
          "truncated": false,
          "total_bytes": 1024,
          "omitted_bytes": 0,
          "stdout": { "truncated": false },
          "stderr": { "truncated": false }
        }
      }
    ],
    "budget": {
      "time_budget_ms": 60000,
      "output_budget_bytes": 32768,
      "max_commands": 4
    },
    "output_guard": {
      "truncated": false,
      "commands": 1,
      "truncated_commands": 0,
      "total_bytes": 1024,
      "omitted_bytes": 0,
      "effective_total_bytes": 32768
    },
    "sandbox": false
  }
}
```

如果输出被守卫裁剪，还会出现：

```json
{
  "next_step_hint": "Command output was truncated by the output guard..."
}
```

## `dry_run`

```json
{
  "ok": true,
  "action": "execute_command",
  "state": "dry_run",
  "summary": "Validated command plan without execution.",
  "data": {
    "dry_run": true,
    "workdir": "C:/.../workspace",
    "command_count": 1,
    "commands": ["cargo check --release"],
    "timeout_s": 60,
    "budget": { ... },
    "output_guard": { ... },
    "sandbox": false
  }
}
```

## 失败返回

非零退出、超时、工作目录错误都走统一失败骨架。常见错误码：

- `TOOL_EXEC_COMMAND_REQUIRED`
- `TOOL_EXEC_WORKDIR_NOT_FOUND`
- `TOOL_EXEC_WORKDIR_NOT_DIR`
- `TOOL_EXEC_NOT_ALLOWED`
- `TOOL_EXEC_TIMEOUT`
- `TOOL_EXEC_NON_ZERO_EXIT`
- `TOOL_EXEC_BUDGET_COMMAND_LIMIT`

超时或非零退出时，`data.results` 里仍会保留已经收集到的 stdout/stderr，方便继续判断。

## 特殊行为：误把 patch 文本传进来

如果 `content` 里不是命令，而是完整的 `*** Begin Patch ... *** End Patch`，系统会自动转去执行 `apply_patch`，并在结果上补一个字段：

```json
{
  "intercepted_from": "execute_command"
}
```

## 什么时候别用它

- 只想小范围改文件：用 [应用补丁](/docs/zh-CN/tools/apply-patch/)
- 纯 Python 临时程序：用 [ptc](/docs/zh-CN/tools/ptc/)
- 只是想读代码：用 [工作区文件](/docs/zh-CN/tools/workspace-files/)
