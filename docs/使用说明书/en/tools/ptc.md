---
title: ptc
summary: The current behavior and return structure of the temporary Python script execution tool.
read_when:
  - You need a complete Python snippet to finish a local task
source_docs:
  - src/services/tools.rs
  - src/services/tools/catalog.rs
updated_at: 2026-04-10
---

# ptc

`ptc` stands for programmatic tool call. Its job is to:

- write a complete Python script into a temporary file
- run it under the specified `workdir`
- return `stdout`, `stderr`, and `returncode`

It is not a generic replacement for shell execution. For ordinary command-line work, prefer [Execute Command](/docs/en/tools/exec/).

## Minimum arguments

```json
{
  "filename": "helper.py",
  "content": "print('hello')"
}
```

## Success result

```json
{
  "ok": true,
  "action": "ptc",
  "state": "completed",
  "summary": "Executed Python script C:/.../ptc_temp/helper.py.",
  "data": {
    "path": "ptc_temp/helper.py",
    "workdir": ".",
    "returncode": 0,
    "stdout": "hello\n",
    "stderr": ""
  }
}
```

## Failure results

Common error codes:

- `TOOL_PTC_INVALID_FILENAME`
- `TOOL_PTC_CONTENT_REQUIRED`
- `TOOL_PTC_EXEC_ERROR`
- `TOOL_PTC_TIMEOUT`
- `TOOL_PTC_EXEC_FAILED`

On failure, `data` usually still preserves:

- `path`
- `workdir`
- `returncode`
- `stdout`
- `stderr`

## Usage boundary

Good fit:

- temporary data processing
- small Python parsers
- a few steps of Python logic that are not worth turning into a durable project file

Poor fit:

- multi-command shell workflows
- persistent script projects
- complex build pipelines

For those cases, [Execute Command](/docs/en/tools/exec/) is a better fit.
