"use strict";

import { createSlide } from "./utils.js";

// 第 2 页：目录页（明确章节结构），用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide toc" data-title="Agenda">
        <div class="eyebrow">Agenda</div>
        <h2>Six parts for today</h2>
        <div class="toc-grid">
          <div class="toc-item">
            <div class="toc-index">01</div>
            <div>
              <div class="toc-title">Core idea</div>
              <div class="toc-desc">From chat to action</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">02</div>
            <div>
              <div class="toc-title">Tool system</div>
              <div class="toc-desc">Six tool types and unified governance</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">03</div>
            <div>
              <div class="toc-title">Workspace</div>
              <div class="toc-desc">Deliverables and reusable assets</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">04</div>
            <div>
              <div class="toc-title">Frontier features</div>
              <div class="toc-desc">Memory/compaction + A2UI + A2A</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">05</div>
            <div>
              <div class="toc-title">Agent session management</div>
              <div class="toc-desc">Stable, observable, cancelable</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">06</div>
            <div>
              <div class="toc-title">Quick start</div>
              <div class="toc-desc">Start with one scenario</div>
            </div>
          </div>
        </div>
      </section>
  `);
}
