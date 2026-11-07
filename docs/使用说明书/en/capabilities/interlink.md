---
title: Interlink
summary: Device management, workspace access, and thread control after a local workbench signs in to the cloud.
updated_at: 2026-10-17
---

Desktop remains a local AI workbench that works offline. After it signs in to the Server, it can use cloud agents and threads without opening a local port. If the user explicitly enables local sharing, Cloud can access the device's authorized workspaces and threads through the Server. CLI exposes the same operations for manual and scripted use.

## Sign-in and sharing

The first Desktop or CLI sign-in creates an independent device session. It does not automatically expose local files or threads. The user can select off, summary sharing, or full sharing in account settings, and Admin policy can further restrict every device.

“Signed in”, “tunnel online”, and “this operation is authorized” are separate states. Sign-in permits cloud access; an online tunnel is required to operate a local device; mutations and execution also require capability, policy, and approval checks.

## Prerequisites

1. Desktop or CLI is signed in to the Server.
2. To access a local device, that device is online and its owner enabled sharing.
3. The Server is the authority for accounts, devices, quota, and policy.

## Capabilities

| ID | Capability | What it means |
|------|------|------|
| R1 | Device state | The Server distinguishes a valid sign-in, a live tunnel, and effective capabilities; it presents `online` / `away` / `busy` / `reconnecting` / `offline` |
| R2 | Workspace visibility | The local workspace appears in the Cloud as a **shadow projection**, projecting metadata summaries (thread directory, directory-tree summary, scheduled tasks, usage); **file contents are not mirrored**, and are pulled on demand over the interlink tunnel as the user acts |
| R3 | Cloud controlling local | The Cloud can issue graded operations to online local nodes (query / session control / workspace write / tool execution); sensitive operations go through local **human-in-the-loop approval**, and everything is audited |
| R4 | Local controlling cloud | The Desktop and the CLI treat the cloud as a symmetric node — browse the cloud workspace, send messages to cloud threads, read thread event streams; the CLI provides a scriptable command group |

The unified state model:

| State | Meaning |
|------|------|
| `online` | Online and idle |
| `busy` | Online and executing |
| `away` | Online but inactive |
| `reconnecting` | Recovering from a dropped connection |
| `offline` | Offline |

## Entry Points

### Desktop

| Entry point | What it does |
|------|------|
| Account settings | Sign-in state, local sharing permission, and sharing level |
| Cloud agents and threads | Agents, recent threads, send, stop, tool approval, live status, history pages, and full-text copy |
| Local approval | Controlled requests from Cloud to this machine |

### Cloud (web)

| Entry point | What it does |
|------|------|
| Sidebar | Local workspaces sit beside the cloud workspace: the default root is “device name-Local workspace”, named roots are “device name-workspace name”, and selecting one expands that workspace’s local threads beneath it |
| The "cloud / device" dropdown switcher | Removed |

### CLI

| Entry point | What it does |
|------|------|
| The `cloud` subcommand group | `devices` / `ws` / `threads` / `thread` / `send` / `watch` / `audit` / `approve` |
| `cloud status` | Combines four sections: interlink tunnel, pending approvals, other devices, cloud workspace usage |
| `cloud watch` | Frame types are `snapshot` / `delta` / `command` / `error` / `close` |

## Security Boundary

| Boundary | Meaning |
|------|------|
| No public inbound entry point | Reverse tunnel: the local side only makes outbound connections, opens no listening port, and control traffic is delivered over the already-established link |
| Least capability | Each class of remote operation is a declarative capability, read-only by default |
| Explicit authorization | Write and execution classes require local user authorization, and an administrator can tighten it further in the Admin |
| Graded approval | Risk tiers L0–L3, see the table below |
| Complete audit | Every remote operation forms a chain that can be inspected |
| One-click shutdown | The kill switch is held by the user |

Risk tiers:

| Level | Operations covered | Approval |
|------|------|------|
| L0 read-only | `node.summary` / `shadow.refresh` / `workspace.list\|search\|stat\|read` / `threads.list\|get` | No approval needed |
| L0+ | `workspace.read:binary` | No approval needed + audited |
| L1 session | `thread.create` / `thread.message` / `thread.cancel` / `thread.answer` | Local-device operations normally need local approval; a user's own cloud threads use account authorization |
| L2 workspace write | `workspace.write\|mkdir\|move\|copy\|delete` | Approval mandatory, cannot be lowered |
| L3 execution | `tool.exec` (allow-listed commands) / `agent.spawn` | Approval mandatory + disabled for administrators by default, can be enabled |

- **L3 is off by default**: `tool.exec` and `agent.spawn` are not in a device's capability set by default; approvals are never remembered; only a summary of the execution result is returned.
- **L2 writes are backed up first**: write operations are backed up to `工作区/.interlink_backup/<command_id>/` (the most recent 50 generations are kept), and the result reports the SHA256 before and after.

## Constraints

### Disconnection and recovery

The Desktop reconnects with backoff. While the Server process remains running, its bounded queue retains commands that have not been sent and delivers them after reconnection. Commands already sent, executing, or awaiting approval on the device fail explicitly when their connection ends, avoiding duplicate operations.

After a Server restart, unfinished commands are settled in bounded batches with `SERVER_RESTARTED`. Command bodies are not persisted, so arguments are never reconstructed from digests and writes are not replayed automatically. Queries can be submitted again; check the actual outcome of a write before resubmitting it through the normal approval flow. Remote thread views obtain a new snapshot after reconnecting.

### Boundaries

- Local and cloud threads are not merged. Interlink is remote operation, not data synchronization.
- No full mirroring or two-way synchronization of workspace files — only metadata projection plus on-demand pull.
- No graphical remote control; control granularity stops at the engine and file APIs.
- No NAT hole-punching; all traffic is relayed through the Server.
- The Desktop and the CLI do not interlink directly; local-to-local access always goes through the Server.

Remote file reads use a 256 KiB inline-result boundary; larger device files use bounded chunks. Cloud-direct execution has no device data channel, so larger reads and writes must be split. Cloud does not offer binary upload, archive, or batch file operations.
