"use strict";

import { createSlide } from "./utils.js";

// 第 5 页：内置工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Built-in tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <span class="section-chip">Overview</span>
            <span class="section-chip active">Built-in</span>
            <span class="section-chip">MCP</span>
            <span class="section-chip">Skills</span>
            <span class="section-chip">Knowledge</span>
            <span class="section-chip">Custom</span>
            <span class="section-chip">Shared</span>
          </div>
        </div>
        <h2>Built-in tools: core actions</h2>
        <p class="section-lead">Standardize common actions first</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What it is</span>
            <ul>
              <li>File read/write, search, replace</li>
              <li>Command execution and script running</li>
              <li>ptc temporary scripts</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">Why it matters</span>
            <ul>
              <li>Write answers directly into files</li>
              <li>Batch clean, process, and generate outputs</li>
            </ul>
            <span class="pill">Governance</span>
            <p>allow_paths / allow_commands control access scope</p>
          </div>
        </div>
      </section>
  `);
}
