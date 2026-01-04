"use strict";

import { createSlide } from "./utils.js";

// 第 15 页：A2A 标准接口，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="A2A standard interface">
        <div class="slide-meta">
          <span class="section-tag">Section 4 Frontier features</span>
          <div class="section-map">
            <a class="section-chip" href="#13">Memory & compaction</a>
            <a class="section-chip" href="#14">A2UI</a>
            <a class="section-chip active" href="#15">A2A</a>
          </div>
        </div>
        <h2>A2A: standardized agent interface</h2>
        <p class="section-lead">JSON-RPC + SSE for interoperable agent workflows</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Capabilities</span>
            <ul>
              <li>SendMessage / SendStreamingMessage</li>
              <li>GetTask / ListTasks / SubscribeToTask</li>
              <li>AgentCard discovery</li>
            </ul>
            <span class="pill">How to connect</span>
            <ul>
              <li>Endpoint: /a2a (JSON-RPC 2.0)</li>
              <li>Discovery: /.well-known/agent-card.json</li>
              <li>Auth: unified API Key check</li>
            </ul>
            <span class="pill">Value</span>
            <p>Unified cross-system calls with Wunder tooling</p>
          </div>
          <div class="card media-panel stack">
            <h3>Image placeholder</h3>
            <p>Suggested: client ↔ /a2a ↔ wunder call flow</p>
            <span class="tag">assets/feature-a2a.png</span>
          </div>
        </div>
      </section>
  `);
}
