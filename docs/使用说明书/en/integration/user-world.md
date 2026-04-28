---
title: User World
summary: `/wunder/user_world/*` is the user-to-user communication domain in Wunder, existing in parallel with the user-to-agent conversation domain.
read_when:
  - You need to integrate user-to-user direct or group chat
  - You want to understand the boundary between user_world and chat endpoints
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
  - src/api/user_world.rs
  - src/api/user_world_ws.rs
---

# User World

Wunder is not limited to "users chatting with agents."

It also maintains a separate "user-to-user communication domain":

- `/wunder/user_world/*`

## What Problem Does This Solve

It addresses these scenarios:

- Contact lists
- User-to-user direct chat
- Group chat
- Read receipts
- User world message event streams

In other words, `user_world` is not a sub-capability of the chat interface -- it is a parallel messaging domain.

## How It Differs from `/wunder/chat/*`

The core distinction in one sentence:

- `/wunder/chat/*` is for users and agents
- `/wunder/user_world/*` is for users and users

If you conflate these two domains, your frontend state, conversation model, and authentication will become tangled.

## Which Endpoints You Will Typically Use

### Contacts and Groups

- `GET /wunder/user_world/contacts`
- `GET /wunder/user_world/groups`
- `POST /wunder/user_world/groups`

### Conversations

- `POST /wunder/user_world/conversations`
- `GET /wunder/user_world/conversations`
- `GET /wunder/user_world/conversations/{conversation_id}`

### Messages

- `GET /wunder/user_world/conversations/{conversation_id}/messages`
- `POST /wunder/user_world/conversations/{conversation_id}/messages`
- `POST /wunder/user_world/conversations/{conversation_id}/read`

### Real-Time Streams

- `GET /wunder/user_world/conversations/{conversation_id}/events`
- `GET /wunder/user_world/ws`

## Why WebSocket Is Still Recommended

Like the main chat domain, user world also prioritizes WebSocket.

The reason is straightforward:

- Contact, conversation, and read receipt changes are better suited to real-time push
- In group chat scenarios, SSE is only suitable as a compatibility fallback

So if you are building a full client, prioritize connecting to:

- `/wunder/user_world/ws`

## What Are Events

The two most essential events are:

- `uw.message`
- `uw.read`

You can think of them as:

- New message
- Read receipt update

These are sufficient to drive most chat UIs.

## How to Distinguish Group Chat from Direct Chat

The model supports two conversation types:

- `direct`
- `group`

Group chat objects additionally carry:

- `group_id`
- `group_name`
- `member_count`
- `announcement`

So the frontend does not need to guess whether a conversation is a group chat.

## How Files and Voice Are Handled

User world is not limited to plain text.

It also supports:

- In-conversation file downloads
- Voice messages

For voice messages, `content_type` can be:

- `voice`
- `audio/*`

And `content` typically uses a JSON string carrying metadata such as `path`, `duration_ms`, etc.

## When You Should Not Use It

If your goal is to:

- Send tasks to a model
- Manage agent conversations
- View tool calls and intermediate steps

Then you should use `/wunder` or `/wunder/chat/*`, not `user_world`.

## Typical Integration Order

If you are building a full client, the recommended order is:

1. Fetch contacts and conversation lists
2. Once inside a conversation, fetch message history
3. Establish a WebSocket connection at `/wunder/user_world/ws`
4. Incrementally update the UI upon receiving `uw.message` and `uw.read` events

## Further Reading

- [User-Side Frontend](/docs/en/surfaces/frontend/)
- [Chat WebSocket](/docs/en/integration/chat-ws/)
- [External Login and Embedded SSO](/docs/en/integration/external-login/)
