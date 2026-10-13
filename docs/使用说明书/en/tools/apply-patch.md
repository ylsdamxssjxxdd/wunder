---
title: Apply Patch
summary: Use patch text for small, reviewable, precise edits; `apply_patch` has been merged into `Text Edit`.
updated_at: 2026-10-13
---

> **Merged**: `Apply Patch` (`apply_patch`) has been merged into `Text Edit` (`edit_file`) and is no longer exposed as a standalone tool; only the legacy invocation name is still recognized.

The patch form suits precise edits with few files, few changes, and clear context, while keeping context so changes stay easy to review. Whole-file writes go to `write_file`; command execution goes to `execute_command`.

## Writing a patch

Minimum example:

```text
*** Begin Patch
*** Update File: src/main.rs
@@
-fn old() {}
+fn new() {}
*** End Patch
```

On the model side there are only two common arguments: `input` and `dry_run`.

## Recommended flow

1. Read the target file or the exact excerpt first
2. Build only one very small patch at a time
3. When unsure whether the context is stable, add `dry_run` to preview first
4. Once the preview passes, drop `dry_run` and submit for real
5. If the edit spans many scattered regions or is close to a whole-file rewrite, switch to `write_file` or a script

## Hard rules for Update File

- After `@@`, every line must start with a prefix: a space, `-`, or `+`
- A blank context line cannot be left empty; write it as a line with a single leading space
- To change a line, write `-old line` followed by `+new line`
- Do not write both the old and the new line as ordinary context lines
- Do not copy the `>>> path` header, line numbers, or separators from read results
- Around each change, keep 2-3 lines of the original context where possible

The tool tolerates a few common small deviations (for example a missing `*** Begin Patch`, a unified-diff header like `@@ -1,3 +1,3 @@`, a stray `@@` inside `Add File`, or missing `+` prefixes in `Add File`). That is only to raise first-call success; the smallest standard form is still preferred.

## Adding a file

```text
*** Begin Patch
*** Add File: notes.txt
+first line
+second line
*** End Patch
```

## Preview with `dry_run`

`dry_run` shares the same staging, target-conflict checks, and context matching as a real run, but discards the staged result without writing to disk. Previewing first is safest when you are unsure of the format, when the edit location is sensitive, or when you are using the tool for the first time.

## Change previews

The result carries a change preview: at most 80 lines per file, and at most 320 lines and 24 KiB across a single call. Lines beyond the remaining budget are omitted whole, while the visible lines keep their exact content and original line numbers. The same limit applies to a preview run, affects display only, never shrinks the actual edit, and is not a backup of deleted content.

The model receives only the file summary and exact added/removed line counts, not the preview body.

## Diagnosing failures

Patch failures usually fall into a few kinds: an empty patch, oversized input, too many file operations or hunks, a task failure during execution, and parsing failures, path escapes, target conflicts, or context mismatches. What the user sees is a single failure message plus a next-step hint.

Common failure causes:

- The patch body is wrapped in another layer of JSON or a Markdown code fence
- Raw file content is pasted after `@@` without a prefix on every line
- A blank context line is written as a truly empty line
- Both the old and the new line are written as space-prefixed context lines
- Line numbers or the `>>> path` header were copied from a read result
- Too many scattered regions are changed in a single patch, beyond its scope
- The `-old line` and `+new line` are effectively identical, so the patch succeeds yet changes nothing

## More specific corrective hints

When context matching fails, the tool tries to say which kind of problem it is, not just that the context was not found:

- a hunk that has only context and no real change → an empty hunk
- a later hunk reusing the same anchor set as an earlier empty hunk → a duplicate anchor, one `@@` block too many
- a line being deleted and also kept as a context line
- a patch that looks like it changes something but leaves the file unchanged → distinguish "context only" from "old and new lines are the same"

When you see these hints: delete empty hunks that have no `+` / `-`; merge an insertion or replacement at the same spot back into one hunk; do not let two consecutive hunks reuse the same anchors; and for insertion-only hunks keep both the leading and trailing original lines where possible.

## Where it fits

It suits small-to-medium precise editing: changing a few lines of code, adding a small function, adjusting a few independent files.

It does not suit whole-file rewrites, bulk generation of documents or assets, edits that need a script to run before the result is known, or large refactors across many scattered regions. For whole-file rewrites use `write_file`; to run a command first, use `execute_command`.

## Difference from `write_file`

- `apply_patch`: keeps context, easier to review
- `write_file`: overwrites with the final content directly

When the whole file's final content is known, `write_file` is more direct; the patch form targets precise changes.
