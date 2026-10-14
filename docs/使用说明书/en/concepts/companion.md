---
title: Companion System
summary: A companion is a visual layer attached to an agent — a ZIP companion package plus a sprite sheet that follows the runtime state and takes no part in any execution logic.
read_when:
  - You want to add, replace, or trim a desktop pet and need to know the package format
  - A companion shows the wrong animation or the wrong size
  - You need to know who may upload companions and where they are stored
source_docs:
  - crates/wunder-runtime/src/services/companions.rs
  - crates/wunder-runtime/src/api/companions.rs
  - crates/wunder-runtime/src/api/admin/resource_admin.rs
  - frontend-slint/src/companion_sprite.rs
  - crates/wunder-desktop/native_companion_overlay.rs
updated_at: 2026-10-13
---

A companion is a visual layer of company: it plays an animation that matches the session's runtime state and occasionally shows a message bubble. **It takes no part in execution and does not affect scheduling; deleting a companion changes no functionality.**

## What a companion package looks like

A companion package is a ZIP that holds exactly two things: a manifest and a sprite-sheet image.

- The manifest (`pet.json`) is JSON and states the companion's id, display name, and description, along with where the sprite-sheet file lives.
- The sprite sheet is a single image whose path the manifest gives.
- Size limits: the whole ZIP is at most 24 MB, and the single sprite-sheet file is at most 18 MB.
- Import guards against path traversal inside the archive.

Companion files live in one fixed companion directory (`config/data/companions/global`, overridable through `WUNDER_COMPANIONS_ROOT`); embedded forms such as the Pupa and the Helm pin this directory to their own data directory. The directory starts empty, so no companion package ships built in and an empty list is normal.

## Sprite-sheet specification

- Every frame is a fixed **192 × 208** pixels, and frames of the same state line up horizontally in the same row; **one state occupies one row**.
- The row number is the state (rows 0–8):

| Row | State | Row | State |
| --- | --- | --- | --- |
| 0 | idle | 5 | failed |
| 1 | running-right | 6 | waiting |
| 2 | running-left | 7 | running |
| 3 | waving | 8 | review |
| 4 | jumping | | |

- The session's runtime state maps to an animation row: running → running (row 7), completed → jumping (row 4), waiting → waiting (row 6), error → failed (row 5), and everything else falls back to idle (row 0). Clicking the companion plays the waving animation once.
- A companion package may declare fewer rows or frames: out-of-range rows or frames are degraded by a bounds check and never crash.

## How each form presents it

| Form | Presentation |
| --- | --- |
| Pupa (Windows) | Native floating window: a Win32 layered window per pet, with dragging and the frame clock on a separate message thread that does not occupy the UI thread |
| Pupa (Linux) | **No equivalent floating window**; the desktop pet exists only on Windows |
| Beehive | An in-page overlay that follows the chat state |
| Bridge | A companion management panel: upload, edit, delete, and export global companions |
| Helm | No companion UI; it only points the companion directory at the local data directory |

## Scale and position

| Item | Range |
| --- | --- |
| Stored / menu scale | 0.5–1.6 (presets 0.5 / 0.8 / 1.0 / 1.2 / 1.4 / 1.6) |
| Render scale | 0.5–1.8 |
| Position | Clamped to the screen work area with margins kept; drag results are saved |

The Pupa stores position, per-companion show / scale overrides, and the global message-hint switch in a local file; the Beehive stores them in the browser (clearing browser data resets position and preferences).

## Who can change companions

- The user side is read-only: four GET endpoints only — the global list, one companion's detail, its sprite sheet, and its exported package (`/wunder/companions/global*`).
- All adding, changing, and deleting sits on the administrator side (`/wunder/admin/companions*`): list, import, edit and delete, sprite sheet, export package. The import request-body limit matches the ZIP limit (24 MB).
- A "private companion" in the Beehive takes effect only in the current browser and is not uploaded to the server; a global companion is uploaded by an administrator through the Bridge and is then visible to all users. The Bridge is the only upload path.

## Easy to get wrong

- A companion is not "another kind of agent"; it is only the visual shell of an agent, and one agent binds one companion.
- Users cannot upload a companion to the server from the Beehive; server-side companions are always managed by administrators through the Bridge, and private imports stay in the local browser.
- The Pupa on Linux has no desktop floating window — the overlay is a Win32 implementation with no Linux equivalent.
- Companion animation does not affect task execution: the runtime state drives the animation, not the other way around.
- The system ships no companion package; an administrator uploads them.

## Related

- [Goal Mode](/docs/en/concepts/goal-mode/) — the goal-continuation round that the companion's "running" animation corresponds to
- [Bridge Panel Guide](/docs/en/reference/admin-panels/) — working with the companion management panel
- [Agent Loop](/docs/en/concepts/core-agent-loop/) — where the runtime state really comes from
