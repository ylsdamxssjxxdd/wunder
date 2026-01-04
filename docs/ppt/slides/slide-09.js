"use strict";

import { createSlide } from "./utils.js";

// 第 9 页：自建工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="自建工具">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <a class="section-chip" href="#4">总览</a>
            <a class="section-chip" href="#5">内置</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip" href="#7">Skills</a>
            <a class="section-chip" href="#8">知识库</a>
            <a class="section-chip active" href="#9">自建</a>
            <a class="section-chip" href="#10">共享</a>
          </div>
        </div>
        <h2>自建工具：个人专属能力</h2>
        <p class="section-lead">每个人都能拥有自己的工具箱</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">是什么</span>
            <ul>
              <li>个人工具包独立配置</li>
              <li>路径：data/user_tools/&lt;user_id&gt;</li>
              <li>别名统一为 user_id@tool</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">有什么用</span>
            <ul>
              <li>满足个性化需求且不影响他人</li>
              <li>能力可选择共享给团队</li>
            </ul>
            <span class="pill">治理要点</span>
            <p>隔离可控，启用需明确勾选</p>
          </div>
        </div>
      </section>
  `);
}
