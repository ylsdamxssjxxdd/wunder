---
title: Tools
summary: How the four layers — tools, skills, capabilities, and user tools — are told apart, canonical names and aliases, dispatch order for dynamic registration, default sets and visibility gates, and the two result-trimming tracks.
updated_at: 2026-10-16
---

The agent's ability to act is collectively called "tools," but tools, skills, capabilities, and user tools are four different layers, and conflating them is the most common cause of broken configuration.

![Tools layer diagram: tool description, execution layer, model input results, and display projection—four related layers](/docs/assets/manual/core-tools.svg)

## The four layers

| Layer | What it is | Who uses it | How to tell |
| --- | --- | --- | --- |
| **Tool** | Executor: one call reads a file, runs a command, or sends a request | The model calls it directly; the engine dispatches and runs it | Registered in the tool catalog and carries a dispatch branch |
| **Skill** | Knowledge-and-script package: a directory holding the `SKILL.md` body and an entry script | The model can call it by skill name, or read the body through the skill-call action | Registered in the skill catalog |
| **Capability** | Selection unit on the governance side: a unified descriptor an administrator uses to choose what an agent may use | Configured on the admin side; never enters the execution chain | Its source is built-in, MCP, skill, knowledge base, user MCP, user skill, user knowledge base, or shared |
| **User tool** | An extension the user connects, in three kinds: MCP, skill, knowledge base | Mounted per user; the alias enters the routing table | Registered in the user tool catalog |

A capability being selected does not mean an executor exists, and an executor existing does not mean the model can see it.

## Canonical names and aliases

Most of the tool names the model sees are Chinese canonical names, with English aliases alongside, and a normalization pass runs before every invocation.

| Canonical | Common aliases | Canonical | Common aliases |
| --- | --- | --- | --- |
| `执行命令` | `execute_command` | `读取文件` | `read_file` |
| `命令会话` | `command_session`, `write_command_stdin` | `写入文件` | `write_file` |
| `ptc` | `programmatic_tool_call` | `文本编辑` | `edit`, `patch`, `apply_patch`, `应用补丁` |
| `列出文件` | `list_files` | `搜索内容` | `search_content` |
| `glob` | — | `技能调用` | `skill_call`, `skill_get` |
| `计划面板` | `update_plan` | `问询面板` | `question_panel`, `ask_panel` |
| `定时任务` | `schedule_task` | `记忆管理` | `memory_manager`, `memory_manage` |
| `子智能体控制` | `subagent_control` | `网页抓取` | `web_fetch` |

When a tool is written down in configuration, logs, or documentation, give both the canonical name and the alias; writing only one makes the two easy to mismatch.

## Dynamic registration

The names that enter the model's tool list are assembled from four kinds of dynamic registration.

| Source | Name on the model side | Notes |
| --- | --- | --- |
| MCP server | `server@tool` | Registered tool by tool; on a name clash the display name gains a server-name or hash suffix |
| Packed MCP | `server@__mcp_pack__` | A server marked as packed folds into a single tool whose actions are list, get, and call |
| Skill | The skill name itself | A skill name can be called directly; the skill-call action reads the body and substitutes the skill root-path placeholder |
| Knowledge base | The knowledge base name | Registered one base at a time, replacing the old single knowledge tool |
| User-tool alias | The alias | Dispatch routes by alias first; a hit goes straight to the user tool |

Dispatch order is alias → skill → MCP → knowledge base → built-in tools, and a hit earlier in the chain short-circuits the rest.

## Default sets and per-form differences

Default tool sets, the desktop hidden set, and the desktop extra set all have a single registration; do not keep a second copy in the docs or the configuration.

| Set | Contents |
| --- | --- |
| Default built-ins (13) | `定时任务`, `记忆管理`, `执行命令`, `命令会话`, `ptc`, `列出文件`, `glob`, `搜索内容`, `读取文件`, `网页抓取`, `技能调用`, `写入文件`, `文本编辑` |
| Default skills | `技能创建器`, `公文写作`, `文稿校对`, `3D网页`, `视频制作` |
| Desktop hidden (neither selectable nor on by default) | `goal` (`action=get|create|edit|pause|resume|complete|blocked`, injected automatically by the runtime per session), `会话线程控制`, `会话让出` |
| Desktop extras, on by default | `读图工具`, `桌面控制`, `子智能体控制` |

`网页搜索` and `ptc` are **temporarily withdrawn, not deleted**: the code is kept and dispatch still recognizes them, but the availability check returns unavailable for both, so neither the model nor the user can see them. To restore them, remove the matching entries from the withdrawal list.

Visibility also runs through a few real gates.

| Gate | Effect |
| --- | --- |
| The model has no vision | Filters out `读图工具` and the desktop tools |
| The matching model or service is not configured | Speech generation, speech-to-text, image generation, and video generation each go unavailable |
| Configuration not enabled | `网页抓取`, `网页搜索`, `browser`, and the desktop tools |
| Form | The desktop form filters out the hidden set |

Browser is one strict tool: once enabled the model calls `browser(action=...)`, and each action exposes only its required fields. The aggregate display name "Browser" represents that same capability.

## Result trimming

**Display trimming and model-input trimming are separate and do not affect each other**.

| Track | Rule | Affects |
| --- | --- | --- |
| Display | A preview of at most 2,400 characters or 16 lines, with an ellipsis past that | Only the client display; the raw result is kept in full and can be copied |
| Model input | Trimmed by budget, with reproducible old observations compressed | What the model sees at the next step |

Seeing "…" in the UI does not mean the model sees that same truncated copy; and when the model's context has been compressed to a preview, the UI may still show the full content. To judge context pressure, look at the compression page and the usage page, not the collapsing in the UI.

## Approval tiers

Approvals are filed by the tool's risk category, and there are four categories: read-only, file write, running a command or a script, and controlling host capabilities.

- Pending requests are registered in two kinds by source: chat connection and channel.
- The cache that lets one approval cover a short window lasts 600 seconds within the same session.
- Execution policy has three states: allow, audit, and enforce; under enforce, high-risk actions must have human approval.
- A front-end confirmation cannot replace back-end authorization.
