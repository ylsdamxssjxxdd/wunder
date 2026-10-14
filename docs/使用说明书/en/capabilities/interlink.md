---
title: Interlink
summary: The Beehive, the Pupa, and the Helm share one presence state, see each other's workspaces, and control each other in both directions.
updated_at: 2026-10-13
---

Interlink connects the Beehive (web), the Pupa (desktop), and the Helm (cli) into one system: **unified presence + cross-end workspace visibility + two-way remote control**. Local devices open no public inbound entry point; all traffic is relayed through the Hull.

## The three endpoints and prerequisites

| Name | Form | Role |
|------|------|------|
| Beehive | web | The cloud AI workbench, provided with the Hull, opened in a browser |
| Pupa | desktop | The local AI workbench, the desktop form, with direct access to files on this machine |
| Helm | cli | The manual local entry point, the command-line form |

Prerequisites:

1. All three endpoints share the `wunder-runtime` engine and the `wunder-core` contract.
2. The local device has signed in to the Hull through the existing cloud connection scheme, and holds `wund_<token>` + `device_id`.
3. The Hull is the sole authority for accounts, devices, quota, and policy.

## The four capabilities

| ID | Capability | What it means |
|------|------|------|
| R1 | Unified presence | The Hull maintains a cross-end node directory, and the three endpoints share one state model — `online` / `away` / `busy` / `reconnecting` / `offline` — at second-level precision (local) and near-real-time precision (web) |
| R2 | Workspace visibility | The local workspace appears in the Beehive as a **shadow projection**, projecting metadata summaries (thread directory, directory-tree summary, scheduled tasks, usage); **file contents are not mirrored**, and are pulled on demand over the interlink tunnel as the user acts |
| R3 | Cloud controlling local | The Beehive can issue graded operations to online local nodes (query / session control / workspace write / tool execution); sensitive operations go through local **human-in-the-loop approval**, and everything is audited |
| R4 | Local controlling cloud | The Pupa and the Helm treat the cloud as a symmetric node — browse the cloud workspace, send messages to cloud threads, read thread event streams; the Helm provides a scriptable command group |

The unified state model:

| State | Meaning |
|------|------|
| `online` | Online and idle |
| `busy` | Online and executing |
| `away` | Online but inactive |
| `reconnecting` | Recovering from a dropped connection |
| `offline` | Offline |

## How to use it

### Pupa (desktop)

| Entry point | What it does |
|------|------|
| Settings → the device interlink category | Tunnel state, device table, cloud workspace browsing |
| Remote command approval dialog | Polls the pending queue every 2s; approve / reject |
| Sidebar thread source badge | Cloud / device |
| Tunnel client | Hangs up as soon as the process starts |

### Beehive (web)

| Entry point | What it does |
|------|------|
| Sidebar | Presented as "device as workspace": a device appears as a sidebar workspace row, and the directory falls into place as threads are clicked |
| The "cloud / device" dropdown switcher | Removed |

### Helm (cli)

| Entry point | What it does |
|------|------|
| The `cloud` subcommand group | `devices` / `ws` / `send` / `watch` / `audit` / `approve` |
| `cloud status` | Combines four sections: interlink tunnel, pending approvals, other devices, cloud workspace usage |
| `cloud watch` | Frame types are `snapshot` / `delta` / `command` / `error` / `close` |

## Security boundary

| Boundary | Meaning |
|------|------|
| No public inbound entry point | Reverse tunnel: the local side only makes outbound connections, opens no listening port, and control traffic is delivered over the already-established link |
| Least capability | Each class of remote operation is a declarative capability, read-only by default |
| Explicit authorization | Write and execution classes require local user authorization, and an administrator can tighten it further in the Bridge |
| Graded approval | Risk tiers L0–L3, see the table below |
| Complete audit | Every remote operation forms a chain that can be inspected |
| One-click shutdown | The kill switch is held by the user |

Risk tiers:

| Level | Operations covered | Approval |
|------|------|------|
| L0 read-only | `node.summary` / `shadow.refresh` / `workspace.list\|search\|stat\|read` / `threads.list\|get` | No approval needed |
| L0+ | `workspace.read:binary` | No approval needed + audited |
| L1 session | `thread.create` / `thread.message` / `thread.cancel` / `thread.answer` | Approval needed by default, can be lowered to none |
| L2 workspace write | `workspace.write\|mkdir\|move\|copy\|delete` | Approval mandatory, cannot be lowered |
| L3 execution | `tool.exec` (allow-listed commands) / `agent.spawn` | Approval mandatory + disabled for administrators by default, can be enabled |

The two worth remembering most:

- **L3 is off by default**: `tool.exec` and `agent.spawn` are not in a device's capability set by default; approvals are never remembered; only a summary of the execution result is returned.
- **L2 writes are backed up first**: write operations are backed up to `工作区/.interlink_backup/<command_id>/` (the most recent 50 generations are kept), and the result reports the SHA256 before and after.

## Non-goals

- Local and cloud threads are **not merged**, and are not visible to each other. Interlink is remote operation, not data synchronization; a remote thread always exists on the other side marked as "remote".
- No full mirroring or two-way synchronization of workspace files — only metadata projection plus on-demand pull.
- No graphical remote control; control granularity stops at the engine and file APIs.
- No NAT hole-punching; all traffic is relayed through the Hull.
- The Pupa and the Helm do not interlink directly; local-to-local access always goes through the Hull.

## Cost

| Metric | Measured |
|------|------|
| Idle tunnel CPU | 0.114% of a single core |
| Idle window memory | Working set growth 0.18 MB (one-time +4.7 MB at link setup, counted separately) |

The numbers come from the two-phase same-process sampling of `scripts/interlink-bench/measure-idle.ps1` (one baseline phase and one tunnel phase, measuring the delta).
