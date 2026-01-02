"use strict";

import { createSlide } from "./utils.js";

// 第 9 页：自建工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Custom tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <span class="section-chip">Overview</span>
            <span class="section-chip">Built-in</span>
            <span class="section-chip">MCP</span>
            <span class="section-chip">Skills</span>
            <span class="section-chip">Knowledge</span>
            <span class="section-chip active">Custom</span>
            <span class="section-chip">Shared</span>
          </div>
        </div>
        <h2>Custom tools: personal capabilities</h2>
        <p class="section-lead">Everyone can have their own toolbox</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What it is</span>
            <ul>
              <li>Personal tool pack configuration</li>
              <li>Path: data/user_tools/&lt;user_id&gt;</li>
              <li>Alias as user_id@tool</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">Why it matters</span>
            <ul>
              <li>Meet personal needs without affecting others</li>
              <li>Optionally share capabilities with the team</li>
            </ul>
            <span class="pill">Governance</span>
            <p>Isolated and controlled, must be explicitly enabled</p>
          </div>
        </div>
      </section>
  `);
}
