"use strict";

import { createSlide } from "./utils.js";

// 第 5 页：内置工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="内置工具">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <span class="section-chip">总览</span>
            <span class="section-chip active">内置</span>
            <span class="section-chip">MCP</span>
            <span class="section-chip">Skills</span>
            <span class="section-chip">知识库</span>
            <span class="section-chip">自建</span>
            <span class="section-chip">共享</span>
          </div>
        </div>
        <h2>内置工具：基础手脚</h2>
        <p class="section-lead">先把常用动作标准化</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">是什么</span>
            <ul>
              <li>文件读写、搜索、替换</li>
              <li>命令执行与脚本运行</li>
              <li>ptc 临时脚本支持</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">有什么用</span>
            <ul>
              <li>把答案直接写成文件结果</li>
              <li>批量整理、清洗、生成产物</li>
            </ul>
            <span class="pill">治理要点</span>
            <p>allow_paths / allow_commands 控制访问范围</p>
          </div>
        </div>
      </section>
  `);
}
