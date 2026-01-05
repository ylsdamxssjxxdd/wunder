"use strict";

import { createSlide } from "./utils.js";

// 第 4 页：工具体系总览，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Tool system">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <a class="section-chip active" href="#4">Overview</a>
            <a class="section-chip" href="#5">Built-in</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip" href="#7">Skills</a>
            <a class="section-chip" href="#8">Knowledge</a>
            <a class="section-chip" href="#9">Custom</a>
            <a class="section-chip" href="#10">Shared</a>
          </div>
        </div>
        <h2>Six tool types form the capability map</h2>
        <p class="section-lead">Decomposition enables governance, reuse, and sharing</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Tool types</span>
            <ul>
              <li>Built-in tools: files, commands, ptc</li>
              <li>MCP tools: external services</li>
              <li>Skills tools: codified workflows</li>
              <li>Knowledge tools: searchable docs</li>
              <li>Custom tools: personal packs</li>
              <li>Shared tools: team pool</li>
            </ul>
            <span class="pill">Unified governance</span>
            <p>Shared catalog, allowlist control, composable usage</p>
            <span class="pill">Value</span>
            <p>Standardize capabilities and make results reusable</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/tool-system-overview.svg" alt="Tool system overview (six tool types + orchestration)" />
          </div>
        </div>
      </section>
  `);
}
