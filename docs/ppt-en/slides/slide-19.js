"use strict";

import { createSlide } from "./utils.js";

// 第 19 页：Capability evaluation，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Capability evaluation">
        <div class="slide-meta">
          <span class="section-tag">Section 5 Agent management testing</span>
          <div class="section-map">
            <a class="section-chip" href="#16">Overview</a>
            <a class="section-chip" href="#17">Session management</a>
            <a class="section-chip" href="#18">Throughput testing</a>
            <a class="section-chip active" href="#19">Capability evaluation</a>
          </div>
        </div>
        <h2>Capability evaluation: quality & regression</h2>
        <p class="section-lead">Unified test sets and metrics to track improvements over time</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Test sets</span>
            <p>Real scenarios with traceable samples</p>
            <span class="pill">Scoring</span>
            <p>Accuracy / completeness / usability</p>
            <span class="pill">Regression</span>
            <p>Compare versions at a glance</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/agent-capability-eval.svg" alt="Capability evaluation illustration" />
          </div>
        </div>
      </section>
  `);
}
