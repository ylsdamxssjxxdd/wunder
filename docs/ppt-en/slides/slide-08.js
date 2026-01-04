"use strict";

import { createSlide } from "./utils.js";

// 第 8 页：Skills 工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Skills tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <a class="section-chip" href="#5">Overview</a>
            <a class="section-chip" href="#6">Built-in</a>
            <a class="section-chip" href="#7">MCP</a>
            <a class="section-chip active" href="#8">Skills</a>
            <a class="section-chip" href="#9">Knowledge</a>
            <a class="section-chip" href="#10">Custom</a>
            <a class="section-chip" href="#11">Shared</a>
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
            <span class="pill">Why it matters</span>
            <ul>
              <li>Consistent execution and repeatability</li>
              <li>Reduce manual work and communication cost</li>
            </ul>
            <span class="pill">Governance</span>
            <p>Versioning with clear sharing boundaries</p>
          </div>
          <div class="card media-panel stack">
            <h3>Image placeholder</h3>
            <p>Suggested: SKILL.md + run.py structure or workflow diagram</p>
            <span class="tag">assets/tool-skills.png</span>
          </div>
        </div>
      </section>
  `);
}
