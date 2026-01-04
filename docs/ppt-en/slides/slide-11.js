"use strict";

import { createSlide } from "./utils.js";

// 第 10 页：共享工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Shared tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <a class="section-chip" href="#4">Overview</a>
            <a class="section-chip" href="#5">Built-in</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip" href="#7">Skills</a>
            <a class="section-chip" href="#8">Knowledge</a>
            <a class="section-chip" href="#9">Custom</a>
            <a class="section-chip active" href="#10">Shared</a>
          </div>
        </div>
        <h2>Shared tools: team reuse</h2>
        <p class="section-lead">Capabilities are shared, workspaces stay isolated</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What it is</span>
            <ul>
              <li>Shared tool catalog and configuration</li>
              <li>Workspaces are not shared</li>
              <li>Named as owner_id@tool</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">Why it matters</span>
            <ul>
              <li>Replicate mature capabilities across the team</li>
              <li>Lower collaboration friction and cost</li>
            </ul>
            <span class="pill">Governance</span>
            <p>Users must explicitly enable shared tools</p>
          </div>
        </div>
      </section>
  `);
}
