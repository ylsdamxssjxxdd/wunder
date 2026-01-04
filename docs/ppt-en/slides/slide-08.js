"use strict";

import { createSlide } from "./utils.js";

// 第 8 页：知识库工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Knowledge tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <a class="section-chip" href="#4">Overview</a>
            <a class="section-chip" href="#5">Built-in</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip" href="#7">Skills</a>
            <a class="section-chip active" href="#8">Knowledge</a>
            <a class="section-chip" href="#9">Custom</a>
            <a class="section-chip" href="#10">Shared</a>
          </div>
        </div>
        <h2>Knowledge tools: make docs searchable tools</h2>
        <p class="section-lead">Make answers traceable and reduce guesswork</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What it is</span>
            <ul>
              <li>Build knowledge bases with Markdown</li>
              <li>Split knowledge by headings</li>
              <li>query / limit as inputs</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">Why it matters</span>
            <ul>
              <li>Unified search for policies, processes, docs</li>
              <li>Traceable, reusable answers</li>
            </ul>
            <span class="pill">Governance</span>
            <p>Regular updates and access control</p>
          </div>
        </div>
      </section>
  `);
}
