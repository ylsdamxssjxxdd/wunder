---
title: Cloud Usage
nav_title: Usage
summary: Cloud usage: sign-in, realtime state, running in the background and attachment limits.
updated_at: 2026-11-14
---

# Cloud Usage

## Usage

- **Sign-in**: use a Server account; an unauthenticated visit lands on the login page, and self-registration is available. After signing in you land on your agent's chat page.
- **Realtime state**: new messages, progress and approvals ride one long connection; after a reconnect, ordinary requests backfill the state without reloading the page.
- **Background runs**: switching, creating, refreshing or briefly going offline does not cancel a running thread; on reconnect the state and pending approvals are restored from saved events.
- **Attachments**: 200 MB for temporary attachments, 10 MB for persistent ones; the send button is disabled while an attachment is processed.
- **Long threads**: the timeline loads in windows as you scroll, with parsing and rendering off the main thread.

## Boundaries

- **No interactive terminal**: use Desktop if you want to type commands inside the interface.
- **Cloud and the Admin are different interfaces**: Cloud serves users, the Admin serves administrators, with different routing and visual rules; ordinary users never see the Admin.
- **Very large threads can still stutter**: the high-volume rendering plan has not fully landed.
