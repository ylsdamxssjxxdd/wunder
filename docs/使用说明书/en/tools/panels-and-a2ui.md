---
title: Panels and a2ui
summary: The semantics and return structures of `a2ui`, `plan_update`, and `question_panel`.
read_when:
  - You need to update the frontend plan panel, open a question panel, or send structured UI instructions
source_docs:
  - src/services/tools.rs
  - src/services/tools/catalog.rs
updated_at: 2026-04-10
---

# Panels and a2ui

There are three things on this page:

- `a2ui`
- `plan_update`
- `question_panel`

`a2ui` is an exception in the return format. The other two already use the unified success envelope.

## `a2ui`

This tool sends structured UI instructions directly to the frontend, so its return shape stays very thin:

```json
{
  "uid": "surface_xxx",
  "a2ui": [
    {
      "beginRendering": {}
    }
  ],
  "content": "optional text"
}
```

Recommended message shapes:

- `beginRendering`
- `surfaceUpdate`
- `dataModelUpdate`
- `deleteSurface`

## `plan_update`

### Minimum arguments

```json
{
  "plan": [
    { "step": "Analyze the code", "status": "completed" },
    { "step": "Update the docs", "status": "in_progress" }
  ]
}
```

### Success result

```json
{
  "ok": true,
  "action": "plan_update",
  "state": "completed",
  "summary": "Updated the execution plan.",
  "data": {
    "status": "ok"
  }
}
```

## `question_panel`

### Minimum arguments

```json
{
  "question": "Choose how to proceed",
  "routes": [
    {
      "label": "Conservative fix",
      "description": "Start with the smallest safe repair",
      "recommended": true
    }
  ],
  "multiple": false
}
```

### Success result

```json
{
  "ok": true,
  "action": "question_panel",
  "state": "awaiting_input",
  "summary": "Opened a question panel and is waiting for user input.",
  "data": {
    "question": "Choose how to proceed",
    "routes": [
      {
        "label": "Conservative fix",
        "description": "Start with the smallest safe repair",
        "recommended": true
      }
    ],
    "multiple": false
  }
}
```

## Key points

- `plan_update` only updates the display. It is not a task executor.
- `question_panel` succeeding does not mean the task is done. It only means the system entered `awaiting_input`.
- `a2ui` is the lower-level option and should be used when you already know the exact frontend structure to send
