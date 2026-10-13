---
title: ptc
summary: ptc was the temporary Python script execution tool; it has been removed from wunder, and this page is a historical reference.
updated_at: 2026-10-13
---

> **Removed**: ptc has been removed from wunder and is not visible to the agent or users; this page is a historical reference. To run commands now, use [Execute Command](/docs/en/tools/exec/).

ptc stood for programmatic tool call. It wrote a complete Python script into a temporary file, ran it under a given directory, and handed back `stdout`, `stderr`, and the exit code.

It was not a drop-in replacement for running a shell: plain command-line work always preferred [Execute Command](/docs/en/tools/exec/).

## How it worked

A call provided two things: the script filename and the script content.

```json
{
  "filename": "helper.py",
  "content": "print('hello')"
}
```

Each call stored its script separately under `ptc_temp/<invocation_id>/`, so scripts with the same filename could not overwrite each other when a workspace was shared. The returned `path` pointed at that script, and the `workdir` you gave remained the directory the program actually ran in.

It returned the script path, the run directory, the exit code, `stdout`, and `stderr`; even on failure these were usually kept.

## What it was good for

- Temporary data processing.
- Short Python parsing tasks.
- A few steps of Python logic that were not worth turning into a separate project file.

## What it was not good for

- Shell workflows chaining several commands.
- Script projects that needed long-term maintenance.
- Complex system build pipelines.

Those were better handled by [Execute Command](/docs/en/tools/exec/).
