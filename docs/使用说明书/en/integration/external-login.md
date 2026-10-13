---
title: External Login and Embedding
summary: Bring users who are already logged in to an external system straight into wunder without asking them to type a password again; they can land on the chat page or on a specific agent.
updated_at: 2026-10-13
---

When your users are already logged in elsewhere and you want them to enter wunder without retyping a password, use this group of interfaces:

- `/wunder/auth/external/*`

This is a separate matter from logging in to your own admin backend.

## Applicability

- Embedding wunder into an external system, or a passwordless redirect → use these.
- Aligning an external identity with wunder, or issuing a login state of wunder's own → use these.
- Ordinary admin login or username-and-password login → not these.

Different external systems integrate differently: some are suited to first requesting a one-time credential and then exchanging it for a login state, while others are suited to a direct redirect. wunder therefore keeps several entries.

## Exchanging an External Identity for a wunder Session

The most common approach is exchanging a token for a login state:

1. The external system brings a token and a user identity, and may also carry a target agent name.
2. wunder exchanges it for its own access token and also returns the bound agent ID.
3. If the name matches an existing agent the current user can access, it additionally enters focus mode.
4. The frontend always enters the embed page: by default the message page, or the focus page when an agent matches, with the left sidebar hidden.

A shorter entry brings the user identity in directly, without generating a token in the external system first.

The entries available on this path:

- `POST /wunder/auth/external/login`
- `POST /wunder/auth/external/code`
- `POST /wunder/auth/external/launch`
- `POST /wunder/auth/external/token_launch`
- `POST /wunder/auth/external/token_login`
- `POST /wunder/auth/external/exchange`

## Where Users Land

External links all land inside the embed page: the default message page carries at least the "messages" section, the default agent additionally carries the "default" entry, and focus mode carries the matched agent ID. Embedding the agent page is also possible, and likewise hides the left sidebar.

wunder does more than hand the token back; based on whether a named agent matched, it decides which page the user lands on. An embedded user equally has only one agent instance in its own session.

## Key Settings and Common Pitfalls

- This path has its own key; when it is not configured separately, it falls back to the platform's general key, so it is not open by default.
- Common pitfalls: configuring only the external token but forgetting the key fallback; getting only the token and not handling the returned agent ID; wanting to change the current thread's prompt when an external link only affects new threads; treating the passwordless entry as a public interface.

## Further Reading

- [wunder API](/docs/en/integration/wunder-api/)
- [User World Interface](/docs/en/integration/user-world/)
- [Authentication and Security](/docs/en/ops/auth-and-security/)
