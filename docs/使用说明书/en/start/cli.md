---
title: CLI Usage
summary: Read this page when you need terminal execution, scripted integration, or developer-oriented debugging through `wunder-cli`.
read_when:
  - You want to use wunder directly from a terminal
  - You care more about debugging, scripting tasks, and workspace-driven execution
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
---

# CLI Usage

If you mainly work in a terminal, start here.

`wunder-cli` is wunder's command-line runtime form. It is designed for developers, local tasks, scriptable workflows, and workspace-driven execution.

---

## When to choose CLI

| Scenario | Choose CLI |
|------|--------|
| Programming work or file-processing tasks | ✅ |
| Watching events, tool calls, and artifacts in the terminal | ✅ |
| Scripts, CI, batch jobs, or automation | ✅ |
| JSONL event streams or TUI interaction | ✅ |
| Daily end-user use without touching a terminal | ❌ Choose Desktop |

---

## What CLI is

CLI is not "Desktop with fewer features." It is a **terminal-first independent runtime form**:

```text
┌─────────────────────────────────────┐
│     wunder-cli                      │
│  ┌───────────────────────────────┐ │
│  │  Optional TUI interface       │ │
│  │  - Codex-like terminal flow   │ │
│  │  - real-time event stream     │ │
│  └───────────────────────────────┘ │
│  ↓ command-line driven             │
│  ┌───────────────────────────────┐ │
│  │  Rust core engine             │ │
│  │  - orchestrator               │ │
│  │  - toolchain                  │ │
│  │  - current dir as workspace   │ │
│  └───────────────────────────────┘ │
└─────────────────────────────────────┘
```

### Core CLI capabilities

- complete agent orchestration and toolchain
- TUI interaction with a Codex-like flow
- JSONL output for pipes and automation
- session management with `fork`, `compact`, and `resume`
- channel integration such as Feishu, WeChat, QQ, and XMPP
- debugging utilities including `debug-config` and `statusline`

---

## Five steps to get started

### 1. Install CLI

**Build from source, requires Rust 1.70+**
```bash
git clone <repo-url>
cd wunder
cargo build --release
```

After the build, the executable is in `./target/release/wunder-cli`.

### 2. Configure a model

Create or edit `~/.wunder/config.yaml`:

```yaml
llm:
  models:
    - name: gpt-4o
      api_key: your-api-key
      endpoint: https://api.openai.com/v1
      max_context: 128000
      max_rounds: 20
```

### 3. Start interactive mode

```bash
cd my-project
wunder-cli
```

You will see something like:

```text
wunder-cli v0.1.0
Workspace: /path/to/my-project
Type /help for commands
>
```

### 4. Launch the first task

```text
> Help me list the files in the current directory
```

You will see:

- model reasoning
- a file-listing tool call
- the returned file list
- the final answer

### 5. Explore built-in commands

Try these:

| Command | Meaning |
|------|------|
| `/help` | show all available commands |
| `/fork` | fork the current session |
| `/compact` | compress session history |
| `/resume <id>` | resume a previous session |
| `/debug-config` | show the active configuration |
| `/statusline` | show runtime status |

---

## CLI-specific strengths

### TUI mode

Start with `--tui`:

```bash
wunder-cli --tui
```

This gives you a Codex-like terminal experience:

- left: session list
- center: chat area
- right: tool calls and status
- bottom: input box

### Session management

```text
> /fork
> /compact
> /resume abc123
> /list
```

### Scripting and automation

**Single-shot execution**
```bash
wunder-cli --execute "Help me write a Hello World program"
```

**JSONL output for pipes**
```bash
wunder-cli --jsonl --execute "List files" | jq .
```

**Read the task from a file**
```bash
wunder-cli --file task.txt
```

### Workspace-driven execution

By default, CLI uses the **current directory** as its workspace:

```bash
cd my-project
wunder-cli
```

You can also specify one explicitly:

```bash
wunder-cli --workspace /path/to/workspace
```

---

## Common questions

**Q: Does CLI need network access?**  
A: Model calls do, but many tool operations can stay local.

**Q: Can it work with Desktop or Server?**  
A: Yes. All three forms share the same core engine and use compatible workspace conventions.

**Q: How should I use it in CI/CD?**  
A: Use `--execute` together with `--jsonl` for script-friendly integration.

**Q: Where is session data stored?**  
A: By default in `~/.wunder/sessions/`, and this can be changed by configuration.

---

## Next

- Want the full tool catalog? -> [Tools Overview](/docs/en/tools/)
- Need integration patterns? -> [Integration Overview](/docs/en/integration/)
- Hit a problem? -> [Troubleshooting](/docs/en/help/troubleshooting/)
- Want the core system model? -> [Core Concepts](/docs/en/concepts/)
