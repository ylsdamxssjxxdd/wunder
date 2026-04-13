---
title: FAQ
summary: Quick answers to high-frequency questions about Wunder, suitable for initial assessment before diving into detailed troubleshooting.
read_when:
  - You have common usage questions
  - You want to quickly determine if it's a failure
source_docs:
  - docs/API文档.md
  - frontend/src/views/LoginView.vue
  - frontend/src/views/MessengerView.vue
updated_at: 2026-04-10
---

# FAQ

## Does the `user_id` in `/wunder` have to be a registered user?

No. `user_id` is an isolation and attribution identifier; it can be a virtual user identifier passed from the business side.

## When building a chat product, should I prioritize `/wunder` or `/wunder/chat/*`?

Prioritize `/wunder/chat/*` combined with `/wunder/chat/ws`. `/wunder` is better suited for capability-call-style integration.

## How do I choose between SSE and WebSocket?

For real-time chat, prioritize WebSocket; use SSE as a fallback.

## Why doesn't the token count match the bill?

Look at it in two layers:

- `round_usage.total_tokens` is the actual context usage after a single request completes
- Actual total consumption is the cumulative sum of each request's `round_usage.total_tokens`

## What's needed to reset password on the login page?

Only:

- Username
- Email
- New password

## Where to change username or password after logging in?

Go to "My Profile -> Edit Profile".

Here you can:

- Change username
- Change email
- Change login password

## Why is the "New Thread" button sometimes grayed out?

Because the current agent is still running.

The frontend disables `New Thread` during execution to prevent main thread state confusion. Wait for it to complete, or stop the current session first before creating a new one.

## How do I choose between Swarm and Sub-Agent?

- To call existing agents for collaboration: use Swarm
- To temporarily spawn a child execution from the current session: use Sub-Agent control

## Can `temp_dir` be used as a long-term storage directory?

Not recommended. `temp_dir` is a temporary directory; long-term business data should go in the database or workspace persistent directory.

## Why can't I see a tool in a particular session?

Usually related to tool mounting policy, runtime capabilities, session-level parameters, or MCP/A2A enablement status.

## Does Desktop mode require Server deployment first?

No. Desktop can run locally and independently; deploy Server when you need multi-tenant governance and unified access.

## What's the difference between `web_fetch` and browser tools?

`web_fetch` is for content extraction; browser tools are for real page interaction.

## Which page should I look at first when problems occur?

Start with [Troubleshooting](/docs/en/help/troubleshooting/).