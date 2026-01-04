"use strict";

import { createSlide } from "./utils.js";

// 第 3 页：核心理念，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Core idea">
        <div class="slide-meta">
          <span class="section-tag">Section 1 Core idea</span>
          <div class="section-map">
            <a class="section-chip active" href="#3">Core idea</a>
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
            <span class="pill">Execution chain</span>
            <div class="flow">
              <div class="flow-item">Question</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">Task planning</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">Tool execution</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">Deliverables</div>
            </div>
            <div class="note">
              <strong>Principle:</strong> for developers everything is an interface, for the model everything is a tool
            </div>
          </div>
        </div>
      </section>
  `);
}
