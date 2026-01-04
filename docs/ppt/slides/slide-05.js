"use strict";

import { createSlide } from "./utils.js";

// 第 5 页：内置工具，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="内置工具">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <a class="section-chip" href="#4">总览</a>
            <a class="section-chip active" href="#5">内置</a>
            <a class="section-chip" href="#6">MCP</a>
            <a class="section-chip" href="#7">Skills</a>
            <a class="section-chip" href="#8">知识库</a>
            <a class="section-chip" href="#9">自建</a>
            <a class="section-chip" href="#10">共享</a>
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
            <span class="pill">有什么用</span>
            <ul>
              <li>把答案直接写成文件结果</li>
              <li>批量整理、清洗、生成产物</li>
            </ul>
            <span class="pill">治理要点</span>
            <p>allow_paths / allow_commands 控制访问范围</p>
          </div>
          <div class="card media-panel stack">
            <h3>图片占位</h3>
            <p>建议：内置工具清单或文件操作示意</p>
            <span class="tag">assets/tool-builtin.png</span>
          </div>
        </div>
      </section>
  `);
}
