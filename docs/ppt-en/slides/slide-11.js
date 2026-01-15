"use strict";

import { createSlide } from "./utils.js";

// 第 11 页：工作区，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Workspace">
        <div class="slide-meta">
          <span class="section-tag">Section 3 Workspace</span>
          <div class="section-map">
            <a class="section-chip active" href="#11">Workspace</a>
            <a class="section-chip" href="#12">Demo</a>
          </div>
        </div>
        <h2>Workspace: a long-term home for artifacts</h2>
        <p class="section-lead">Outputs persist and keep accumulating</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">Purpose</span>
            <p>One persistent space per user</p>
            <span class="pill">Path example</span>
            <p>data/workspaces/&lt;user_id&gt;</p>
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
          <div class="card media-panel is-image stack">
            <img src="assets/workspace-tree.svg" alt="Workspace tree illustration" />
          </div>
        </div>
      </section>
  `);
}
