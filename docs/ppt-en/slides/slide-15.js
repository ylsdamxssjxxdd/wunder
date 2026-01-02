"use strict";

import { createSlide } from "./utils.js";

// 第 15 页：智能体线程管理，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Session management">
        <div class="slide-meta">
          <span class="section-tag">Section 6 Agent session management</span>
          <div class="section-map">
            <span class="section-chip active">Session management</span>
          </div>
        </div>
        <h2>Agent session management: stable and controllable</h2>
        <p class="section-lead">Ensure stability under concurrent tasks</p>
        <div class="grid three">
          <div class="card">
            <h3>Lifecycle</h3>
            <p>running → finished / error / cancelled</p>
          </div>
          <div class="card">
            <h3>Concurrency rules</h3>
            <p>One thread per user, conflicts reported immediately</p>
          </div>
          <div class="card">
            <h3>Monitoring & cancel</h3>
            <p>Traceable process, cancel when stuck</p>
          </div>
        </div>
      </section>
  `);
}
