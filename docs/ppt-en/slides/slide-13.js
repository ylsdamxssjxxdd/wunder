"use strict";

import { createSlide } from "./utils.js";

// 第 13 页：智能体功能演示，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Agent demo">
        <div class="slide-meta">
          <span class="section-tag">Section 3 Workspace</span>
          <div class="section-map">
            <a class="section-chip" href="#12">Workspace</a>
            <a class="section-chip active" href="#13">Demo</a>
          </div>
        </div>
        <h2>Agent demo: draw a heart and save it</h2>
        <p class="section-lead">Prove the tools + workspace loop</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Steps</span>
            <ul>
              <li>Ask: draw a heart in Python</li>
              <li>Run: generate and save in the workspace</li>
              <li>Download: user saves the result locally</li>
            </ul>
            <div class="note">
              <strong>Result:</strong> from one sentence to a deliverable file
            </div>
          </div>
          <div class="card media-panel stack">
            <h3>Image placeholder</h3>
            <p>Suggested: heart output or download result screenshot</p>
            <span class="tag">assets/demo-heart.png</span>
          </div>
        </div>
      </section>
  `);
}
