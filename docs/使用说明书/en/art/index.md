---
title: Art Sheets
summary: Ten implementation-based interface sheets for the Pupa desktop app and six for the Helm terminal app, organized around real UI, functional structure, control states and constraints, with drag-to-pan and zoom in the browser.
updated_at: 2026-10-25
---

# Art Sheets

These sheets focus on Pupa, the local AI workbench, while also covering Helm, the manual terminal entry. Code and actual rendering are the sources of truth; SVG is used for design changes, refactoring and acceptance checks. When a sheet and the implementation disagree, check the running UI instead of treating the sketch as reality.

- Open the [unified art gallery](/docs/art/) for all 16 sheets. Sources live in `docs/wunder美术设计稿`, split into `蛹/` and `舵机/`.
- Drag with the primary mouse button or one finger. Fit width, fit window, 100%, zoom and previous/next navigation are available. Arrow keys pan, `+/-` zoom, `0` resets and Esc closes.
- Large images embedded on this page also pan directly. The normal wheel scrolls the document; Ctrl/Command + wheel zooms the image. Raw SVG links remain available without JavaScript.
- Every sheet follows “real UI → functional structure → state presentation → dimensions and behavior”. Control interaction states and task lifecycle states are separate, and proposals are never drawn as current behavior.
- The Pupa help manual embeds raster previews so no SVG renderer is needed. Helm sheets are not included in that manual.

## 00 Overview

The whole-window baseline is 1462×866 logical pixels, with a 240 sidebar, a 1000 reading column and a 154 composer band. It separates navigation, conversation, execution, input and system feedback, and distinguishes a fresh thread from completed history with an idle composer.

![Overview design sheet](/docs/art/00-整体总览.svg)

*00 Overview · real window, functional zones and state distinctions*

## 01 Foundations

Neutral, accent and semantic colors, type, spacing, radii and window metrics. Tokens map to `frontend-slint/ui/design_tokens.slint`; document annotations and product UI use independent scales.

![Foundations design sheet](/docs/art/01-基础规范.svg)

*01 Foundations · color, type, spacing, radii and metrics*

## 02 Icons

Thirty production icons grouped by navigation, conversation actions, input and status. Solid Font Awesome assets, repository line SVGs and code-drawn status nodes remain distinct; this is not a fictional unified line-icon redraw.

![Icons design sheet](/docs/art/02-图标系统.svg)

*02 Icons · production assets, sizes, color and states*

## 03 Shell and Sidebar

Title bar, sidebar, workspace/thread rows, working directory and splitter. Thread hover actions are rename, trajectory and archive, in that order. Row interactions, multi-select and task lifecycle appear in separate matrices.

![Shell and sidebar design sheet](/docs/art/03-外壳与侧栏.svg)

*03 Shell and sidebar · window, navigation, row states and lifecycle*

## 04 Timeline

One complete turn shows user input, process, assistant answer and footer in reading order. Markdown, status capsules, empty/loading/error states and follow scrolling are explained separately.

![Timeline design sheet](/docs/art/04-会话时间线.svg)

*04 Timeline · complete turns, content blocks, states and scrolling*

## 05 Process and Tools

Process bars, tool rows, expanded output, tool groups, subagents and patch cards. Running/completed/failed execution states are separate from expanded/collapsed view state, using real 12px nodes and a 26px row rhythm.

![Process and tools design sheet](/docs/art/05-过程与工具.svg)

*05 Process and tools · execution state, view state and details*

## 06 Plan and Inquiry

Plan pill, explanation and steps, goal banner and inquiry panel. The expanded plan has no extra count header. Goal actions are edit, pause/resume and clear. Inquiry answers are submitted through Composer.

![Plan and inquiry design sheet](/docs/art/06-计划与问询.svg)

*06 Plan and inquiry · steps, goals, options and quick answers*

## 07 Composer

A 972×100 editor card and a 30-high action row; attachments, branches and queued turns add height as needed. Send/stop is derived from draft, attachments, inquiry readiness, busy, generating and stopping, with recording, transcription, queue and model menus shown separately.

![Composer design sheet](/docs/art/07-输入区.svg)

*07 Composer · editor, actions, send/stop and popovers*

## 08 Settings and Dialogs

