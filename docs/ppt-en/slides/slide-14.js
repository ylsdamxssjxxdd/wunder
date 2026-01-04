"use strict";

import { createSlide } from "./utils.js";

// 第 13 页：前沿特性（记忆与上下文压缩），用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Frontier features">
        <div class="slide-meta">
          <span class="section-tag">Section 4 Frontier features</span>
          <div class="section-map">
            <a class="section-chip active" href="#13">Memory & compaction</a>
            <a class="section-chip" href="#14">A2UI</a>
            <a class="section-chip" href="#15">A2A</a>
          </div>
        </div>
        <h2>Frontier features: memory & context compaction</h2>
        <p class="section-lead">Keep long chats stable and carry key conclusions forward</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Context compaction</span>
            <ul>
              <li>Trigger: context usage hits threshold</li>
              <li>Keep system prompt + recent messages</li>
              <li>Generate a structured summary and continue</li>
            </ul>
            <span class="pill">Metric note</span>
            <p>Counts context tokens, not total usage</p>
          </div>
          <div class="card soft stack">
            <span class="pill">Long-term memory</span>
            <ul>
              <li>Auto summarize after the final reply</li>
              <li>Write to long-term memory with timestamp</li>
              <li>Injected as [Long-term memory] later</li>
            </ul>
            <span class="pill">Controls</span>
            <p>Enable, disable, clear, and delete</p>
          </div>
        </div>
      </section>
  `);
}
