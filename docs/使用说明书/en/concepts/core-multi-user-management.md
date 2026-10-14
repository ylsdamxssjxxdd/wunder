---
title: Multi-user Management
summary: Users, units, and permission boundaries in the Hull (server) form — login state, presence, the unit tree, levels and credits, agent definition documents, and data isolation.
read_when:
  - You are setting up accounts, units, or roles for a Hull deployment
  - A token, a refresh, or a session on another device behaves unexpectedly
  - You need to know what Credits measure, and how a worker card differs from a thread
source_docs:
  - crates/wunder-runtime/src/services/user_store.rs
  - crates/wunder-runtime/src/api/auth.rs
  - crates/wunder-core/src/auth.rs
  - crates/wunder-runtime/src/services/presence/mod.rs
  - crates/wunder-runtime/src/services/user_leveling.rs
  - crates/wunder-runtime/src/services/worker_card_files.rs
updated_at: 2026-10-13
---

Multi-user governance is a foundational capability of the Hull (server) form; the Pupa (desktop) and the Helm (cli) are local, single-machine forms with no account system. Concrete panel operations are covered in the reference section.

## The three gates of identity

- **Management gate**: management paths (such as `/wunder/admin/*`) require an API key or a Bearer token on the server side, and the account must be an administrator; a unit head is separately allowed on designated paths.
- **User ownership**: a user-facing request is first resolved to a user by Bearer, then its ownership is checked, ensuring it can touch only its own sessions, tools, and tasks.
- **Realtime channels**: a WebSocket connection carries a token in its handshake, and each of the six realtime channels authenticates on its own.

## Login state uses opaque tokens

| Credential | Shape | Default lifetime |
| --- | --- | --- |
| Access token | A random string starting with `wund_`, verified by looking it up in the database | 7 days |
| Refresh token | A random string starting with `wundr_` | 30 days |
| External-login one-time code | Starting with `wundc_`, held in memory | Short-lived |

A few key semantics:

- **These are not JWTs.** A token carries no claims and cannot be parsed offline; validation is a database lookup, so revocation takes effect immediately.
- **Refresh is an OAuth-style family rotation**: one refresh issues a new access plus a new refresh, both in the same rotation family; presenting an already-revoked token to refresh again is judged to be a stolen token, and the whole family (including the access token in use) is revoked at once.
- **The session scope decides who can renew**: login reads the `x-wunder-session-scope` header, and only local scopes such as the Pupa and the Helm get a refresh chain that can renew; browser sessions do not take this path.
- **Logging in again from the same scope evicts the old session**: when the same user logs in again from the same scope, the old session receives a notice that it has been superseded and is disconnected. When troubleshooting a connection that dropped on another client, first check whether it was the same scope logging in again.
- Passwords are stored as Argon2 hashes; plaintext never reaches the database.

## Presence is split by form

Presence is recorded separately by client type, with the Beehive (web), the Pupa (desktop), and the Helm (cli) as three independent lines; roughly 90 seconds without a touch counts as offline, with a sweep interval of 30 seconds (minimum 10 seconds). As a result, presence in the user list is split into web, desktop, and cli columns — "online" is no longer a yes-or-no value, and to see who is online on which form you look at the matching column. The management exit is at `/wunder/admin/gateway/presence`.

## The unit tree and the unit-head scope

At startup, if the unit table is empty, it is automatically seeded from a unit template. The unit tree is at most 4 levels deep.

The unit-head role is allowed only on the two paths `/wunder/admin/org_units` and `/wunder/admin/user_accounts`; its data scope equals the units it heads plus their descendant units, and stepping outside that scope is a permission denial. The frontend's check is only a convenience; the unit-head boundary is ultimately enforced by the server.

## Levels and credits are two different things

**Levels** lean toward growth display: XP equals the integer number of seconds the session has run, the level cap is 200 (`MAX_USER_LEVEL`), and the XP needed to reach the next level increases as `240 + 28L + 0.9L²`. Levels unlock no resource; they only reflect time spent.

**Credits** lean toward governance: every user holds an integer Credits account with a balance, cumulative issuance, cumulative consumption, and a daily grant (1000 per day by default). It is an abstract allowance of issuance and deduction, **not a token count** — each request sent to the model service deducts exactly 1 Credit, independent of token metering. When the balance is short, a new request is rejected by quota and returns a snapshot of the remaining allowance; administrator accounts are exempt from deduction but still record request counts.

## An agent's definition document is not the same thing as a thread

| Concept | What it is |
| --- | --- |
| worker card (`.worker-card.json`) | An agent's **portable definition document**, holding basic information (identifier, name, description, icon), prompts, capabilities, interaction, runtime settings, and preset publication status |
| Thread / session | The continuous unit of execution once an agent is invoked; one agent can hold several independent task threads at once |

After a preset agent is published, you can control which units can see it and decide which fields a bound user may change. Note that the disk directory for preset cards (`user_agents.worker_cards_root`) is an **empty string by default** — without explicit configuration there is no preset card loaded from disk, so this capability does not ship out of the box. Approval mode has three values, `suggest`, `auto_edit`, and `full_auto`, with `full_auto` the default. Sessions bind to an agent by agent id, and their task threads stay independent of one another.

## Where data isolation lives

Sessions, tasks, workspaces, tools, and channel bindings are all isolated by user ownership, and the user-facing side checks ownership handler by handler. Cloud files and execution boundaries are carried by a sandbox: read-write and execute operations run in each user's own container, mounting only that user's own directory, and path checking is no longer responsible for tenant isolation. Quotas, levels, and presence are account-level; model configuration and preset agents are unit-level or global-level.

## A few clarifications

- A token is not a self-contained JWT and cannot be parsed offline — it is an opaque random string, verified by a database lookup, and revocation takes effect immediately.
- A lost refresh cannot be replaced by resending the old one; replaying an old refresh triggers whole-family revocation, and the correct move is to log in again.
- Credits are not a token budget; they are an integer request allowance, one deduction per model request, with token usage metered separately.
- A unit head cannot manage every admin page; it is allowed only on the unit and user-account paths, and only within descendant units.
- A worker card is not a task thread; it is an agent's definition document, while a thread is a unit of execution — two different layers.
- The system does not ship a set of preset agent cards; the preset card directory is empty by default, and nothing exists until it is configured.

## Related pages

- [Bridge Panel Guide](/docs/en/reference/admin-panels/) — user management and unit management operations
- [Quota and Token Usage](/docs/en/concepts/quota-and-token-usage/) — metering details
- [Interlink](/docs/en/reference/interlink/) — how local devices and external callers join the Hull
