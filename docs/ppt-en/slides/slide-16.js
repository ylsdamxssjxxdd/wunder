"use strict";

import { createSlide } from "./utils.js";

// 第 16 页：智能体线程管理，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Session management">
        <div class="slide-meta">
          <span class="section-tag">Section 5 Agent session management</span>
          <div class="section-map">
            <a class="section-chip active" href="#16">Session management</a>
          </div>
        </div>
        <h2>Agent session management: stable and controllable</h2>
        <p class="section-lead">Ensure stability under concurrent tasks</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Lifecycle</span>
            <p>running → finished / error / cancelled</p>
            <span class="pill">Concurrency rules</span>
            <p>One thread per user, conflicts reported immediately</p>
            <span class="pill">Monitoring & cancel</span>
            <p>Traceable process, cancel when stuck</p>
          </div>
          <div class="card media-panel stack">
            <h3>Image placeholder</h3>
            <p>Suggested: session state machine or monitor dashboard</p>
            <span class="tag">assets/monitor-thread.png</span>
          </div>
        </div>
      </section>
  `);
}
