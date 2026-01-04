"use strict";

import { createSlide } from "./utils.js";

// 第 17 页：快速开始，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Quick start">
        <div class="slide-meta">
          <span class="section-tag">Section 6 Quick start</span>
          <div class="section-map">
            <span class="section-chip active">Quick start</span>
            <span class="section-chip">Closing</span>
          </div>
        </div>
        <h2>Quick start: three steps</h2>
        <p class="section-lead">Start with a high-frequency scenario and see results fast</p>
        <div class="grid three">
          <div class="card">
            <h3>1. Pick a scenario</h3>
            <p>Choose a frequent, well-defined need</p>
          </div>
          <div class="card">
            <h3>2. Pick a tool mix</h3>
            <p>Combine tools + knowledge + skills</p>
          </div>
          <div class="card">
            <h3>3. Solidify the workflow</h3>
            <p>Turn success into a workflow template</p>
          </div>
        </div>
        <div class="card soft stack">
          <span class="pill">Pilot examples</span>
          <ul>
            <li>Policy/process Q&A (knowledge base)</li>
            <li>Weekly report/minutes generation (skills)</li>
            <li>Document cleanup and batch processing (built-in tools)</li>
          </ul>
        </div>
      </section>
  `);
}
