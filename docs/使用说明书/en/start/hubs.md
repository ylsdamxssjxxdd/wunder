---
title: Manual Overview
summary: The manual's navigation page: start from the Desktop and move into the interfaces, concepts, and other forms by section. Every page in the start and surfaces groups is here.
read_when:
  - You want the map of the whole manual and where to enter it
  - You are troubleshooting and need to jump to the right form's page
source_docs:
  - docs/使用说明书/site.json
  - crates/wunder-desktop/frontend/run.bat
  - crates/wunder-server/src/main.rs
updated_at: 2026-10-16
---

# Manual Overview

The starting point is the [Desktop](/docs/en/start/desktop/): download the installer and it runs locally, with no Server to deploy first.

## Getting started

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/start/quickstart/">
    <strong>Quick Start</strong>
    <span>Get your first task running in 4 steps.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/desktop/">
    <strong>Desktop intro</strong>
    <span>Download, install, open, go; data stays on your machine, no port taken.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/troubleshooting-first-run/">
    <strong>First-run troubleshooting</strong>
    <span>No interface after install / port unreachable / model 401.</span>
  </a>
</div>

## Interfaces

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/surfaces/index/">
    <strong>Surfaces overview</strong>
    <span>The forms' interface division and capability matrix.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/desktop-ui/">
    <strong>Desktop interface</strong>
    <span>The main window's two pages + 11 settings categories + panels and dialogs.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/frontend/">
    <strong>Cloud interface</strong>
    <span>The single-page app in the browser, with two-way voice.</span>
  </a>
  <a class="docs-card" href="/docs/en/surfaces/web-admin/">
    <strong>Admin interface</strong>
    <span>The admin's governance and debugging backend.</span>
  </a>
</div>

## Concepts and tools

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

## Optional forms

Individual users need only the Desktop; the following are needed only for team collaboration or automation:

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/start/server/">
    <strong>Server deployment</strong>
    <span>18000 serves the API and the Admin; 18001 delivers the Cloud; sandbox on 9001.</span>
  </a>
  <a class="docs-card" href="/docs/en/start/cli/">
    <strong>CLI usage</strong>
    <span>An interactive session in the terminal; `exec --json` event stream; the full `cloud` set.</span>
  </a>
</div>

## Troubleshooting and documentation

- [First-run troubleshooting](/docs/en/start/troubleshooting-first-run/): locate install / launch / access failures by form
- [Help center](/docs/en/help/)
- [Troubleshooting](/docs/en/help/troubleshooting/)
- [FAQ](/docs/en/help/faq/)
- [Configuration reference](/docs/en/capabilities/config/)
- [Documentation directory](/docs/en/start/docs-directory/): how the manual / design docs / technical manual / API reference / performance baselines / changelog / lessons-learned divide the work and how to read them
- [API index](/docs/en/capabilities/api-index/)
