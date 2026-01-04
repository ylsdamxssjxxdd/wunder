"use strict";

import { createSlide } from "./utils.js";

// 第 5 页：工具体系总览，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="工具体系">
        <div class="slide-meta">
          <span class="section-tag">第2节 工具体系</span>
          <div class="section-map">
            <a class="section-chip active" href="#5">总览</a>
            <a class="section-chip" href="#6">内置</a>
            <a class="section-chip" href="#7">MCP</a>
            <a class="section-chip" href="#8">Skills</a>
            <a class="section-chip" href="#9">知识库</a>
            <a class="section-chip" href="#10">自建</a>
            <a class="section-chip" href="#11">共享</a>
          </div>
        </div>
        <h2>六类工具构成能力地图</h2>
        <p class="section-lead">能力拆分后，才能被治理、复用与共享</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">工具类型</span>
            <ul>
              <li>内置工具：文件/命令/ptc</li>
              <li>MCP 工具：外部服务接入</li>
              <li>Skills 工具：流程固化</li>
              <li>知识库工具：可检索资料</li>
              <li>自建工具：个人能力包</li>
              <li>共享工具：团队复用池</li>
            </ul>
            <span class="pill">统一治理</span>
            <p>统一清单、白名单管控、可组合使用</p>
            <span class="pill">价值</span>
            <p>让能力标准化、可追踪、可复用</p>
          </div>
          <div class="card media-panel stack">
            <h3>图片占位</h3>
            <p>建议：工具体系总览图（六类工具 + 调度层）</p>
            <span class="tag">assets/tool-system-overview.png</span>
          </div>
        </div>
      </section>
  `);
}
