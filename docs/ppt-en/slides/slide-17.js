"use strict";

import { createSlide } from "./utils.js";

// 第 17 页：Session management，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Session management">
        <div class="slide-meta">
          <span class="section-tag">Section 5 Agent management testing</span>
          <div class="section-map">
            <a class="section-chip" href="#16">Overview</a>
            <a class="section-chip active" href="#17">Session management</a>
            <a class="section-chip" href="#18">Throughput testing</a>
            <a class="section-chip" href="#19">Capability evaluation</a>
          </div>
        </div>
        <h2>Session management: controlled concurrency</h2>
        <p class="section-lead">Unified lifecycle, concurrency rules, and monitoring events</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Lifecycle</span>
            <p>running → finished / error / cancelled</p>
            <span class="pill">Concurrency rules</span>
            <p>Single session per user, conflicts surfaced</p>
            <span class="pill">Monitoring & cancel</span>
            <p>Live events with safe termination</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/agent-thread-management.svg" alt="Session management illustration" />
          </div>
        </div>
      </section>
  `);
}
