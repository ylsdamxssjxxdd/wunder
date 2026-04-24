---
title: FAQ
summary: Common questions about using wunder. Check here first before detailed troubleshooting.
read_when:
  - You have a usage question
  - You want to quickly determine if something is a bug
source_docs:
  - docs/API文档.md
updated_at: 2026-04-10
---

# FAQ

## Do I need to register an account first?

No. You can start a conversation with any name, and the system will create an identity for you automatically.

## Does the Token count shown equal my cost?

No. The Token count is just a reference for the current conversation's context length. Actual cost depends on your model provider's pricing.

## Does the agent re-learn my requirements every time I send a message?

No. Requirements you set at the beginning of a conversation are remembered and locked in. Subsequent messages will follow them without you needing to repeat.

## Does the agent re-read my long-term memory every conversation turn?

No. Your long-term memory is loaded once at the start of the conversation, not repeatedly in every turn.

## Why is the "New Thread" button sometimes grayed out?

Because the current agent is still running. Wait for it to finish, or stop the current session first.

## Swarm vs. Sub-agent — which should I use?

- **Swarm**: Have existing agents collaborate on a task
- **Sub-agent**: Temporarily spawn an independent agent from the current session

## Does Desktop require deploying Server first?

No. Desktop can run independently locally. Deploy Server when you need multi-user collaboration and unified management.

## How do I change my username or password?

Go to "My Profile → Edit". Changing your password requires entering your current password.

## What if I forgot my password?

Click "Reset Password" on the login page. You only need your username, email, and a new password — no old password required.

## Will uploaded files be automatically deleted?

Files in the temporary directory may be cleaned up. Put important files in the workspace's persistent directory.

## Why can't I see some tools in a session?

Possible reasons: the tool is not enabled, MCP/A2A services are not running, or the current agent doesn't have that tool mounted.

## What's the difference between `web_fetch` and the browser tool?

- **web_fetch**: Grabs the main text content of a webpage
- **Browser tool**: Operates a webpage like a real person (clicking, scrolling, filling forms)

## Where should I look when something goes wrong?

Start with [Troubleshooting](/docs/en/help/troubleshooting/).
