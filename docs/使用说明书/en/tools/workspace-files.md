---
title: Workspace Files
summary: Browsing the workspace, searching content, reading files, and writing files.
updated_at: 2026-10-13
---

The most common operations in the local workspace are these: listing a directory, searching content, reading a file, editing a file, and writing a file.

## Choosing between the tools

- To see the directory structure first: `list_files`.
- When you already know the word you are looking for and want to locate it: `search_content`.
- When you know the path and want to read a range: `read_file`.
- To create or overwrite a whole file: `write_file`.
- To make one exact text replacement: `edit_file2`.

## Listing a directory

Give a path and it lists the directory; you can limit how many levels deep to look and how many entries to return at once. Directory entries are marked with a trailing slash, and large listings are paginated; the result tells you whether another page exists and gives the cursor for continuing.

## Searching content

It searches a directory by keyword and returns the files and positions that match. You can restrict the file type, choose case-sensitive or regular-expression matching, and include a few lines of context before and after each hit; you can also cap how many hits and how many files are scanned. When the result is truncated it says why, and to preview the search plan without running it for real you can turn on a dry run.

## Reading a file

When you know the path, read that range: give a start and end line, reading one file at a time, or pass several files and line ranges to read in a batch. The result assembles the content into directly readable text and gives a summary for each file; when the default window is not enough or the budget truncates the output, it tells you that more reading is needed.

## Editing a file

`edit_file2` does one exact replacement, swapping one stretch of original text for new text:

- The original text must match the file's current content exactly.
- To delete text, set the new text to an empty string.
- When replacing several places at once, you can make it require the match count to be exact, and it errors when they do not line up.
- If the original text accidentally includes a file header or line numbers copied from reading the file, it tries to strip them automatically when the match fails.
- If the file uses CRLF line endings, it retries the match using the target file's line-ending style.
- If the replacement has already been made uniquely, it reports that it was already applied.

It suits an edit where you already have the exact original text, it is a single replacement, and you do not need regular expressions or multi-step logic. For complex replacements, regular-expression replacements, edits spanning several stretches, or edits with conditional logic, write a script with [Execute Command](/docs/en/tools/exec/); to generate or overwrite a whole file, use `write_file`; and for a small code patch reviewed with its surrounding context, use [Apply Patch](/docs/en/tools/apply-patch/).

## Writing a file

`write_file` is for creating or overwriting a whole file. If the workspace has a matching language service, a diagnostic runs alongside the write. It is not meant for small, precise code edits (use the patch form) or for running scripts or builds (use [Execute Command](/docs/en/tools/exec/)).

## When something goes wrong

When any of these tools fails it puts the reason in the result; look mainly at the error code, the hint, and the attached data, which usually shows directly whether the path is missing, the arguments are wrong, or the file cannot be read.
