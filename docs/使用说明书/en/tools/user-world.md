---
title: User World
summary: Looking up which users exist inside the site, and sending messages to users inside the site.
updated_at: 2026-10-13
---

`user_world` handles two things: looking up which users exist inside the site, and sending messages to users inside the site.

## When to use it

- You want to see who is on the site, or whether a given user is still active.
- You need to send a message to a user on the site, optionally with attachments.

## How to use it

Two actions:

- `list_users`: list the users on the site, returning their username, status, and unit, along with the total and pagination information.
- `send_message`: send a message to users, returning per user whether the send succeeded.

## What it handles

This is the in-site user world, not an external channel, and messages are delivered only within that scope. If a message carries attachments, the outcome of each attachment is listed separately.

## When something goes wrong

The result of sending a message gives a status per user; the users that did not succeed are flagged in the result, and you can act on the returned hint.
