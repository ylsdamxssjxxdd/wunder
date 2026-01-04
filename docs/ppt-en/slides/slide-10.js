"use strict";

import { createSlide } from "./utils.js";

// 第 10 页：自建工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Custom tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <a class="section-chip" href="#5">Overview</a>
            <a class="section-chip" href="#6">Built-in</a>
            <a class="section-chip" href="#7">MCP</a>
            <a class="section-chip" href="#8">Skills</a>
            <a class="section-chip" href="#9">Knowledge</a>
            <a class="section-chip active" href="#10">Custom</a>
            <a class="section-chip" href="#11">Shared</a>
          </div>
        </div>
        <h2>Custom tools: personal capabilities</h2>
        <p class="section-lead">Everyone can have their own toolbox</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What it is</span>
            <ul>
              <li>Personal tool pack configuration</li>
              <li>Path: data/user_tools/&lt;user_id&gt;</li>
              <li>Alias as user_id@tool</li>
            </ul>
            <span class="pill">Why it matters</span>
            <ul>
              <li>Meet personal needs without affecting others</li>
              <li>Optionally share capabilities with the team</li>
            </ul>
            <span class="pill">Governance</span>
            <p>Isolated and controlled, must be explicitly enabled</p>
          </div>
          <div class="card media-panel stack">
            <h3>Image placeholder</h3>
            <p>Suggested: user tool folder structure or config view</p>
            <span class="tag">assets/tool-custom.png</span>
          </div>
        </div>
      </section>
  `);
}
