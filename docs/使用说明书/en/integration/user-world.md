---
title: User World API
summary: One-to-one chats, group chats, and read state between users all live here, parallel to the user-to-agent conversation domain.
updated_at: 2026-10-13
---

wunder is not only "users chatting with an agent". It also keeps a separate "user-to-user communication domain":

- `/wunder/user_world/*`

## User World vs. Chat Domain

- Your goal is to send tasks to a model, manage agent sessions, or watch tool calls → use the [wunder API](/docs/en/integration/wunder-api/) or [chat sessions](/docs/en/integration/chat-sessions/), not this.
- Your goal is contacts, one-to-one chats between users, group chats, or read state → use this.

The dividing line: `/wunder/chat/*` faces users and agents, while `/wunder/user_world/*` faces users and users. Mixing the two tangles frontend state and authentication.

## Wiring Up a Full Client

1. Fetch contacts and the conversation list;
2. Once inside a conversation, fetch message history;
3. Establish `/wunder/user_world/ws`;
4. On receiving new messages and read updates, refresh the interface incrementally.

## Common Endpoints

Contacts and group chats:

- `GET /wunder/user_world/contacts`
- `GET /wunder/user_world/groups`
- `POST /wunder/user_world/groups`

Conversations:

- `POST /wunder/user_world/conversations`
- `GET /wunder/user_world/conversations`
- `GET /wunder/user_world/conversations/{conversation_id}`

Messages:

- `GET /wunder/user_world/conversations/{conversation_id}/messages`
- `POST /wunder/user_world/conversations/{conversation_id}/messages`
- `POST /wunder/user_world/conversations/{conversation_id}/read`

Real-time streams:

- `GET /wunder/user_world/conversations/{conversation_id}/events`
- `GET /wunder/user_world/ws`

## Key Settings and Common Pitfalls

- As with the main chat domain, prefer WebSocket: contacts, conversations, and read state suit real-time push, and SSE is only for compatibility or as a fallback.
- There are two core events: `uw.message` (new message) and `uw.read` (read update).
- Conversations come in `direct` (one-to-one) and `group`; a group also carries the group ID, group name, member count, and announcement, so the frontend does not have to guess.
- It is not only plain text: file download and voice messages are supported inside a conversation. Voice uses `voice` or `audio/*` as its type, and the content carries metadata such as path and duration as JSON.

## Further Reading

- [User-Side Frontend](/docs/en/surfaces/frontend/)
- [Chat WebSocket](/docs/en/integration/chat-ws/)
- [External Login and Embedded SSO](/docs/en/integration/external-login/)
