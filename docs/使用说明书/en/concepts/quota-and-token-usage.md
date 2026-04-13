---
title: "Token Account and Usage"
summary: "Wunder uses Token accounts to manage user's disposable balance, and Token usage to observe context load; these are related but not the same thing."
read_when:
  - "You are looking at Token balance, daily grants, or session resource usage"
  - "You want to understand why the system emphasizes round_usage.total_tokens and context occupancy"
source_docs:
  - "docs/API文档.md"
  - "docs/设计方案.md"
  - "docs/系统介绍.md"
---

# Token Account and Usage

In Wunder, the most easily misunderstood thing is conflating Token account, Token consumption, and vendor billing into one concept.

## Key Points

This page explains two parallel concepts:

- What is the user's Token account
- What is runtime Token usage

## What Is a Token Account

You can think of a Token account as the disposable currency balance a user has in Wunder:

- `token_balance`: Current balance, can accumulate, and will decrease with model consumption
- `token_granted_total`: Cumulative total of grants and rewards
- `token_used_total`: Cumulative total of consumption
- `daily_token_grant`: Number of Tokens to be granted to the current user each day
- `last_token_grant_date`: The date when the last daily grant was completed

The current default rules are:

- Level 1/2/3/4 users receive `100M / 50M / 10M / 1M` Tokens daily respectively
- Daily grants are directly accumulated into `token_balance`
- Model calls are deducted based on actual `total_tokens`
- When users upgrade, they receive additional Token rewards, which are also directly credited to the Token account

So the Tokens here are more like an in-system currency, not a "reset daily" quota bar.

## What Is Token Usage

Token usage is a runtime observation metric, answering:

- How much context did this current round of request actually occupy
- How many Tokens did this request actually consume

It is not the vendor billing metric, nor is it the user balance itself.

## Why Emphasize This Metric

Because many stability issues in Wunder are fundamentally related to context size:

- Whether threads are too long
- Whether compression is timely
- Whether tool results are maxing out the context
- Whether certain tasks are persistently occupying high input windows

If you only watch the billing, it's hard to guide system governance.

## Key Fields to Watch

In monitoring and session summaries, the most meaningful are:

- `round_usage.total_tokens`
- `token_usage.total_tokens`

Where:

- `round_usage.total_tokens` represents **the actual context occupancy after a single round of request completes**, currently serving as the authoritative metric for context occupancy.
- `token_usage.total_tokens` represents **the usage breakdown of a single model call**; when only one model call occurs in a round, it usually matches `round_usage.total_tokens`.

If you are doing new integrations, we recommend directly consuming these explicit aliases:

- `context_occupancy_tokens`: Current context occupancy
- `request_consumed_tokens`: Single request consumption
- `consumed_tokens`: Cumulative consumption in aggregate APIs

Fields like `context_usage` are more for process estimation, suitable for observation, not as the sole benchmark.

## Token Account and Token Usage Are Not the Same Thing

You can understand it this way:

- Token Account: Governance and settlement metric, determines how much disposable Token the user has left
- Token Usage: Runtime observation metric, tells you how heavy this round's context actually is

So a request might:

- Have enough balance, but the context is already heavy
- Have low billing, but thread governance is already strained

## Applicable Scenarios

- Some sessions show abnormally high token_usage in monitoring
- You are investigating why a user has insufficient balance
- You are working on compression, pruning, and tool result length governance
- You are explaining "why admin mode and regular user mode metrics differ"

## Implementation Suggestions

- When checking if a user can continue to use, prioritize Token account fields.
- For current context occupancy recorded by Wunder, prioritize `round_usage.total_tokens`.
- For cumulative consumption recorded by Wunder, sum up each request's `round_usage.total_tokens`.
- Do not conflate single-call usage, context occupancy, cumulative consumption, Token account, and vendor billing into one concept.

## Further Reading

- [Stream Events Reference](/docs/en/reference/stream-events/)
- [Operations Overview](/docs/en/ops/)
- [Long-term Memory](/docs/en/concepts/memory/)