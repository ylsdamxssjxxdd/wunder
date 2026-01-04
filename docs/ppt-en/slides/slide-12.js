"use strict";

import { createSlide } from "./utils.js";

// 第 12 页：工作区，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Workspace">
        <div class="slide-meta">
          <span class="section-tag">Section 3 Workspace</span>
          <div class="section-map">
            <a class="section-chip active" href="#12">Workspace</a>
            <a class="section-chip" href="#13">Demo</a>
          </div>
        </div>
        <h2>Workspace: a long-term home for artifacts</h2>
        <p class="section-lead">Outputs persist and keep accumulating</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Purpose</span>
            <p>One persistent space per user</p>
            <span class="pill">Path example</span>
            <p>data/workspaces/&lt;user_id&gt;/files</p>
            <span class="pill">Stored content</span>
            <ul>
              <li>Docs, scripts, analysis results</li>
              <li>Tool outputs and intermediate files</li>
            </ul>
            <span class="pill">Why it matters</span>
            <ul>
              <li>Conversation output becomes assets</li>
              <li>Reuse the same materials across sessions</li>
              <li>Easy to share, reuse, and collaborate</li>
            </ul>
          </div>
          <div class="card media-panel stack">
            <h3>Image placeholder</h3>
            <p>Suggested: workspace tree or file list screenshot</p>
            <span class="tag">assets/workspace-tree.png</span>
          </div>
        </div>
      </section>
  `);
}
