---
title: External Login and Seamless Embedding
summary: Wunder reserves `/wunder/auth/external/*` as the interface for external system embedding and passwordless access.
read_when:
  - You want to enter Wunder directly from an external system
  - You want to understand the purposes of external login, launch, and token_login
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - config/wunder-example.yaml
  - src/api/auth.rs
---

# External Login and Seamless Embedding

Wunder currently reserves a complete set of external access interfaces:

- `/wunder/auth/external/*`

These are not for regular administrator login, but for:

- External system embedding
- Passwordless redirection
- User identity alignment
- Issuing Wunder's own login session

## Why Not Reuse Regular Login

External system integration typically has these characteristics:

- Users are already logged in to the external system
- Wunder only handles session handover, not suitable for asking users to enter passwords again
- After login, should redirect directly to a specific chat or embedded page

Therefore, Wunder specifically reserves the external access interface.

## Common Interfaces

Currently, the codebase has at least these entry points:

- `POST /wunder/auth/external/login`
- `POST /wunder/auth/external/code`
- `POST /wunder/auth/external/launch`
- `POST /wunder/auth/external/token_launch`
- `POST /wunder/auth/external/token_login`
- `POST /wunder/auth/external/exchange`

For the most common scenarios, handle as follows:

- `token_login`

## What `token_login` Is Suitable For

The most typical usage currently:

- External system provides `token + user_id`, optionally with `agent_name`
- Wunder directly exchanges for its own `access_token`
- Also returns `agent_id`
- When `agent_name` matches an existing agent accessible to the current user, additionally returns `focus_mode=true`
- Frontend enters regular chat page or focused embed page based on returned result

In other words, it's more like a bridging interface for "exchanging external identity for Wunder session".

## Why launch / code Still Exist

Different external systems have different integration approaches.

Some systems are suitable for:

- First request a one-time code
- Then exchange for login session

Some systems are suitable for:

- Direct launch
- Direct redirect to target page

Wunder retains these entry points to be compatible with different embedding methods, rather than requiring all integrators to follow a fixed process.

## What Ensures Security Boundaries

The key configuration for this chain is:

- `security.external_auth_key`

If not explicitly configured, it will automatically fall back to:

- `security.api_key`

So it's not "open by default".

## Where to Redirect After Integration

Currently, the most typical destinations fall into two categories:

- No `agent_name` passed, or name doesn't match existing agent: `/app/chat?section=messages&entry=default`
- Matches existing agent and enters focus mode: `/app/embed/chat?section=messages&agent_id=<agent_id>`
- Desktop equivalents: `/desktop/chat?section=messages&entry=default` and `/desktop/embed/chat?section=messages&agent_id=<agent_id>`

In other words, regular chat pages will at least have `section=messages`; default agents will additionally have `entry=default`, and focus mode will have the matched `agent_id`.
Wunder doesn't just return a token, but also decides the final chat shell based on whether a specified agent is matched.

## What Scenarios This Chain Is Suitable For

Suitable for:

- Unified portal embedding Wunder
- External system single sign-on into a specific agent
- Team system bringing user identity into Wunder

Not suitable for:

- Replacing administrator backend login
- Replacing regular user account/password system

## Common Pitfalls

- Only configured external JWT, but didn't configure external_auth_key fallback
- Only got the token, didn't handle the returned `agent_id`
- Wanted to change current thread prompt, but forgot external link only affects new threads
- Treating external passwordless login as a regular open interface

## Further Reading

- [wunder API](/docs/en/integration/wunder-api/)
- [User World Interface](/docs/en/integration/user-world/)
- [Authentication and Security](/docs/en/ops/auth-and-security/)