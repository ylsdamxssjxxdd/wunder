---
title: Final Response
summary: The sole job of `final_response` is to end the current turn with the reply for the user.
read_when:
  - You have finished tool calls and reasoning and are ready to answer the user
source_docs:
  - src/services/tools/dispatch.rs
  - src/services/tools/catalog.rs
updated_at: 2026-04-10
---

# Final Response

`final_response` is not a normal business tool. It is only a termination signal.

## Input

```json
{
  "content": "This is the final reply"
}
```

## Return

```json
{
  "answer": "This is the final reply"
}
```

## Key points

- It does not use the unified success envelope
- It does not include `ok`, `action`, `state`, `summary`, or `data`
- It should only be called when the task is genuinely ready to close

If the system still needs to wait for a child task, keep polling, or request more input from the user, this is not the right tool yet.
