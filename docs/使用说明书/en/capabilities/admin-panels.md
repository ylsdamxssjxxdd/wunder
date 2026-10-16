---
title: Bridge Panel Guide
summary: What each panel in the four groups manages, plus the throughput-test, simulated-model, and random-simulator conventions.
updated_at: 2026-10-16
---

The Bridge is reached at its own address: open the Bridge URL in a browser and sign in with an administrator account, and the management panels are available. The exact address is fixed at deployment time; ask whoever deployed the system or check the deployment documentation. Panels are organized into four groups: system, agent, debug, and documentation.

The Bridge holds the highest privileges in the system:

- Only authorized administrators can access it
- Administrator passwords should be strong
- Rotate administrator passwords regularly
- Confirm the scope of impact before acting
- Keep records of important actions

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

## System group

### Internal status

Shows how the system is running:

- Active sessions
- System load
- State of key components

### System settings

Global parameters:

- Service address and port
- Security settings
- Storage settings
- Tool switches

Changing a system setting:

1. System settings → find the relevant option
2. Change the value
3. Save (usually takes effect immediately)
4. Confirm the effect in Internal status

### Organization management

Manages the organizational structure:

- Create and manage organizations
- Organization hierarchy
- Organization members

### User management

Manages user accounts:

- View the user list
- Create users
- Edit user details
- Manage quota accounts (grant, deduct, set balance)

The quota actions inside a user's settings:

- **Set the balance directly**: overwrites the current balance, which is useful for correcting the recorded figure
- **Grant quota**: increases the balance and records the cumulative amount received
- **Deduct quota**: reduces the balance and records the cumulative amount consumed
- Administrator accounts are not subject to balance limits

The online status in the user list is split by client: Beehive (web), Pupa (desktop), and Helm (command line).

Adding a new user:

1. User management → create user
2. Set the username, password, and organization
3. Grant quota as needed
4. The user can now log in and start working

## Agent group

### Model configuration

Manages the AI models available:

- Add a model by filling in its API key and endpoint
- Configure model parameters
- Set the default model

### Thread management

Views and manages active threads:

- Thread status
- Force-cancelling a thread
- Viewing thread logs

Handling an abnormal session:

1. Thread management → look at the abnormal thread
2. Check the thread logs and status
3. To stop it, click cancel
4. Review the related logs to locate the problem

### Preset agents

There can be several preset agent templates, and each user is bound one-to-one to one of them.

- Create, edit, and delete presets
- Configure a preset's tools, model, prompts, and the fields the user may customize
- View bound users and adjust bindings in bulk (bind, rebind, unbind); unbinding requires naming a new preset
- Sync to bound users' instances — sync only the fields a user has not customized, or force-overwrite everything, with a dry run that previews how many users are affected

### Tool management

Manages the tools available:

- View the built-in tool list
- Configure external MCP tools
- Set tool permissions

### System prompts

Manages the global prompt templates:

- View and edit system templates
- Switch the default template pack
- Create new packs

## Debug group

### Throughput testing

Test parameters: a model, a concurrency list, an input length, and an output length. Each run tests one concurrency level from the list in order and fires that many requests at once. The default concurrency list is `1,2,4,8`, the default input is `8.2k` tokens, and the default output is `128` tokens; the usual presets are kept, and you can also type any integer within range.

Readings: token counts are shown as `k` from 1,000 and as `m` from 100,000; history snapshots show the input usage actually measured by the API; the right side shows the aggregated input and output, the first-token latency, the generation speed, and the total time; the history curve plots concurrency on the horizontal axis and shows both overall and per-request speeds.

- Engines that support it receive a fixed-length output parameter; other endpoints may finish early, and the actual usage and the reason for ending are kept in the details.
- Tests create no sessions or thread logs and keep only the last 50 performance summaries.
- The details and curves separate overall prefill and generation speed, per-prefill speed, and per-generation speed; speeds are shown as `k tok/s` from 1,000 and as `m tok/s` from 100,000.
- Selecting history entries compares performance curves by concurrency: the vertical axis uses concurrency 1 as the 0% baseline and shows relative change, hovering reveals the measured speed, and you can reuse a configuration or export the selected data.

For virtual models, model configuration offers fast, medium, and slow profiles, defaulting to fast: prefill at 2000 / 500 / 100 tokens/s and reasoning plus generation at 200 / 50 / 10 tokens/s. Random replies and throughput simulations both reason before generating content, and log replay uses the recorded reasoning within the configured capability and budget. Throughput tests default to reasoning equal to one quarter of the total output and can be tuned with the reasoning switch and budget; simulated records are tagged and grouped separately by profile.

### Simulated model capabilities

With a virtual model selected in model configuration, you can set the maximum context, maximum output, vision, hearing, reasoning, and tool capabilities. Context and output left empty default to 128k and 4k. When the input (including tool definitions) plus the output budget exceeds the window, a context-overflow is reported. Turning off vision or hearing makes the corresponding media requests fail. Images and audio can each carry a per-item token count; this only simulates usage and time and does not recognize content.

Replay follows the tool-calling mode: `function_call` uses the native tool channel, `tool_call` uses the text protocol, and `freeform_call` falls back to native or text depending on the API mode. Tool names and arguments come straight from the log, so nothing has to be filled in by hand; tool execution still follows permissions and approvals, and replay continues to the next model round once results come back. Running out of log entries raises an error rather than repeating old calls. Synthetic replies do not initiate tools. An over-long output is marked as a length termination, incomplete tool arguments are not executed, and the reasoning capability and budget also apply to throughput tests.

### Round conventions of the random simulator

With no replay log available, the random simulator follows conventions carried by the human input of the current turn. They behave identically in every form (蛹, 蜂巢, 舵机):

- Rounds: the first number in the input sets the model rounds for this turn and may carry an `s` / `轮` / `次` suffix (`12`, `12轮`, `1s` all read as 12 or 1). The value is capped at 2000 and anything below 1 counts as 1. Only the human input of the current turn is read — never history, so context compaction cannot change the plan mid-turn.
- Spawning a subagent: an input containing `subagent`, the shorthand `s` / `sub`, `子智能体` / `子代理`, or a number followed directly by `s` (`1s`) makes the first model round emit a real subagent dispatch (`spawn` or `batch_spawn`). Such turns span at least two rounds — one to dispatch, one to close — so `1s` still spawns the child before answering.
- Middle rounds pick read-only tools from the request's tool list and issue harmless calls; the last round outputs the summary. The plan is derived deterministically from (session, user round), so cancel-and-resume stays consistent.

### Performance testing

Tests response performance:

- Single-request latency measurement
- Performance sampling

### Capability evaluation

Evaluates an agent's capability level:

- Task-completion quality assessment
- Capability regression tests

### Debug panel

Live debugging tools:

- Real-time request tracing
- Tool-call logs
- Error investigation

### LSP

Language service protocol debugging:

- Code intelligence service status
- LSP configuration validation

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
