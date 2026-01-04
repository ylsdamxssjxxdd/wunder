"use strict";

import { createSlide } from "./utils.js";

// 第 4 页：运行流程，用于展示 /wunder 从请求到回复的链路。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Runtime flow">
        <div class="slide-meta">
          <span class="section-tag">Section 1 Core idea</span>
          <div class="section-map">
            <a class="section-chip" href="#3">Core idea</a>
            <a class="section-chip active" href="#4">Runtime flow</a>
          </div>
        </div>
        <h2>From request to response</h2>
        <p class="section-lead">One question drives understanding → execution → delivery</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Key steps</span>
            <ul>
              <li>POST /wunder (user_id, question, tool_names, stream)</li>
              <li>Orchestrator plans and builds the prompt</li>
              <li>LLM triggers tools, tools return results</li>
              <li>SSE streams progress and final reply</li>
            </ul>
            <span class="pill">Why it matters</span>
            <p>One entry point, observable execution, reusable outputs.</p>
          </div>
          <div class="card media-panel stack">
            <h3>Image placeholder</h3>
            <p>Suggested: request-to-response flow diagram</p>
            <span class="tag">assets/request-flow.png</span>
          </div>
        </div>
      </section>
  `);
}
