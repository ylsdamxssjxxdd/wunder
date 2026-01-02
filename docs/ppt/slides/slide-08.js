"use strict";

import { createSlide } from "./utils.js";

// 第 8 页：知识库工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="知识库工具">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <span class="section-chip">总览</span>
            <span class="section-chip">内置</span>
            <span class="section-chip">MCP</span>
            <span class="section-chip">Skills</span>
            <span class="section-chip active">知识库</span>
            <span class="section-chip">自建</span>
            <span class="section-chip">共享</span>
          </div>
        </div>
        <h2>知识库工具：把资料变成可检索工具</h2>
        <p class="section-lead">让答案可追溯，减少“拍脑袋”</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">是什么</span>
            <ul>
              <li>用 Markdown 构建资料库</li>
              <li>按标题切分知识点</li>
              <li>query / limit 作为输入</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">有什么用</span>
            <ul>
              <li>制度、流程、资料统一检索</li>
              <li>回答可追溯、可复用</li>
            </ul>
            <span class="pill">治理要点</span>
            <p>定期更新与权限控制</p>
          </div>
        </div>
      </section>
  `);
}
