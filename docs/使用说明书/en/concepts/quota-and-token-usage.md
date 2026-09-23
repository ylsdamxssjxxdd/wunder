---
title: Quota and Token Usage
summary: Each model request costs 1 credit; regular users receive 1000 credits daily. Token usage remains diagnostic.
---

# Quota and Token Usage

Regular users receive **1000 credits daily**, regardless of organization or user level. Unspent credits accumulate. The daily grant is applied once on the first account operation that day; inactive days are not backfilled.

Each actual model request costs **1 credit**, including tool-loop continuations, compaction requests, and retries. Stream chunks, standalone tool execution, and history replay cost no additional credits. Failed dispatched requests still consume a credit. Insufficient balance prevents dispatch. Administrators and unregistered virtual users retain their existing exemptions.

## View and manage credits

My Profile shows available credits and the daily grant. Administrators can set the final balance or grant/deduct credits in User Management → Settings. An exact set is the final balance after settling today's grant. Grants increase cumulative grants; deductions increase cumulative usage. Overdrafts are rejected atomically.

| Field | Meaning |
| --- | --- |
| `quota_balance` | Available credits |
| `quota_granted_total` | Cumulative grants |
| `quota_used_total` | Cumulative usage and administrative deductions |
| `daily_quota_grant` | Daily grant |
| `last_quota_grant_date` | Last grant date |

Concurrent threads share the user account and cannot overspend. Levels still track experience but no longer award credits. On first upgrade, existing token-balance accounts start with 1000 credits; historical tokens are not converted. Restarting does not reset the balance.

## Token observations

Input, output, reasoning and context tokens remain available for runtime diagnostics. They do not determine the credit charge. Short and long model requests both cost 1 credit.

- `context_occupancy_tokens`: latest observed context occupancy.
- `request_consumed_tokens` / `round_usage.total_tokens`: cumulative model tokens for a user request.
- `token_usage` / `model_usage`: model usage events.
- `quota_usage`: credit account snapshot.

[User management](/docs/en/concepts/core-multi-user-management/) · [Admin panels](/docs/en/reference/admin-panels/)
