"use strict";

import { createSlide } from "./utils.js";

// 第 16 页：智能体管理测试功能，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Agent management testing">
        <div class="slide-meta">
          <span class="section-tag">Section 5 Agent management testing</span>
          <div class="section-map">
            <a class="section-chip active" href="#16">Overview</a>
            <a class="section-chip" href="#17">Session management</a>
            <a class="section-chip" href="#18">Throughput testing</a>
            <a class="section-chip" href="#19">Capability evaluation</a>
          </div>
        </div>
        <h2>Agent management testing: overview</h2>
        <p class="section-lead">Section 5 spans four slides: session management / throughput testing / capability evaluation</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Session management</span>
            <p>Lifecycle visibility, conflicts surfaced, cancel when needed</p>
            <span class="pill">Throughput testing</span>
            <p>Concurrency benchmarks with QPS and resource utilization</p>
            <span class="pill">Capability evaluation</span>
            <p>Success rates, quality scoring, and regression comparisons</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/agent-management-overview.svg" alt="Management testing overview illustration" />
          </div>
        </div>
      </section>
  `);
}
