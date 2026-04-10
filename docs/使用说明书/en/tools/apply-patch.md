---
title: Apply Patch
summary: The precise editing semantics, success result, and patch-specific failure codes of `apply_patch`.
read_when:
  - You need a small, reviewable, replayable, and precise edit
source_docs:
  - src/services/tools/apply_patch_tool.rs
  - src/services/tools/tool_apply_patch.lark
updated_at: 2026-04-10
---

# Apply Patch

`apply_patch` is currently the best tool for edits that involve a small number of files, a small number of hunks, and explicit surrounding context.

Its role is narrow by design:

- it is not a whole-file writing tool
- it is not a command execution tool
- it is a structured, incremental, and verifiable tool for precise edits

## The input is grammar text, not JSON Patch

Minimum example:

```text
*** Begin Patch
*** Update File: src/main.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
```

From the model side, the only common inputs are:

- `input`
- `dry_run`

## Success result

```json
{
  "ok": true,
  "action": "apply_patch",
  "state": "completed",
  "summary": "Applied patch touching 2 files.",
  "data": {
    "changed_files": 2,
    "added": 1,
    "updated": 1,
    "deleted": 0,
    "moved": 0,
    "hunks_applied": 3,
    "files": [
      {
        "action": "update",
        "path": "src/main.rs",
        "to_path": null,
        "hunks": 1
      }
    ],
    "lsp": [
      {
        "path": "C:/.../src/main.rs",
        "state": {
          "enabled": true,
          "matched": true,
          "touched": true
        }
      }
    ]
  }
}
```

## `dry_run`

```json
{
  "ok": true,
  "action": "apply_patch",
  "state": "dry_run",
  "summary": "Validated patch touching 2 files without applying it.",
  "data": {
    "dry_run": true,
    "changed_files": 2,
    "added": 1,
    "updated": 1,
    "deleted": 0,
    "moved": 0,
    "hunks_applied": 3,
    "files": [ ... ],
    "lsp": []
  }
}
```

## Failure results

`apply_patch` still uses the unified failure envelope, but its error codes are more specific than those of general file tools:

- `PATCH_LIMIT_INPUT_TOO_LARGE`
- `PATCH_FORMAT_EMPTY_PATCH`
- `PATCH_LIMIT_TOO_MANY_FILE_OPS`
- `PATCH_LIMIT_TOO_MANY_CHUNKS`
- `PATCH_RUNTIME_TASK_FAILED`

It can also emit parsing errors, path-escape errors, target conflicts, and context-mismatch errors that are specific to patch application.

## When to use it and when not to

Good fit:

- changing a few lines of code
- adding a small helper function
- adjusting a few independent files

Poor fit:

- rewriting a whole file
- generating a large amount of documentation or assets
- edits that require running a script before the final content is known

For those cases, consider:

- whole-file replacement: `write_file`
- command-first workflows: `execute_command`

## Difference from `write_file`

- `apply_patch`: preserves context and is easier to review
- `write_file`: writes the final content directly

If you already know exactly what the entire new file should be, `write_file` is more direct.  
If you only need a precise change, `apply_patch` is safer.
