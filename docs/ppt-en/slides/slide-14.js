"use strict";

import { createSlide } from "./utils.js";

// 第 14 页：长期记忆，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Long-term memory">
        <div class="slide-meta">
          <span class="section-tag">Section 5 Long-term memory</span>
          <div class="section-map">
            <span class="section-chip active">Long-term memory</span>
          </div>
        </div>
        <h2>Long-term memory: consistent across sessions</h2>
        <p class="section-lead">Carry key conclusions into the next collaboration</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">How it is generated</span>
            <ul>
              <li>Auto summarize after the final reply</li>
              <li>Write to long-term memory (up to 30 items)</li>
              <li>Timestamped for traceability</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">Usage and control</span>
            <ul>
              <li>Injected as [Long-term memory] in later sessions</li>
              <li>Enable, disable, clear, delete</li>
              <li>Keep preferences and constraints consistent</li>
            </ul>
          </div>
        </div>
      </section>
  `);
}
