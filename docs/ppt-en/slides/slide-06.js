"use strict";

import { createSlide } from "./utils.js";

// 第 6 页：内置工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Built-in tools">
        <div class="slide-meta">
          <span class="section-tag">Section 2 Tool system</span>
          <div class="section-map">
            <a class="section-chip" href="#5">Overview</a>
            <a class="section-chip active" href="#6">Built-in</a>
            <a class="section-chip" href="#7">MCP</a>
            <a class="section-chip" href="#8">Skills</a>
            <a class="section-chip" href="#9">Knowledge</a>
            <a class="section-chip" href="#10">Custom</a>
            <a class="section-chip" href="#11">Shared</a>
          </div>
        </div>
        <h2>Built-in tools: core actions</h2>
        <p class="section-lead">Standardize common actions first</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">What it is</span>
            <ul>
              <li>File read/write, search, replace</li>
              <li>Command execution and script running</li>
              <li>ptc temporary scripts</li>
            </ul>
            <span class="pill">Why it matters</span>
            <ul>
              <li>Write answers directly into files</li>
              <li>Batch clean, process, and generate outputs</li>
            </ul>
            <span class="pill">Governance</span>
            <p>allow_paths / allow_commands control access scope</p>
          </div>
          <div class="card media-panel stack">
            <h3>Image placeholder</h3>
            <p>Suggested: built-in tool list or file operations</p>
            <span class="tag">assets/tool-builtin.png</span>
          </div>
        </div>
      </section>
  `);
}
