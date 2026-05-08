---
title: Apply Patch
summary: The precise editing semantics, success result, and patch-specific failure codes of `apply_patch`.
read_when:
  - You need a small, reviewable, replayable, and precise edit
source_docs:
  - src/services/tools/apply_patch_tool.rs
  - src/services/tools/tool_apply_patch.lark
updated_at: 2026-05-08
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

## Minimal workflow for weaker models

Follow this order for the highest success rate:

1. Use `read_file` to fetch the target file or exact excerpt first
2. Build only one very small patch at a time
3. If you are unsure whether the context is stable, add `dry_run` first
4. Only apply for real after the preview succeeds
5. If the edit spans many distant regions or is close to a whole-file rewrite, switch to `write_file`

## Hard rules for Update File hunks

- After `@@`, every line must start with a prefix: space / `-` / `+`
- A blank context line cannot be empty; it must be written as a line containing exactly one leading space
- When a line changes, write it explicitly as `-old_line` followed by `+new_line`
- Do not leave both the old and new versions as ordinary context lines
- Never copy `read_file` display artifacts such as `>>> path`, `N: ` line numbers, or separators
- Prefer 2-3 lines of raw current-file context around each change

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

`dry_run` only parses and resolves the patch. It does not write to disk. Prefer it when:

- you just built the first patch from a fresh `read_file` result
- the edit location is sensitive and you expect context matching risk
- a weaker model is making its first attempt with this tool

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

## Common failure causes

- wrapping the patch body in JSON or markdown again
- pasting raw file lines after `@@` without line prefixes
- leaving a blank context line truly empty
- writing both the old and new versions as space-prefixed context lines
- copying `read_file` display line numbers or the `>>> path` header
- trying to edit too many distant regions in a single patch
- making the `-old_line` and `+new_line` effectively identical, so the patch applies but changes nothing

## Typical corrective hints

For some `PATCH_CONTEXT_NOT_FOUND` cases, the tool now tries to provide a concrete corrective hint instead of only saying that the context was not found. For example:

- if the previous chunk contains only context and no real edit, the hint will call it an "empty hunk"
- if the next chunk reuses the same anchor after that empty hunk, the hint will explain that the failure is caused by a duplicate anchor / extra `@@` block
- if a context line is duplicated as a delete line, the hint will point out that the deleted line should not also appear as context
- if the patch appears to do work but the file ends up unchanged, the hint will distinguish between "context-only" and "old/new lines are the same"

When you see such hints, prefer these fixes first:

- remove empty hunks that contain no `+` or `-`
- merge one insertion or replacement back into a single hunk
- do not let two consecutive hunks reuse the same anchor lines
- for insertion-only hunks, keep both leading and trailing unchanged lines when possible instead of relying on one-sided anchoring

## When to use it and when not to

This version has relaxed some scope limits compared with the earliest implementation, so a single patch may cover more scattered regions. It is still best used for small-to-medium precise edits rather than whole-file rewrites.

Good fit:

- changing a few lines of code
- adding a small helper function
- adjusting a few independent files

Poor fit:

- rewriting a whole file
- generating a large amount of documentation or assets
- edits that require running a script before the final content is known
- large refactors that touch many distant regions

For those cases, consider:

- whole-file replacement: `write_file`
- command-first workflows: `execute_command`

## Difference from `write_file`

- `apply_patch`: preserves context and is easier to review
- `write_file`: writes the final content directly

If you already know exactly what the entire new file should be, `write_file` is more direct.  
If you only need a precise change, `apply_patch` is safer.
