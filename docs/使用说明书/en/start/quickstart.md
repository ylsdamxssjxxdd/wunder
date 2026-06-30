---
title: Quick Start
summary: The shortest path to open Hive, configure a model, and complete your first task.
read_when:
  - You're using wunder for the first time
  - You need to verify a working setup
source_docs:
  - README.md
---

# Quick Start

This page helps you open Hive and complete your first task. Hive is where you work — individuals use the desktop app, team members use the web browser.

## Step 1: Open Hive

| Situation | What to do |
|----------|--------|
| Personal use | Install the [desktop app](/docs/en/start/desktop/), launch and go |
| Team member | After an admin [deploys the server](/docs/en/start/server/), open Hive in a browser |
| Automation / scripting | Use the [CLI](/docs/en/start/cli/) from a terminal |

Not sure which to pick? Default to the desktop app — lowest barrier.

## Shortest path: Desktop app

For individual users, download and go.

### 5 steps to get running

1. **Download and install**
   - Get the installer for your system from Releases
   - Install or extract, then launch

2. **Configure the model**
   - Open "System Settings" → "Model Configuration"
   - Enter your API Key and endpoint URL
   - Click "Test Connection" before saving

3. **Start your first conversation**
   - Go back to the chat interface
   - Type: `List the files in the current directory`
   - Press Enter

4. **Watch it work**
   - Model thinks → calls tools → shows results → replies

5. **Verify**
   - Saw the full execution process and results? You're up and running.

### Desktop-only capabilities

- **Local-first**: runs locally by default, can also connect to a remote server
- **Desktop control**: can operate local windows, files, browsers
- **Persistent workspace**: files are not auto-cleaned
- **Direct agent editing**: adjust agent configuration and prompts anytime

## Team path: Web browser

For multi-user collaboration and organizational governance.

### Prerequisites

- Docker and Docker Compose (recommended)
- At least 4GB available memory

### 3 steps to deploy

1. **Get the code**
   ```bash
   git clone <repo-url>
   cd wunder
   ```

2. **Start the service**
   ```bash
   # x86 architecture
   docker-compose -f docker-compose-x86.yml up -d
   
   # ARM architecture
   docker-compose -f docker-compose-arm.yml up -d
   ```

3. **Open Hive**
   - Hive: http://localhost:18002
   - Admin & docs: http://localhost:18000
   - Default admin: admin / admin

Once deployed, team members open Hive in a browser — no client install needed.

### Server core capabilities

- **Multi-tenancy**: layered management of users, organizations, permissions
- **Channel integration**: Feishu, WeChat, QQ, and more
- **Observability**: monitoring, benchmarking, capability evaluation

## Developer path: CLI

For developers and automation scripts.

### Install and run

```bash
# Build (requires Rust)
cargo build --release

# Run
./target/release/wunder-cli
```

### First session

```bash
wunder-cli
> Write a Hello World Python script
```

### CLI-only capabilities

- **TUI interface**: terminal interaction
- **Session management**: `/fork`, `/compact`, `/resume`
- **JSONL output**: easy pipe and automation integration

## Verification checklist

No matter which path you chose, confirm:

- [ ] You can successfully start a conversation
- [ ] You can see intermediate steps and tool calls
- [ ] You can get the final result
- [ ] You know what to read next

## Next steps

- Meet Hive → [Hive Interface](/docs/en/surfaces/frontend/)
- Understand the system → [Core Concepts](/docs/en/concepts/)
- Integrate with existing systems → [Integration Overview](/docs/en/integration/)
- See all tools → [Tools Overview](/docs/en/tools/)
- Running into issues → [Troubleshooting](/docs/en/help/troubleshooting/)
