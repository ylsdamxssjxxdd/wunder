---
title: Bridge Panel Guide
summary: What each panel in the Bridge manages and where its entry lives.
updated_at: 2026-10-13
---

The Bridge is the administrator's workbench: configure the system, manage users, and watch how things are running.

## Panel groups at a glance

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="#system-group"><strong>System group</strong><span>Global configuration and status</span></a>
  <a class="docs-card" href="#agent-group"><strong>Agent group</strong><span>Model and capability configuration</span></a>
  <a class="docs-card" href="#debug-group"><strong>Debug group</strong><span>Testing and performance evaluation</span></a>
  <a class="docs-card" href="#documentation-group"><strong>Documentation group</strong><span>User guide and interface reference</span></a>
</div>

## System group

### Internal status

Shows how the system is running, including active sessions, system load, and the state of key components.

### System settings

Configures global parameters, including the service address and port, security settings, storage settings, and tool switches.

### Organization management

Manages the organizational structure: create and manage organizations, their hierarchy, and their members.

### User management

Manages user accounts: view the user list, create users, edit user details, and manage quota accounts (grant, deduct, set balance).

Inside a user's settings, the quota actions are:

- **Set the balance directly**: overwrites the current balance, which is useful for correcting the recorded figure
- **Grant quota**: increases the balance and records the cumulative amount received
- **Deduct quota**: reduces the balance and records the cumulative amount consumed
- Administrator accounts are not subject to balance limits

The online status in the user list is split by client: Beehive (web), Pupa (desktop), and Helm (command line).

## Agent group

### Model configuration

Manages the AI models available: add a model by filling in its API key and endpoint, configure model parameters, and set the default model.

### Thread management

Views and manages active threads: thread status, force-cancelling a thread, and viewing thread logs.

### Preset agents

Manages preset agent templates (there can be several, and each user is bound one-to-one to one of them):

- Create, edit, and delete presets
- Configure a preset's tools, model, prompts, and the fields the user may customize
- View bound users and adjust bindings in bulk (bind, rebind, unbind); unbinding requires naming a new preset
- Sync to bound users' instances — sync only the fields a user has not customized, or force-overwrite everything, with a dry run that previews how many users are affected

### Tool management

Manages the tools available: view the built-in tool list, configure external MCP tools, and set tool permissions.

### System prompts

Manages the global prompt templates: view and edit system templates, switch the default template pack, and create new packs.

## Debug group

### Throughput testing

Choose a model, a concurrency list, an input length, and an output length. Each run tests one concurrency level from the list in order and fires that many requests at once. The default concurrency list is `1,2,4,8`, the default input is `8.2k` tokens, and the default output is `128` tokens; the usual presets are kept, and you can also type any integer within range. Token counts are shown as `k` from 1,000 and as `m` from 100,000. History snapshots show the input usage actually measured by the API. The right side shows the aggregated input and output, the first-token latency, the generation speed, and the total time; the history curve plots concurrency on the horizontal axis and shows both overall and per-request speeds.

Engines that support it receive a fixed-length output parameter; other endpoints may finish early, and the actual usage and the reason for ending are kept in the details. Tests create no sessions or thread logs and keep only the last 50 performance summaries. The details and curves separate overall prefill and generation speed, per-prefill speed, and per-generation speed; speeds are shown as `k tok/s` from 1,000 and as `m tok/s` from 100,000. Selecting history entries compares performance curves by concurrency; the vertical axis uses concurrency 1 as the 0% baseline and shows relative change, hovering reveals the measured speed, and you can reuse a configuration or export the selected data.

For virtual models, model configuration offers fast, medium, and slow profiles, defaulting to fast: prefill at 2000 / 500 / 100 tokens/s and reasoning plus generation at 200 / 50 / 10 tokens/s. Random replies and throughput simulations both reason before generating content, and log replay uses the recorded reasoning within the configured capability and budget. Throughput tests default to reasoning equal to one quarter of the total output and can be tuned with the reasoning switch and budget; simulated records are tagged and grouped separately by profile.

### Simulated model capabilities

With a virtual model selected in model configuration, you can set the maximum context, maximum output, vision, hearing, reasoning, and tool capabilities. Context and output left empty default to 128k and 4k. When the input (including tool definitions) plus the output budget exceeds the window, a context-overflow is reported. Turning off vision or hearing makes the corresponding media requests fail. Images and audio can each carry a per-item token count; this only simulates usage and time and does not recognize content.

Replay follows the tool-calling mode: `function_call` uses the native tool channel, `tool_call` uses the text protocol, and `freeform_call` falls back to native or text depending on the API mode. Tool names and arguments come straight from the log, so nothing has to be filled in by hand; tool execution still follows permissions and approvals, and replay continues to the next model round once results come back. Running out of log entries raises an error rather than repeating old calls. Synthetic replies do not initiate tools. An over-long output is marked as a length termination, incomplete tool arguments are not executed, and the reasoning capability and budget also apply to throughput tests.

### Performance testing

Tests response performance with single-request latency measurement and performance sampling.

### Capability evaluation

Evaluates an agent's capability level with task-completion quality assessment and capability regression tests.

### Debug panel

Live debugging tools: real-time request tracing, tool-call logs, and error investigation.

### LSP

Language service protocol debugging: code intelligence service status and LSP configuration validation.

## Documentation group

### User guide

This is the documentation site you are reading now:

- Getting started
- Concepts
- Operating references
- Troubleshooting

### Interface reference

Lists interface information:

- Endpoint addresses
- Field descriptions
- Usage examples

## Panels by role

### Administrators' day-to-day work

Most used:

- System settings (tuning configuration)
- User management (handling user issues)
- User guide (looking up how to do something)

### Technical operations

Most used:

- Internal status (system health)
- Thread management (handling abnormal sessions)
- Performance testing (tracking down performance problems)
- Debug panel (locating issues)

### Operations staff

Most used:

- User management (user service)
- Organization management (organizational changes)
- User guide (how-to guidance)

## Common workflows

### Adding a new user

1. User management → create user
2. Set the username, password, and organization
3. Grant quota as needed
4. The user can now log in and start working

### Changing system settings

1. System settings → find the relevant option
2. Change the value
3. Save (usually takes effect immediately)
4. Confirm the effect in Internal status

### Handling an abnormal session

1. Thread management → look at the abnormal thread
2. Check the thread logs and status
3. To stop it, click cancel
4. Review the related logs to locate the problem

## Quick locator

| I want to… | Go to this panel |
|---------|------------|
| Add a user | User management |
| Grant a user quota | User management → open the user → Settings |
| See how the system is running | Internal status |
| Adjust system configuration | System settings |
| Add a new model | Model configuration |
| View active sessions | Thread management or Internal status |
| Handle a stuck thread | Thread management → cancel |
| Test system performance | Performance testing or Throughput testing |
| Look up how to use something | User guide |

## How to reach the Bridge

The Bridge is usually reached at its own address:

- Open the Bridge URL in a browser
- Sign in with an administrator account
- The management panels are available once you are in

The exact address is fixed at deployment time; ask whoever deployed the system or check the deployment documentation.

## Security reminders

The Bridge holds the highest privileges in the system:

- Only authorized administrators can access it
- Administrator passwords should be strong
- Rotate administrator passwords regularly
- Confirm the scope of impact before acting
- Keep records of important actions

## Further reading

- [System Settings Reference](/docs/en/reference/config/)
