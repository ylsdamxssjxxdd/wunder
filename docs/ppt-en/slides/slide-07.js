"use strict";

import { createSlide } from "./utils.js";

// 第 7 页：Skills 工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Skills tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <a class="section-chip" href="#4">Overview</a>
            <a class="section-chip" href="#5">Built-in</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip active" href="#7">Skills</a>
            <a class="section-chip" href="#8">Knowledge</a>
            <a class="section-chip" href="#9">Custom</a>
            <a class="section-chip" href="#10">Shared</a>
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
