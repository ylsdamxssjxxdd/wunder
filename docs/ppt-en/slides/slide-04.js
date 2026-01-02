"use strict";

import { createSlide } from "./utils.js";

// 第 4 页：工具体系总览，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Tool system">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <span class="section-chip active">Overview</span>
            <span class="section-chip">Built-in</span>
            <span class="section-chip">MCP</span>
            <span class="section-chip">Skills</span>
            <span class="section-chip">Knowledge</span>
            <span class="section-chip">Custom</span>
            <span class="section-chip">Shared</span>
          </div>
        </div>
        <h2>Six tool types form the capability map</h2>
        <p class="section-lead">Decomposition enables governance, reuse, and sharing</p>
        <div class="grid three">
          <div class="card">
            <h3>Built-in tools</h3>
            <p>File, command, and basic actions</p>
          </div>
          <div class="card">
            <h3>MCP tools</h3>
            <p>Connect external systems and platforms</p>
          </div>
          <div class="card">
            <h3>Skills tools</h3>
            <p>Turn experience into standard workflows</p>
          </div>
          <div class="card">
            <h3>Knowledge tools</h3>
            <p>Documents stay searchable and traceable</p>
          </div>
          <div class="card">
            <h3>Custom tools</h3>
            <p>Personal capability packs</p>
          </div>
          <div class="card">
            <h3>Shared tools</h3>
            <p>Team capability pool</p>
          </div>
        </div>
        <div class="note">
          <strong>Unified governance:</strong> shared catalog, allowlist control, composable usage
        </div>
      </section>
  `);
}
