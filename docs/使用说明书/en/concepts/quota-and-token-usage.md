---
title: Quota and Token Usage
summary: Each model request costs 1 credit and 1000 are granted daily; token usage is observed only and never deducted.
updated_at: 2026-10-13
---

Quota limits how many model requests you can make. Regular users receive **1000 credits** per day, the same regardless of organization or level.

- Unused credits accumulate. The grant is applied once on the day's first use or account adjustment; days when nothing was used are **not** backfilled.
- Each real model request costs **1 credit**, including the next model call inside a tool loop, compaction summaries, and failed retries.
- Streaming chunks, standalone tool execution, and history replay cost **no** credits.
- A request consumes a credit even if it fails; when the balance is insufficient, no new model request is dispatched.
- Administrators and unregistered virtual users are not limited by quota.

## Viewing and adjusting

On the user side, "My Profile" shows the current balance and the daily grant. Administrators can set the balance directly in "User management → Settings", or grant and deduct credits:

- A value set directly is the final balance after the day's grant is carried over.
- A grant increases "cumulative granted", while a deduction counts toward "cumulative used"; an operation is rejected when the balance is insufficient.

The account shows several numbers: available credits, cumulative granted, cumulative used and administrative deductions, the daily grant, and the most recent grant date.

Multiple sessions share one user credit account, and concurrent requests cannot overdraw it. User levels still accumulate experience but no longer award credits. On the first upgrade, an old token-balance account is initialized to 1000 credits and is **not** converted from its historical token count; a restart afterward does not reset the balance either.

## Tokens are for observation only

Token usage continues to describe a model's input, output, reasoning, and context size, and it **never affects the credit deduction**. A short request and a long request both cost 1 credit.

The token metrics you can see include:

- Context occupancy: the context size most recently observed.
- The cumulative model tokens consumed within one user request.
- Model usage events, plus the request count, whether it was charged, the credit consumed in that turn, and the account snapshot for each real model request.

In a session, "model request count" and "credit consumption" are two different metrics: for a regular user, one billable model request normally increases each by one; in an exempt case such as an administrator, the request count still increases but credit consumption stays at zero. The account snapshot only shows the balance at that moment and does not represent how much this request deducted. Old credit records are kept only for reading history.

[Multi-user management](/docs/en/concepts/core-multi-user-management/) · [Admin panels guide](/docs/en/capabilities/admin-panels/)
