"use strict";

import { createSlide } from "./utils.js";

// 第 7 页：Skills 工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Skills tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <span class="section-chip">Overview</span>
            <span class="section-chip">Built-in</span>
            <span class="section-chip">MCP</span>
            <span class="section-chip active">Skills</span>
            <span class="section-chip">Knowledge</span>
            <span class="section-chip">Custom</span>
            <span class="section-chip">Shared</span>
          </div>
        </div>
        <h2>Skills tools: turn experience into workflows</h2>
        <p class="section-lead">Turn successful practice into repeatable SOPs</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What it is</span>
            <ul>
              <li>SKILL.md defines inputs and steps</li>
              <li>run.py executes the workflow</li>
              <li>One-line trigger for the full task</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">Why it matters</span>
            <ul>
              <li>Consistent execution and repeatability</li>
              <li>Reduce manual work and communication cost</li>
            </ul>
            <span class="pill">Governance</span>
            <p>Versioning with clear sharing boundaries</p>
          </div>
        </div>
      </section>
  `);
}
