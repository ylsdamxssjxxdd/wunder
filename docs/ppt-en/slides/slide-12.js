"use strict";

import { createSlide } from "./utils.js";

// 第 12 页：智能体功能演示，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Agent demo">
        <div class="slide-meta">
          <span class="section-tag">Section 3 Workspace</span>
          <div class="section-map">
            <a class="section-chip" href="#11">Workspace</a>
            <a class="section-chip active" href="#12">Demo</a>
          </div>
        </div>
        <h2>Agent demo: draw a heart and save it</h2>
        <p class="section-lead">Prove the tools + workspace loop</p>
        <div class="grid three">
          <div class="card">
            <h3>1. Ask</h3>
            <p>Please draw a heart in Python</p>
          </div>
          <div class="card">
            <h3>2. Run</h3>
            <p>Generate an image and save to the temp workspace</p>
          </div>
          <div class="card">
            <h3>3. Download</h3>
            <p>User downloads the result locally</p>
          </div>
        </div>
        <div class="note">
          <strong>Result:</strong> from one sentence to a deliverable file
        </div>
      </section>
  `);
}
