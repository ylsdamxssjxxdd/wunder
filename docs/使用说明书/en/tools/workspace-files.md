---
title: Workspace Files
summary: Parameters, matching rules, and failure diagnosis for the five operations: list a directory, search content, read a range, exact replacement, and whole-file write.
updated_at: 2026-10-17
---

# Workspace Files

Five common operations in the local workspace map to five tools: list a directory with `list_files`, search content with `search_content`, read a range with `read_file`, exact replacement with `edit`, and whole-file write with `write_file`.

## Listing a directory

Give a path and it lists the directory; you can limit how many levels deep to look and how many entries to return at once. Directory entries are marked with a trailing slash, and large listings are paginated; the result tells you whether another page exists and gives the cursor for continuing.

## Searching content

It searches a directory by keyword and returns the files and positions that match. You can restrict the file type, choose case-sensitive or regular-expression matching, and include a few lines of context before and after each hit; you can also cap how many hits and how many files are scanned. When the result is truncated it says why, and to preview the search plan without running it for real you can turn on a dry run.

## Reading a file

When you know the path, read that range: give a start and end line, reading one file at a time, or pass several files and line ranges to read in a batch. The result assembles the content into directly readable text and gives a summary for each file; when the default window is not enough or the budget truncates the output, it tells you that more reading is needed.

## Exact replacement and text editing

`文本编辑` (English alias `edit`) provides literal replacement, insertion, creation, and patches:

- Default replacement requires `file_path`, a non-empty `old_string`, and `new_string`. The original must match once; set `replace_all=true` to replace every occurrence. `command=str_replace` uses the same rules.
- Preserve spaces and indentation, and omit displayed line numbers. Only LF/CRLF differences are normalized to the target file's uniform newline style.
- Deletion requires an explicit `new_string=""`; missing or incorrectly typed text fails validation. Legacy `path`, `old_str`, and `new_str` remain accepted; aliases and canonical fields must agree when both are supplied.
- Insert with `command=insert`, `file_path`, `insert_line`, and `new_string`. Insert after an existing 1-based line; 0 prepends. A terminal newline does not add a phantom line. Insertion follows the file's newline style and does not add another blank line when the input already ends in a newline.
- Create with `command=create`, `file_path`, and `file_text`; existing targets are rejected. Read with `command=view` and `file_path`; optional `view_range=[start,end]` uses `end=-1` for EOF.
- Use `input` for multi-file or multi-hunk patches, starting with `*** Begin Patch` and ending with `*** End Patch`. Paths are inside the patch, so `file_path` is unnecessary. Do not combine with `command` or replacement fields.
- Every write mode supports `dry_run=true`: validate and return bounded diff previews without writing files, creating directories, or updating workspace versions.

When no match exists, read the current text and revise the arguments. Multiple matches report the count and up to 8 line numbers; add context or explicitly replace all. Repeating identical arguments does not resolve these errors. Identical content returns `unchanged` without rewriting the file.

Use [Execute Command](/docs/en/tools/exec/) for scripts requiring regular expressions or conditional logic, and `write_file` for whole-file generation or overwrites.

## Whole-file write

`write_file` is for creating or overwriting a whole file. If the workspace has a matching language service, a diagnostic runs alongside the write. It is not meant for small, precise code edits (use the patch form) or for running scripts or builds (use [Execute Command](/docs/en/tools/exec/)).

## Errors

When any of these tools fails it puts the reason in the result; look mainly at the error code, the hint, and the attached data, which usually shows directly whether the path is missing, the arguments are wrong, or the file cannot be read.
