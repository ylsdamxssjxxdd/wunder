"use strict";

import { createSlide } from "./utils.js";

// 第 2 页：核心理念，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Core idea">
        <div class="slide-meta">
          <span class="section-tag">Section 1 Core idea</span>
          <div class="section-map">
            <a class="section-chip active" href="#2">Core idea</a>
            <a class="section-chip" href="#3">Runtime flow</a>
          </div>
        </div>
        <h2>From chatting to getting things done</h2>
        <p class="section-lead">One question connects understanding to delivery</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What users see</span>
            <ul>
              <li>Just ask the question</li>
              <li>Process is clear and traceable</li>
              <li>Results become deliverables</li>
            </ul>
            <span class="pill">Unified entry</span>
            <p>/wunder supports streaming progress and final replies</p>
          </div>
          <div class="card soft stack">
            <span class="pill">Core idea</span>
            <ul>
              <li>For developers: everything is an interface (API/config/tools)</li>
              <li>For the model: everything is a tool (callable, composable, governable)</li>
              <li>One question drives the full execution chain</li>
            </ul>
            <div class="note">
              <strong>Outcome:</strong> answers become reusable deliverables
            </div>
          </div>
        </div>
      </section>
  `);
}
