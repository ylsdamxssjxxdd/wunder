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
            <span class="pill">Runtime flow</span>
            <!-- 运行流程图：对应 docs/系统介绍.md 的“从请求到回复” -->
            <div class="flow">
              <div class="flow-item">User/Client</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">API layer</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">Orchestrator</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">Prompt Builder</div>
            </div>
            <div class="flow">
              <div class="flow-item">LLM</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">Tool executor</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">Storage/Monitor</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">SSE/Final reply</div>
            </div>
            <p class="hint">Request: POST /wunder (user_id, question, tool_names, stream)</p>
            <div class="note">
              <strong>Principle:</strong> for developers everything is an interface, for the model everything is a tool
            </div>
          </div>
        </div>
      </section>
  `);
}
