"use strict";

import { createSlide } from "./utils.js";

// 第 18 页：收尾，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Closing">
        <div class="slide-meta">
          <span class="section-tag">Section 6 Quick start</span>
          <div class="section-map">
            <span class="section-chip">Quick start</span>
            <span class="section-chip active">Closing</span>
          </div>
        </div>
        <h2>Thanks</h2>
        <p class="section-lead">Questions welcome, and pilots too</p>
        <div class="card">
          <h3>wunder: make LLMs get work done and stay reusable</h3>
          <p>Start from one scenario and turn success into tools and workflows</p>
        </div>
      </section>
  `);
}
