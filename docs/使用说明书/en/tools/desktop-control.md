---
title: Desktop Control
summary: In desktop mode, the Desktop Controller and Desktop Monitor drive the local mouse and keyboard, capture screenshots, and feed the picture to the model as appended visual input.
updated_at: 2026-10-13
---

| Canonical name | Aliases |
| --- | --- |
| `桌面控制器` | `desktop_controller`, `controller` |
| `桌面监视器` | `desktop_monitor`, `monitor` |

## When to use it

- Only when a target has a **graphical interface as its sole entry point**, with no command line or interface available, use `桌面控制器` to click and type.
- To see the desktop's current state, or to confirm whether the previous action took effect, use `桌面监视器`.
- If commands, files, or browser tools can solve it, do not go through desktop control: it is slower, more error-prone, and it directly drives the user's real screen.

## Prerequisites

All three must hold:

1. The installed build was compiled with desktop control enabled (it is off by default).
2. You are in desktop mode and have explicitly turned the desktop control switch on (it is off by default).
3. The current model supports vision; otherwise desktop control, the desktop monitor, and the image-reading tool are all filtered out.

Across the forms: the Pupa (desktop mode) lists these three tools in its extra-enabled set by default; under the Hull, Beehive, and Helm forms the desktop tools are unavailable.

## How to use it

### Desktop Controller

One call needs both a **position** and an **action**:

- Position uses normalized coordinates: either an `[x1,y1,x2,y2]` tuple or a two-value center point. Coordinates are based on a default `1000×1000` normalized grid; at run time the center is taken and mapped linearly onto real screen pixels.
- There are 17 actions: `left_click`, `left_double_click`, `right_click`, `middle_click`; `left_hold` / `right_hold` / `middle_hold` and their matching `left_release` / `right_release` / `middle_release`; `scroll_down` / `scroll_up`; `press_key`, `type_text`, `delay`, `move_mouse`, `drag_drop`.
- Related arguments: a key uses `key`, typed text uses `text`, a delay uses `delay_ms`, drag duration uses `duration_ms` (about 400 ms when omitted), scroll steps use `scroll_steps`, and a drag destination uses `to_bbox`.

Loose aliases are accepted too: `keyboard`→`press_key`, `send_text`→`type_text`, `sleep`→`delay`, `move`→`move_mouse`, `drag`→`drag_drop`, and hyphens and spaces are normalized automatically. An unknown action reports "unknown action".

The action description is only used for the on-screen hint, and defaults to the action name itself.

### Desktop Monitor

Give a wait duration (0–30000 ms; anything larger is clamped to 30000), optionally with a one-line observation note that is echoed back verbatim. The wait argument also accepts forms such as `wait` and `delay_ms`.

## What it does automatically

- **It takes a screenshot after every action.** The controller does not return the screen state from before the action; the outcome is delivered by the visual input that follows.
- After a successful return, the orchestrator assembles the screenshots into a **user-role multimodal message** appended to the model: a text hint plus the screenshot from before the action (if any) and the current screenshot. In other words, the tool result the model sees contains no image itself; the picture arrives through that appended message. Budget a task as "one action = one call + one look". A single screenshot entering context is capped at 8 MiB, and exceeding that fails.
- Hints appear on screen: before an action runs, a red dot on the real screen says "AI is clicking here" and disappears when done; the monitor shows a countdown hint.

## Diagnosing problems

- Desktop control not compiled in: the call fails outright and says the capability is not enabled.
- Not in desktop mode, or the switch is off: it returns "desktop control is disabled".
- Missing arguments: position, action, and wait duration are all required and it reports whichever is missing; a malformed position reports an illegal coordinate.
- Screenshot failure: capture times out after 5000 ms by default (100 ms minimum), and failure reports an error upward.

## Boundaries

- **Actions are real input injection with no rollback.** `left_hold` and `left_release` must be used in pairs, or the mouse state may be left dangling; `drag_drop` is press, smooth move, release.
- Coordinates change with resolution; normalized coordinates only guarantee consistency between one observation and the action that follows it, so observe again after switching screens or changing resolution.
- A screenshot is also saved to the user's container directory; a persistence failure only logs a warning and does not affect the result.
- The screenshot directory keeps frames by count, holding only the most recent few (3 by default), so temporary files do not pile up without bound.
- Authorization follows least privilege: it needs an explicit user switch, it does not silently capture the screen in the background, and it does not expose system capabilities to a remote party.

## Related pages

- Reading local images / video frames: [Read Image](/docs/en/tools/read-image/)
- Interaction inside a webpage: [Browser Tools](/docs/en/tools/browser/)
