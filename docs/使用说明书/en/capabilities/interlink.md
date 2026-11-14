---
title: Multi-Device Interlink
summary: Device states after signing in to the Server, the workspace shadow projection, tiered approval for cross-device control, and the drop and permission boundaries.
updated_at: 2026-11-14
---

# Multi-Device Interlink

Desktop and Android are first of all local workbenches that run offline. After signing in to the Server you can reach cloud agents and threads without opening a local port; once the user enables local sharing, Cloud can go through the Server relay to view and operate that device's workspaces and threads within the granted scope. The CLI exposes the same ability for hands-on and scripted use.

## Three states, not one

| State | Meaning |
| --- | --- |
| Signed in | Cloud resources are reachable |
| Tunnel online | Cloud can operate this local device |
| This operation is authorized | The specific write or exec is allowed |

The first sign-in only yields a separate device session; local files and threads are not opened automatically. The sharing level (off, summary, full) is chosen in account settings, and the Admin can tighten it further.

## Device states

`online` (idle), `busy` (running), `away` (online but idle for a while), `reconnecting`, `offline`. The Server records the signed-in session, the tunnel connection and the available capabilities separately, so the three can disagree.

## Capabilities

| Capability | What it means |
| --- | --- |
| Mutual workspace view | A local workspace appears in Cloud as a shadow projection of metadata (thread index, directory summary, scheduled tasks, usage); file contents are not mirrored and are pulled on demand over the tunnel |
| Cloud controls local | Queries, session control, workspace writes and tool execution are opened in tiers; sensitive ones go through a local human-in-the-loop approval and are audited |
| Local controls Cloud | Desktop, Android and the CLI treat Cloud as a peer node: switch device, browse Cloud threads, send messages, read the event stream |
| Several devices | The device directory distinguishes desktop, phone, CLI and web; a signed-in phone appears as a phone node that Cloud can drive under capability and approval |

## Entry points

| Form | Entry |
| --- | --- |
| Desktop | Account settings (sign-in and sharing level), cloud agents and threads, local approval dialog |
| Cloud | Sidebar lists local and cloud workspaces together: the default directory as "device name - local workspace", named ones as "device name - workspace name" |
| CLI | `cloud` subcommands: `devices`, `ws`, `threads`, `thread`, `send`, `watch`, `audit`, `approve`; `cloud status` summarises tunnel, pending approvals, other devices and usage |

## Tiers and approval

| Tier | Operations | Approval |
| --- | --- | --- |
| Read-only | device summary, shadow refresh, workspace list / search / read, thread list and detail | none |
| Read-only (binary) | reading binary files | none, audited |
| Session | create thread, send message, cancel, answer | local approval when acting on a local device; account auth when acting on your own cloud thread |
| Workspace write | write, mkdir, move, copy, delete | forced, cannot be lowered |
| Execution | allow-listed command exec, spawning subagents | forced, disabled by default in the device capability set, can be enabled |

Workspace writes back up first to `workspace/.interlink_backup/<command id>/` (last 50 generations) and report checksums before and after. Execution is off by default and approvals are never remembered.

## Boundaries

- **No public entry**: a reverse tunnel; the local side only makes an outbound connection and never listens.
- **Least capability**: each remote operation must be declared, read-only by default.
- **One switch**: the user holds the kill switch; the Admin can narrow it further.

## Drops and recovery

Desktop reconnects with backoff. While the Server stays up, an unsent bounded queue is delivered after reconnect; commands already sent, running or awaiting approval fail explicitly when the old connection ends, so nothing runs twice.

After a Server restart, leftover commands settle as "server restarted" instead of waiting forever; command bodies are not persisted, so parameters are never rebuilt from a summary and writes are never redone automatically. Queries can be reissued; verify the actual result before reissuing a write.

## What it deliberately does not do

- Local and cloud threads are not merged: this is remote operation, not sync.
- No full workspace mirroring or two-way sync, only metadata projection plus on-demand pulls.
- No graphical remote control; the granularity stops at the engine and file APIs.
- No NAT traversal; all traffic is relayed by the Server, and Desktop and the CLI never talk to each other directly.

Remote reads return up to 256 KiB inline, with bounded chunking beyond that; direct Cloud execution has no device data channel, so larger reads and writes must be split. Cloud offers no binary upload, archive or batch file operations.
