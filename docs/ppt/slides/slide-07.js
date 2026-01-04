"use strict";

import { createSlide } from "./utils.js";

// 第 7 页：Skills 工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="Skills 工具">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <a class="section-chip" href="#4">总览</a>
            <a class="section-chip" href="#5">内置</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip active" href="#7">Skills</a>
            <a class="section-chip" href="#8">知识库</a>
            <a class="section-chip" href="#9">自建</a>
            <a class="section-chip" href="#10">共享</a>
          </div>
        </div>
        <h2>Skills 工具：经验固化为流程</h2>
        <p class="section-lead">把成功经验写成可重复的 SOP</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">是什么</span>
            <ul>
              <li>SKILL.md 描述输入与步骤</li>
              <li>run.py 执行具体流程</li>
              <li>一句话触发完整任务</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">有什么用</span>
            <ul>
              <li>保证执行一致性与可复制性</li>
              <li>减少人工操作与沟通成本</li>
            </ul>
            <span class="pill">治理要点</span>
            <p>版本管理与共享边界清晰</p>
          </div>
        </div>
      </section>
  `);
}
