"use strict";

import { createSlide } from "./utils.js";

// 第 18 页：Throughput testing，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Throughput testing">
        <div class="slide-meta">
          <span class="section-tag">Section 5 Agent management testing</span>
          <div class="section-map">
            <a class="section-chip" href="#16">Overview</a>
            <a class="section-chip" href="#17">Session management</a>
            <a class="section-chip active" href="#18">Throughput testing</a>
            <a class="section-chip" href="#19">Capability evaluation</a>
          </div>
        </div>
        <h2>Throughput testing: concurrency benchmarks</h2>
        <p class="section-lead">Simulate real load to measure QPS, latency, and resource usage</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Scenario setup</span>
            <p>Concurrency, duration, traffic profile</p>
            <span class="pill">Benchmark metrics</span>
            <p>QPS / P95 latency / error rate</p>
            <span class="pill">Outputs</span>
            <p>Performance baselines and bottlenecks</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/agent-throughput-testing.svg" alt="Throughput testing illustration" />
          </div>
        </div>
      </section>
  `);
}
