---
title: Quick Start
summary: The shortest path to complete your first task in wunder. Desktop by default, then Server or CLI.
read_when:
  - You're using wunder for the first time
  - You need to verify a working setup in 10 minutes
source_docs:
  - README.md
---

# Quick Start

This page does one thing: **help you complete your first task in 10 minutes**.

## Step 1: Pick Your Path

| Your Situation | Recommended Path | Why |
|----------------|-----------------|-----|
| I just want to start using it now | [Desktop Guide](/docs/en/start/desktop/) | Lowest barrier, download and go |
| I need team collaboration and management | [Server Deployment](/docs/en/start/server/) | Multi-user, permissions, unified management |
| I'm a developer, I want automation | [CLI Usage](/docs/en/start/cli/) | Terminal-driven, scriptable |

---

## Shortest Path: Desktop (Recommended)

For: Individual users, local demos

### 5 Steps to Get Running

1. **Download and Install**
   - Get the installer for your system from Releases
   - Install or extract, then launch

2. **Configure Model**
   - Open "System Settings" → "Model Configuration"
   - Enter your API Key and endpoint URL
   - Click "Test Connection" before saving

3. **Start Your First Conversation**
   - Go back to the chat interface
   - Type: `List the files in the current directory`
   - Press Enter

4. **Watch It Work**
   - You'll see: model thinks → calls tools → shows results → gives reply

5. **Verify**
   - Saw the full execution process and results? Congratulations, you're up and running!

### Desktop-Only Capabilities

- **Local-first**: Runs locally by default, can also connect to a remote server
- **Desktop control**: Can operate local windows, files, browsers
- **Persistent workspace**: Files are not auto-cleaned
- **Direct agent editing**: Adjust agent configuration and prompts anytime

---

## Team Path: Server

For: Multi-user collaboration, organizational governance

### Prerequisites

- Docker and Docker Compose (recommended)
- At least 4GB available memory

### 3 Steps to Deploy

1. **Get the Code**
   ```bash
   git clone <repo-url>
   cd wunder
   ```

2. **Start the Service**
   ```bash
   # x86 architecture
   docker-compose -f docker-compose-x86.yml up -d
   
   # ARM architecture
   docker-compose -f docker-compose-arm.yml up -d
   ```

3. **Access the System**
   - User frontend: http://localhost:18002
   - Admin & docs: http://localhost:18000
   - Default admin: admin / admin

### Server Core Capabilities

- **Multi-tenancy**: Users, organizations, layered permission management
- **Channel integration**: Feishu, WeChat, QQ, and more
- **Observability**: Monitoring, benchmarking, capability evaluation

---

## Developer Path: CLI

For: Developers, automation scripts

### Install and Run

```bash
# Build (requires Rust)
cargo build --release

# Run
./target/release/wunder-cli
```

### First Session

```bash
wunder-cli
> Write a Hello World Python script
```

### CLI-Only Capabilities

- **TUI interface**: Terminal interaction similar to Codex
- **Session management**: `/fork`, `/compact`, `/resume`
- **JSONL output**: Easy pipe and automation integration

---

## Verification Checklist

No matter which path you chose, confirm:

- [ ] You can successfully start a conversation
- [ ] You can see intermediate steps and tool calls
- [ ] You can get the final result
- [ ] You know what to read next

---

## Next Steps

- Want to understand the system? → [Core Concepts](/docs/en/concepts/)
- Need to integrate with your system? → [Integration Overview](/docs/en/integration/)
- Running into issues? → [Troubleshooting](/docs/en/help/troubleshooting/)
- Want to see all tools? → [Tools Overview](/docs/en/tools/)
