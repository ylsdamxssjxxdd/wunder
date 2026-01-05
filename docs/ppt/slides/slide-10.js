"use strict";

import { createSlide } from "./utils.js";

// 第 10 页：共享工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="共享工具">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <a class="section-chip" href="#4">总览</a>
            <a class="section-chip" href="#5">内置</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip" href="#7">Skills</a>
            <a class="section-chip" href="#8">知识库</a>
            <a class="section-chip" href="#9">自建</a>
            <a class="section-chip active" href="#10">共享</a>
          </div>
        </div>
        <h2>共享工具：装备市场</h2>
        <p class="section-lead">能力共享，但工作区仍独立</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">是什么</span>
            <ul>
              <li>共享工具清单与配置</li>
              <li>不共享对方工作区</li>
              <li>命名统一为 owner_id@tool</li>
            </ul>
            <span class="pill">有什么用</span>
            <ul>
              <li>成熟能力快速复制到团队</li>
              <li>降低协作门槛与沟通成本</li>
            </ul>
            <span class="pill">治理要点</span>
            <p>使用者需显式启用共享工具</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/tool-shared.svg" alt="共享工具示意图" />
          </div>
        </div>
      </section>
  `);
}
