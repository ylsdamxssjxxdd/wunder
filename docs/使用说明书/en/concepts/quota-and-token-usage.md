---
title: Token Balance and Usage
summary: Wunder uses a token balance as the user's spendable currency, and token usage as the runtime view of context load.
read_when:
  - You need to understand token balance, daily grants, or runtime token usage
source_docs:
  - docs/使用说明书/zh-CN/concepts/quota-and-token-usage.md
updated_at: 2026-04-10
---

# Token Balance and Usage

Wunder separates two concepts that are often mixed together:

- `token_balance`: the user's spendable token currency balance
- token usage: the runtime measure of context occupancy and request consumption

## Token balance

The token account is the governance and settlement layer for a user:

- Level 1 / 2 / 3 / 4 users receive `100M / 50M / 10M / 1M` tokens per day
- Daily grants accumulate into `token_balance`
- Real model usage deducts the actual `total_tokens`
- Level-up rewards are also credited into the same token account

## Token usage

Runtime token usage is mainly for observation:

- `round_usage.total_tokens`: authoritative view of actual context occupancy after a request
- `request_consumed_tokens`: explicit alias for per-request consumption
- `consumed_tokens`: aggregated total consumption in summary APIs

Do not treat runtime token usage, account balance, and provider billing as the same thing.

For the full Chinese explanation, see:

- [Token Balance and Usage (Simplified Chinese)](/docs/zh-CN/concepts/quota-and-token-usage/)
