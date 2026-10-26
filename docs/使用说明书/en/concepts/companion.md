---
title: Companion System
summary: ZIP package and sprite-sheet specification, per-form presentation, scale and position ranges, and user versus administrator permissions.
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
updated_at: 2026-10-16
---

A companion package is a ZIP holding a manifest and a sprite sheet; at runtime the row played depends on the session state. Companions take no part in execution and do not affect scheduling, so deleting one changes no functionality.

## Package Structure

A companion package is a ZIP that holds exactly two things: a manifest and a sprite-sheet image.

- The manifest (`pet.json`) is JSON and states the companion's id, display name, and description, along with where the sprite-sheet file lives.
- The sprite sheet is a single image whose path the manifest gives.
- Size limits: the whole ZIP is at most 24 MB, and the single sprite-sheet file is at most 18 MB.
- Import guards against path traversal inside the archive.

Companion files live in one fixed companion directory (`config/data/companions/global`, overridable through `WUNDER_COMPANIONS_ROOT`); embedded forms such as the Pupa and the Helm pin this directory to their own data directory. The Pupa bundles two sample companion packages and releases them into its local library on first launch; the Beehive and the Bridge ship none, so an empty list there is normal.

## Sprite-Sheet Specification

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

## Presentation per Form

| Form | Presentation |
| --- | --- |
| Pupa (Windows) | Native floating window: a Win32 layered window for the selected companion, with dragging and the frame clock on a separate message thread that does not occupy the UI thread |
| Pupa (Linux) | No equivalent floating window; the desktop pet exists only on Windows |
| Beehive | An in-page overlay that follows the chat state |
| Bridge | A companion management panel: upload, edit, delete, and export global companions |
| Helm | No companion UI; it only points the companion directory at the local data directory |

## Scale and Position

| Item | Range |
| --- | --- |
| Stored / settings-slider scale | 0.1–1.6 (step 0.1, default 0.5) |
| Render scale | 0.1–1.8 |
| Position | Clamped to the screen work area with margins kept; drag results are saved |

The Pupa stores the globally selected companion, the show switch, the scale, the message-hint switch and the position in a local file; the Beehive stores them in the browser, so clearing browser data resets position and preferences.

## Permissions

- The user side is read-only: four GET endpoints only — the global list, one companion's detail, its sprite sheet, and its exported package (`/wunder/companions/global*`).
- Adding, changing, and deleting sit on the administrator side (`/wunder/admin/companions*`): list, import, edit and delete, sprite sheet, export package. The import request-body limit matches the ZIP limit, both 24 MB.
- A "private companion" in the Beehive takes effect only in the current browser and is not uploaded to the server; a global companion is uploaded by an administrator through the Bridge and is then visible to all users. The Bridge is the only upload path.

## Limits

- A companion is not another kind of agent; it is only the visual shell of an agent, and one agent binds one companion.
- Users cannot upload a companion to the server from the Beehive; server-side companions are managed by administrators through the Bridge, and private imports stay in the local browser.
- The Pupa on Linux has no desktop floating window — the overlay is a Win32 implementation with no Linux equivalent.
- Companion animation does not affect task execution: the runtime state drives the animation, not the other way around.
- The Pupa ships sample companion packages; the Beehive and the Bridge's global library ship none, so an administrator uploads them.
