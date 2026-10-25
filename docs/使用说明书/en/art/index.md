---
title: Art Sheets
summary: Ten interface design sheets for the Pupa desktop app, covering the overview, foundations, icons, shell, timeline, process and tools, plan and inquiry, composer, settings and dialogs, and interaction states. Each sheet is annotated in logical pixels and can be checked against the implementation directly.
updated_at: 2026-10-25
---

# Art Sheets

These sheets are the interface baseline for the Pupa desktop app. Every sheet carries its own sizes, spacing, colors and states; when the code and a sheet disagree, check the running app first and then decide which side to change.

- Sources live in `docs/wunder美术设计稿`; every sheet is a standalone SVG with the app icon embedded, so it still renders when copied elsewhere
- The page shows the SVGs directly and stays sharp at any zoom; you can also open the [design index](/docs/art/) and browse all ten sheets as thumbnails
- The Pupa help manual embeds 1200px raster previews of the same sheets, so the Slint renderer needs no SVG pipeline

## 00 Overview

Whole-window baseline: 1462×866 logical pixels, a 240 sidebar, a 1000 reading column and a 154-high composer band. The sidebar, timeline and composer share one sheet so the layout can be compared at a glance.

![Overview design sheet](/docs/art/00-整体总览.svg)

*00 Overview · window 1462×866 (reference screenshot 2558×1515 at 175% DPI)*

## 01 Foundations

Neutral, accent and semantic colors, the type scale, spacing, radii and the window layout baseline. The tokens map one-to-one to `frontend-slint/ui/design_tokens.slint`.

![Foundations design sheet](/docs/art/01-基础规范.svg)

*01 Foundations · color, type, spacing, radii*

## 02 Icons

Rules for redrawing the brand mark, tool icons, state icons and line icons. The app icon is the pupa; the bee mark stays only as an auxiliary channel and collaboration identifier.

![Icons design sheet](/docs/art/02-图标系统.svg)

*02 Icons · brand, tools, states and line icons*

## 03 Shell and Sidebar

Title bar, sidebar width, groups, workspace rows, thread rows, the working-directory toolbar, and the splitter in its rest, hover and drag states. The sidebar splitter shows no rail at rest and a 2×32 pill on hover.

![Shell and sidebar design sheet](/docs/art/03-外壳与侧栏.svg)

*03 Shell and sidebar · title bar, sidebar, splitter*

## 04 Timeline

Reading column and scrolling, user messages, turn footers, the run-state capsule, Markdown fallback styles and the empty state. Streaming updates only the active tail block; finished blocks stay stable.

![Timeline design sheet](/docs/art/04-会话时间线.svg)

*04 Timeline · messages, footers, state capsule*

## 05 Process and Tools

Process bar, thinking entries, tool rows, the expanded output panel, tool groups and subagent cards. The sheet covers icon sizes, fold budgets and indentation for every state.

![Process and tools design sheet](/docs/art/05-过程与工具.svg)

*05 Process and tools · process bar, tool rows, subagents*

## 06 Plan and Inquiry

Plan capsule, expanded card, goal banner and inquiry panel. Option rows include "no preference" and "other", and the inquiry panel has no separate submit button.

![Plan and inquiry design sheet](/docs/art/06-计划与问询.svg)

*06 Plan and inquiry · plan card, goal banner, inquiry panel*

## 07 Composer

The 972×100 input card, the 30-high footer, attachment rows, recording, generating and disabled-send states, plus queue and model menu specs. The stop square is 10.5 and the send arrow is 16.

![Composer design sheet](/docs/art/07-输入区.svg)

*07 Composer · input card, footer, stop and queue*

## 08 Settings and Dialogs

Settings row anatomy, input and select controls, buttons, validation and hints, and the density and layering of dialogs.

![Settings and dialogs design sheet](/docs/art/08-设置与弹窗.svg)

*08 Settings and dialogs · rows, controls, validation*

## 09 States and Motion

Row and input states, the motion timeline, performance limits, key bindings, accessibility feedback and breakpoints. This sheet also records which actions must not block during long lists and streaming output.

![States and motion design sheet](/docs/art/09-交互状态与动效.svg)

*09 States and motion · states, timing, keys*

## Maintenance

After editing an SVG, run once from the repository root:

```bash
python scripts/sync_art.py
```

It does three things in order:

1. Renders the SVGs to 1200px PNGs under `frontend-slint/assets/help/art` for the desktop help
2. Rebuilds the Chinese and English manual JSON from those PNGs
3. Rebuilds the docs site and copies the SVGs to `web/docs/art`, embedding the app icon as a data URI

The web side takes effect after the site rebuild; the desktop app must be rebuilt because the previews are compiled into the binary with `include_bytes!`.

When adding or removing a sheet, update three places besides the artwork: `SHEETS` in `scripts/render_art_previews.py`, `ART_TOPICS` in `scripts/build_help_manual.py`, and the gallery at `docs/wunder美术设计稿/index.html`; add the section on this page too, then run the sync script. Do not edit the PNGs under `frontend-slint/assets/help/art` by hand.
