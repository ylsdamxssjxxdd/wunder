---
title: Capabilities Overview
summary: The ten pages in this partition, plus where each of the nine core capabilities is documented under concepts.
updated_at: 2026-10-16
---

The ten pages in this partition are organized by entry point: capability entries, session status, prompts and personalization, system settings, workspace routing, Bridge panels, the WeChat iLink channel, the chat WebSocket, the MCP endpoint, and interlink.

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/capabilities/api-index/"><strong>Feature and Capability Quick Reference</strong><span>What wunder can do, and where each capability lives.</span></a>
  <a class="docs-card" href="/docs/en/capabilities/stream-events/"><strong>Session Status Guide</strong><span>What the various statuses during a chat mean.</span></a>
  <a class="docs-card" href="/docs/en/capabilities/prompt-templates/"><strong>Prompts and Personalization</strong><span>How to customize the agent's answer style.</span></a>
  <a class="docs-card" href="/docs/en/capabilities/config/"><strong>System Settings Reference</strong><span>What the common settings mean and what to set them to.</span></a>
  <a class="docs-card" href="/docs/en/capabilities/workspace-routing/"><strong>Workspace Routing and File Management</strong><span>The single cloud directory, path conventions, and management tips.</span></a>
  <a class="docs-card" href="/docs/en/capabilities/admin-panels/"><strong>Bridge Panel Guide</strong><span>What each admin panel is for.</span></a>
  <a class="docs-card" href="/docs/en/capabilities/weixin-channel/"><strong>WeChat iLink Channel</strong><span>Configuring the `weixin` channel, and how it differs from the legacy WeChat channel.</span></a>
  <a class="docs-card" href="/docs/en/capabilities/chat-ws/"><strong>Chat WebSocket</strong><span>The realtime channel is also the session-control channel: start, resume, watch, cancel, and returning approvals all ride on the same connection.</span></a>
  <a class="docs-card" href="/docs/en/capabilities/mcp-endpoint/"><strong>MCP Endpoint</strong><span>Keep the two directions apart: exposing wunder as an MCP service, or connecting to an external MCP service.</span></a>
  <a class="docs-card" href="/docs/en/capabilities/interlink/"><strong>Interlink</strong><span>Unified presence across the Beehive, the Pupa, and the Helm, cross-end workspace visibility, and two-way remote control.</span></a>
</div>

## Core capabilities

The nine core capabilities are documented in the concepts partition:

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/concepts/core-goal/"><strong>Goal</strong><span>Lets an agent enter goal mode and keep working on one task.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-compatibility/"><strong>Compatibility</strong><span>Lets the Pupa and the Helm run in older operating-system environments under explicit constraints.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-context-compression/"><strong>Context compression</strong><span>Keeps context size under control in long conversations while retaining useful information.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-subagent/"><strong>Subagents</strong><span>Forks a subtask inside a single thread; an independent executor completes it and returns the result.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-scheduled-tasks/"><strong>Scheduled tasks</strong><span>Supports system-level periodic execution and background governance.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-channels/"><strong>Channels</strong><span>Connects multiple entries to the same runtime capability.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-memory/"><strong>Memory</strong><span>Supports long conversations and long-term material use without polluting the thread's core cognition.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-multithreading/"><strong>Multi-threading</strong><span>Raises throughput and resource use through concurrency control inside a single process.</span></a>
  <a class="docs-card" href="/docs/en/concepts/core-multi-user-management/"><strong>Multi-tenancy</strong><span>Supports the Hull's organization, user, tenant, and permission governance.</span></a>
</div>
