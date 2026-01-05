"use strict";

import { createSlide } from "./utils.js";

// 第 8 页：知识库工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="知识库工具">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <a class="section-chip" href="#4">总览</a>
            <a class="section-chip" href="#5">内置</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip" href="#7">Skills</a>
            <a class="section-chip active" href="#8">知识库</a>
            <a class="section-chip" href="#9">自建</a>
            <a class="section-chip" href="#10">共享</a>
          </div>
        </div>
        <h2>知识库工具：百科全书</h2>
        <p class="section-lead">让答案可追溯，减少“拍脑袋”</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">是什么</span>
            <ul>
              <li>用 Markdown 构建资料库</li>
              <li>按标题切分知识点</li>
              <li>query / limit 作为输入</li>
            </ul>
            <span class="pill">有什么用</span>
            <ul>
              <li>制度、流程、资料统一检索</li>
              <li>回答可追溯、可复用</li>
            </ul>
            <span class="pill">治理要点</span>
            <p>定期更新与权限控制</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/tool-knowledge.svg" alt="知识库工具示意图" />
          </div>
        </div>
      </section>
  `);
}
