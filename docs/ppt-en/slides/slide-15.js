"use strict";

import { createSlide } from "./utils.js";

// 第 14 页：A2UI，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="A2UI">
        <div class="slide-meta">
          <span class="section-tag">Section 4 Frontier features</span>
          <div class="section-map">
            <a class="section-chip" href="#13">Memory & compaction</a>
            <a class="section-chip active" href="#14">A2UI</a>
            <a class="section-chip" href="#15">A2A</a>
          </div>
        </div>
        <h2>A2UI: turn answers into UI</h2>
        <p class="section-lead">Structured output that the front-end can render directly</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What it is</span>
            <ul>
              <li>Model outputs A2UI JSON messages</li>
              <li>Front-end renders cards, forms, buttons</li>
              <li>Structured display for process and results</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">How to use</span>
            <ul>
              <li>Explicitly enable the a2ui tool</li>
              <li>SSE emits a2ui events</li>
              <li>Render with the A2UI component spec</li>
            </ul>
            <span class="pill">Value</span>
            <p>Lower UI integration cost, clearer UX</p>
          </div>
        </div>
      </section>
  `);
}