A real 1080×720 settings window with a 44 header, 260 navigation and 60 navigation items. General settings contain send key, language, four theme previews and font size. Controls, validation, Confirm/Modal/Info and Toast are grouped by purpose.

![Settings and dialogs design sheet](/docs/art/08-设置与弹窗.svg)

*08 Settings and dialogs · actual settings, controls and feedback*

## 09 States and Motion

State matrices come from real `Action`, `FormField`, `ThemeCheck` and system `Switch` behavior. Invented pressed colors, universal loading states and focus rings are not used to fill cells. Motion, keyboard, accessibility, responsive behavior and performance limits follow.

![States and motion design sheet](/docs/art/09-交互状态与动效.svg)

*09 States and motion · actual controls, keys, feedback and degradation*

## Helm 00 Overview

The startup frame and five-band stack: transcript, popup, welcome banner, activity and composer. Startup text belongs to the transcript. The welcome pixels come from current generated data rather than a placeholder mark.

![Helm overview design sheet](/docs/art/servo/00-整体总览.svg)

*Helm 00 · startup frame, welcome pixels and layout contract*

## Helm 01 Foundations

Named colors, modifiers, truecolor surfaces, NO_COLOR, display columns and terminal capabilities. Ordinary popups and centered pickers keep their respective selection colors; the running indicator is documented as a local implementation.

![Helm foundations design sheet](/docs/art/servo/01-基础规范.svg)

*Helm 01 · semantic colors, surfaces, degradation and width*

## Helm 02 Layout and Footer

Five-band layout, a running frame, activity and footer shedding. The composer stays five rows high externally, while cloud errors, warnings and gap content can reduce the draft to one internally scrolling row.

![Helm layout and footer design sheet](/docs/art/servo/02-布局与底栏.svg)

*Helm 02 · layout stack, activity, input budget and footer shedding*

## Helm 03 Timeline

Entry styles, Markdown, streaming, code and diff, the Ctrl+T expanded view, selection and search. Process information stays dim while content and action-required notices retain distinct weight.

![Helm timeline design sheet](/docs/art/servo/03-会话时间线.svg)

*Helm 03 · entries, diffs, expanded view and selection*

## Helm 04 Tools and Plan

Tool lines, patches, the plan card, trajectory tabs and status marks. The plan includes an “Updated Plan · n/m complete” header, shows at most three folded steps, and orders in-progress, pending and completed steps accordingly.

![Helm tools and plan design sheet](/docs/art/servo/04-工具与计划.svg)

*Helm 04 · tools, plan header, trajectory and status marks*

## Helm 05 Input and Popups

The composer, its anchored popup band, centered pickers and the full-screen thread center. Resume, approval and inquiry use centered choices; shortcuts and warnings use information panels. Containers do not share position or selection styling by assumption.

![Helm input and popups design sheet](/docs/art/servo/05-输入与弹层.svg)

*Helm 05 · input, popups, centered choices and thread center*

## Maintenance

After editing an SVG, run once from the repository root:

```bash
python scripts/sync_art.py
```

It performs three steps:

1. Renders the Pupa SVGs into 1200px PNGs under `frontend-slint/assets/help/art` for desktop help.
2. Rebuilds the Chinese and English manual JSON from those images.
3. Rebuilds the docs site, publishing SVGs to `web/docs/art` (Pupa) and `web/docs/art/servo` (Helm), copying the unified gallery and viewer assets, and embedding the app icon as a data URI in published SVGs.

The web side updates immediately after rebuilding. The desktop app must be recompiled because its previews are embedded with `include_bytes!`.

When adding or removing a Pupa sheet, update `SHEETS` in `scripts/render_art_previews.py`, `ART_TOPICS` in `scripts/build_help_manual.py`, the sheet data in `docs/wunder美术设计稿/index.html`, and this page. Do not edit generated PNGs manually.

For Helm changes, update the unified gallery data, this page and `docs/wunder美术设计稿/舵机/README.md`; no in-app assets are involved.

The shared viewer lives in `docs/wunder美术设计稿/assets/art-viewer.js` and `art-viewer.css`. It loads only on the art documentation page, and old instances are destroyed during dynamic navigation. Every sheet should also pass XML/reference checks and Edge rendering at its original width.
