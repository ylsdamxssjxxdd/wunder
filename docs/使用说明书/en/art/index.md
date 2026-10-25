---
title: Art Sheets
summary: Ten interface design sheets for the Pupa desktop app (overview, foundations, icons, shell, timeline, process and tools, plan and inquiry, composer, settings and dialogs, states and motion), plus six for the Helm terminal app (overview, foundations, layout and footer, timeline, tools and plan, input and popups). Each sheet is annotated against the current implementation and can be checked directly.
updated_at: 2026-10-25
---

# Art Sheets

These sheets are the interface baseline for the two local forms: the Pupa desktop app first as the main line, then the Helm terminal app. Every sheet carries its own sizes, colors, states and rules; when the code and a sheet disagree, check the running app first and then decide which side to change.

- Sources live in `docs/wunder美术设计稿`, split by form into the `蛹/` and `舵机/` folders; every sheet is a standalone SVG, and the 蛹 sheets embed the app icon so they still render when copied elsewhere
- The page shows the SVGs directly and stays sharp at any zoom; the ten 蛹 sheets also have a [design index](/docs/art/) that browses them as thumbnails
- The Pupa help manual embeds 1200px raster previews of the 蛹 sheets, so the Slint renderer needs no SVG pipeline; the 舵机 sheets target the terminal implementation and stay out of the in-app manual

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

## Helm 00 Overview

The full terminal frame and its five-band vertical stack: transcript, popup band, welcome banner band, activity line and composer, plus the entry modes. The Helm has no window pixels — every layout is budgeted in terminal rows and columns, and the 5-row composer is never squeezed.

![Helm overview design sheet](/docs/art/servo/00-整体总览.svg)

*Helm 00 · full frame (100×30 example) and layout stack*

## Helm 01 Foundations

The semantic system of named colors plus weight modifiers, truecolor surfaces, NO_COLOR degradation, text-width rules, layout constants and the terminal capability ladder. Facts come from `crates/wunder-cli/tui/theme.rs`.

![Helm foundations design sheet](/docs/art/servo/01-基础规范.svg)

*Helm 01 · semantic colors, truecolor surfaces, degradation, baseline*

## Helm 02 Layout and Footer

Constraint order of the layout stack, a running frame, welcome-banner retirement rules, the activity-line state table, and the footer's composition and width-based shedding order.

![Helm layout design sheet](/docs/art/servo/02-布局与底栏.svg)

*Helm 02 · layout stack, activity line, footer*

## Helm 03 Timeline

Entry styles, the Markdown baseline, streaming rules, code and diff colors, the Ctrl+T expanded view, selection and search.

![Helm timeline design sheet](/docs/art/servo/03-会话时间线.svg)

*Helm 03 · entry styles, diff cards, expanded view*

## Helm 04 Tools and Plan

Tool lines and patch cards, plan-card states and ordering, trajectory view tabs, and the status-mark table. Process information is always secondary text; only status marks are allowed to speak up.

![Helm tools design sheet](/docs/art/servo/04-工具与计划.svg)

*Helm 04 · tool lines, plan card, trajectory view*

## Helm 05 Input and Popups

The composer, the anchored popup band, the four centered modals, the thread center, motion and frame budget, and the brand mark.

![Helm input design sheet](/docs/art/servo/05-输入与弹层.svg)

*Helm 05 · composer, modals, thread center*

## Maintenance

After editing an SVG, run once from the repository root:

```bash
python scripts/sync_art.py
```

It does three things in order:

1. Renders the 蛹 SVGs to 1200px PNGs under `frontend-slint/assets/help/art` for the desktop help
2. Rebuilds the Chinese and English manual JSON from those PNGs
3. Rebuilds the docs site and copies the SVGs to `web/docs/art` (蛹) and `web/docs/art/servo` (舵机), embedding the app icon as a data URI

The web side takes effect after the site rebuild; the desktop app must be rebuilt because the previews are compiled into the binary with `include_bytes!`.

When adding or removing a 蛹 sheet, update three places besides the artwork: `SHEETS` in `scripts/render_art_previews.py`, `ART_TOPICS` in `scripts/build_help_manual.py`, and the gallery at `docs/wunder美术设计稿/蛹/index.html`; add the section on this page too, then run the sync script. Do not edit the PNGs under `frontend-slint/assets/help/art` by hand.

For a 舵机 sheet, sync this page's section and the file table in `docs/wunder美术设计稿/舵机/README.md`; no in-app assets are involved.
