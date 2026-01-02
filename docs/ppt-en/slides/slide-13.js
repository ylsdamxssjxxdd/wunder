"use strict";

import { createSlide } from "./utils.js";

// 第 13 页：上下文压缩，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Context compaction">
        <div class="slide-meta">
          <span class="section-tag">Section 4 Context compaction</span>
          <div class="section-map">
            <span class="section-chip active">Context compaction</span>
          </div>
        </div>
        <h2>Context compaction: keep long chats usable</h2>
        <p class="section-lead">Long conversations get heavy and need a stable mechanism</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Triggers</span>
            <ul>
              <li>Context token usage hits the threshold</li>
              <li>Current message nears the safe limit</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">How it works</span>
            <ul>
              <li>Keep system prompt + recent messages</li>
              <li>Generate a structured summary and inject it</li>
              <li>Reset history usage and continue</li>
            </ul>
            <span class="pill">Metric note</span>
            <p>Counts context tokens, not total usage</p>
          </div>
        </div>
      </section>
  `);
}
