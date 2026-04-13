---
title: Reference Overview
summary: Reference pages provide stable lookup content and also collect runtime model reference pages broken down by topic, suitable for diving into details after reviewing the core overview.
read_when:
  - You need field-level, event-level, or configuration-level reference
  - You already know which layer the problem is in and want to quickly check the standard answer
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# Reference Overview

This set of pages is oriented toward "lookup" and "implementation". The core pages are responsible for establishing the main framework, while here we focus on explaining fields, events, configurations, and decomposed runtime models.

## Reference Entries

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/reference/api-index/"><strong>API Index</strong><span>Quickly locate endpoints by interface domain.</span></a>
  <a class="docs-card" href="/docs/en/reference/config/"><strong>Configuration</strong><span>Runtime configuration, models, storage, security items.</span></a>
  <a class="docs-card" href="/docs/en/reference/stream-events/"><strong>Stream Events Reference</strong><span>Event semantics and terminal state determination.</span></a>
  <a class="docs-card" href="/docs/en/reference/workspace-routing/"><strong>Workspace Routing</strong><span>`user_id/container_id/agent_id` routing rules.</span></a>
  <a class="docs-card" href="/docs/en/reference/prompt-templates/"><strong>Prompt Templates</strong><span>Template segments and activation timing.</span></a>
  <a class="docs-card" href="/docs/en/reference/admin-panels/"><strong>Admin Panel Index</strong><span>Mapping from features to panels.</span></a>
</div>

## Runtime Model Reference

This set of pages comes from the original concepts section, suitable for diving into by topic after reviewing [Core Overview](/docs/en/concepts/).

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/en/concepts/architecture/"><strong>System Architecture</strong><span>Layering, module boundaries, and core pipelines.</span></a>
  <a class="docs-card" href="/docs/en/concepts/workspaces/"><strong>Workspaces and Containers</strong><span>File isolation, routing, and container semantics.</span></a>
  <a class="docs-card" href="/docs/en/concepts/sessions-and-rounds/"><strong>Sessions and Rounds</strong><span>User rounds, model rounds, and thread relationships.</span></a>
  <a class="docs-card" href="/docs/en/concepts/streaming/"><strong>Streaming Execution</strong><span>Event streams, terminal states, and recovery mechanisms.</span></a>
  <a class="docs-card" href="/docs/en/concepts/presence-and-runtime/"><strong>Runtime and Presence</strong><span>Busy/idle status, online presence, and runtime states.</span></a>
  <a class="docs-card" href="/docs/en/concepts/tools/"><strong>Tool System</strong><span>Unified view of built-in tools, MCP, and Skills.</span></a>
  <a class="docs-card" href="/docs/en/concepts/prompt-and-skills/"><strong>Prompts and Skills</strong><span>Thread freezing, skill mounting, and activation boundaries.</span></a>
  <a class="docs-card" href="/docs/en/concepts/memory/"><strong>Long-term Memory</strong><span>Memory injection, extraction, and recall constraints.</span></a>
  <a class="docs-card" href="/docs/en/concepts/quota-and-token-usage/"><strong>Token Accounts and Usage</strong><span>Balance, consumption, and context load differences.</span></a>
  <a class="docs-card" href="/docs/en/concepts/swarm/"><strong>Swarm Collaboration</strong><span>Queen bees, worker bees, and result merging.</span></a>
  <a class="docs-card" href="/docs/en/concepts/boundary-handling/"><strong>Boundary Handling</strong><span>Limit exceeded, failures, and recovery paths.</span></a>
</div>

## When to Prioritize Reading Reference Pages

- You need to confirm fields and parameters, not understand the product background.
- You are doing integration testing and need stable specifications.
- You are troubleshooting and need precise events and configuration basis.

## Common Misconceptions

- Reference pages do not replace core pages; they have different responsibilities.
- For streaming state machines, don't just look at `final`; consider event semantics together.
- Workspace routing and prompt templates should be verified separately; do not infer from experience.

## Further Reading

- [Core Overview](/docs/en/concepts/)
- [Integration Overview](/docs/en/integration/)
- [Help Center](/docs/en/help/)