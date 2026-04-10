---
title: Quickstart
summary: Run your first usable wunder flow with the shortest path. Desktop is the default recommendation, followed by server or cli depending on your role.
read_when:
  - You are using wunder for the first time
  - You need a verifiable result within about ten minutes
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# Quickstart

This page has one job: **help you get the first working wunder flow up and running in ten minutes.**

## Step 1: pick the right path

| Your situation | Recommended path | Why |
|----------|----------|----------|
| I just want to start using it now | [Desktop Guide](/docs/en/start/desktop/) | Lowest barrier, ready to use immediately |
| I need team collaboration and administration | [Server Deployment](/docs/en/start/server/) | Multi-user, permissions, centralized governance |
| I am a developer and need automation | [CLI Usage](/docs/en/start/cli/) | Terminal-driven, scriptable, pipeline-friendly |

---

## Shortest path: Desktop

Best for: individual users, local demos, and a desktop workstation

### Five steps

1. **Download and install**
   - Go to Releases and download the `wunder-desktop` package for your system
   - Install it or unzip it, then launch it

2. **Configure a model**
   - Open `System Settings` -> `Model Configuration`
   - Fill in your API key and endpoint
   - Save and test the connection

3. **Start the first conversation**
   - Go back to the chat screen
   - Enter: `Help me list the files in the current directory`
   - Press Enter

4. **Watch the execution**
   - You will see:
     - model reasoning
     - tool calls such as file listing
     - intermediate results
     - the final reply

5. **Validate success**
   - If you can see the full execution flow and the final result, the core path is working

### Desktop-specific strengths

- **Local first**: runs locally by default, but can also connect to a remote gateway
- **Desktop control**: can operate local windows, files, and browsers
- **Persistent workspace**: files are not auto-cleaned after 24 hours
- **Agent-visible configuration**: agent settings and prompts can be edited directly

---

## Team path: Server

Best for: multi-user collaboration, organizational governance, and unified access

### Before you start

- Docker and Docker Compose, preferably
- A PostgreSQL database, or the one included in compose
- At least 4 GB of free memory

### Three-step deployment

1. **Get the code**
   ```bash
   git clone <repo-url>
   cd wunder
   ```

2. **Start the services**
   ```bash
   # x86
   docker-compose -f docker-compose-x86.yml up -d

   # ARM
   docker-compose -f docker-compose-arm.yml up -d
   ```

3. **Open the system**
   - User frontend: `http://localhost:18002`
   - Admin UI and docs: `http://localhost:18000`
   - Frontend dev service, only for direct integration debugging: `http://localhost:18001`
   - Default admin account: `admin / admin`

### Core server capabilities

- **Multi-tenancy**: hierarchical governance for users, units, and permissions
- **Unified interfaces**: `/wunder`, `/wunder/chat/*`, and `/a2a`
- **Channel integration**: Feishu, WeChat, QQ, XMPP, and more
- **Observability**: monitoring, benchmarking, and capability evaluation

---

## Developer path: CLI

Best for: developers, automation scripts, and terminal-driven tasks

### Install and run

```bash
# build, requires Rust
cargo build --release

# run
./target/release/wunder-cli
```

### First session

```bash
# start the interactive mode
wunder-cli

# enter a task
> Help me write a Hello World Python script
```

### CLI-specific strengths

- **TUI interface**: a terminal experience similar to Codex
- **Session management**: `/fork`, `/compact`, and `/resume`
- **Debugging tools**: `/debug-config` and `/statusline`
- **JSONL output**: easier to use in pipes and automation

---

## Validation checklist

No matter which path you choose, confirm these points:

- [ ] You can submit one execution request successfully
- [ ] You can see the streaming process, including intermediate steps and tool calls
- [ ] You receive the final result
- [ ] You know which document to read next

---

## Next

- Want the runtime model? -> [Core Concepts](/docs/en/concepts/)
- Need to integrate wunder into your own system? -> [Integration Overview](/docs/en/integration/)
- Hit a problem? -> [Troubleshooting](/docs/en/help/troubleshooting/)
- Want the full tool set? -> [Tools Overview](/docs/en/tools/)
