"use strict";

import { createSlide } from "./utils.js";

// 第 7 页：MCP 工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="MCP tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <a class="section-chip" href="#5">Overview</a>
            <a class="section-chip" href="#6">Built-in</a>
            <a class="section-chip active" href="#7">MCP</a>
            <a class="section-chip" href="#8">Skills</a>
            <a class="section-chip" href="#9">Knowledge</a>
            <a class="section-chip" href="#10">Custom</a>
            <a class="section-chip" href="#11">Shared</a>
          </div>
        </div>
        <h2>MCP tools: connect external systems</h2>
        <p class="section-lead">When built-ins are not enough, bring in external capabilities</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What it is</span>
            <ul>
              <li>Connect external services via MCP</li>
              <li>Call as server@tool</li>
              <li>Auto included in the tool catalog</li>
            </ul>
            <span class="pill">Why it matters</span>
            <ul>
              <li>Connect enterprise systems, search, BI, and more</li>
              <li>Build cross-system execution chains</li>
            </ul>
            <span class="pill">Governance</span>
            <p>allow_tools allowlist + unified timeout control</p>
          </div>
          <div class="card media-panel stack">
            <h3>Image placeholder</h3>
            <p>Suggested: MCP topology or external services map</p>
            <span class="tag">assets/tool-mcp.png</span>
          </div>
        </div>
      </section>
  `);
}
