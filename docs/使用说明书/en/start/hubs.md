---
title: Manual Overview
summary: The manual's navigation page: start from the Pupa and move into the interface map, other forms, and reference as needed. Every page in the start and surfaces groups is here.
read_when:
  - You want the map of the whole manual and where to enter it
  - You are troubleshooting and need to jump to the right form's page
source_docs:
  - docs/使用说明书/site.json
  - frontend-slint/run.bat
  - crates/wunder-server/src/main.rs
updated_at: 2026-10-13
---

# Manual Overview

Everyone starts with the [Pupa (desktop)](/docs/en/start/desktop/): download and install it, and opening it gives you your local AI workbench. The entries below are ordered by how often you'll need them.

## Getting started

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/start/quickstart/">
    <strong>Quick Start</strong>
    <span>Get your first task running in 4 steps.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/desktop/">
    <strong>Pupa intro</strong>
    <span>Download, install, open, go; data stays on your machine, no port taken.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/troubleshooting-first-run/">
    <strong>First-run troubleshooting</strong>
    <span>No interface after install / port unreachable / model 401.</span>
  </a>
</div>

## Interface map

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/surfaces/index/">
    <strong>Surfaces overview</strong>
    <span>The four forms' interface division and capability matrix.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/desktop-ui/">
    <strong>Pupa interface</strong>
    <span>The main window's two pages + 12 settings categories + panels and dialogs.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/frontend/">
    <strong>Beehive interface</strong>
    <span>The single-page app in the browser, with two-way voice.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/web-admin/">
    <strong>Bridge interface</strong>
    <span>The admin's governance and debugging backend.</span>
  </a>
</div>

## Going deeper

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/concepts/">
    <strong>Core overview</strong>
    <span>Understand the system's running model.</span>
  </a>
  <a class="docs-card" href="/docs/en/tools/">
    <strong>Tools overview</strong>
    <span>Pick tools by task.</span>
  </a>
</div>

## Other forms (optional)

Individual users need only the Pupa by default; the following are needed only for team collaboration or automation:

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/start/server/">
    <strong>Hull deployment</strong>
    <span>18000 serves the API and the Bridge; 18001 delivers the Beehive; sandbox on 9001.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/cli/">
    <strong>Helm usage</strong>
    <span>An interactive session in the terminal; `exec --json` event stream; the full `cloud` set.</span>
  </a>
</div>

## Troubleshooting entry

- [First-run troubleshooting](/docs/en/start/troubleshooting-first-run/): locate install / launch / access failures by form
- [Help center](/docs/en/help/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
- [FAQ](/docs/en/help/faq/)
- [Configuration reference](/docs/en/reference/config/)

## Documentation ecosystem

- [Documentation directory](/docs/en/start/docs-directory/): how the manual / design docs / technical manual / API reference / performance standards and scoring / changelog / lessons-learned divide the work and how to read them
- [API index](/docs/en/reference/api-index/)

## Home

Back to the [wunder home page](/docs/en/index/).
