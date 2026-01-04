"use strict";

import { createSlide } from "./utils.js";

// 第 3 页：核心理念，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="核心理念">
        <div class="slide-meta">
          <span class="section-tag">第1节 核心理念</span>
          <div class="section-map">
            <a class="section-chip active" href="#3">核心理念</a>
          </div>
        </div>
        <h2>从“会聊”到“会做事”</h2>
        <p class="section-lead">一次提问，跑通从理解到落地的链路</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">用户看到的</span>
            <ul>
              <li>只需提出问题</li>
              <li>过程清晰可追踪</li>
              <li>结果能落成产物</li>
            </ul>
            <span class="pill">统一入口</span>
            <p>/wunder 支持流式返回过程与最终回复</p>
          </div>
          <div class="card soft stack">
            <span class="pill">运行流程</span>
            <!-- 运行流程图：对应 docs/系统介绍.md 的“从请求到回复” -->
            <div class="flow">
              <div class="flow-item">用户/前端</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">API 层</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">Orchestrator</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">Prompt Builder</div>
            </div>
            <div class="flow">
              <div class="flow-item">LLM</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">工具执行层</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">存储/监控</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">SSE/最终回复</div>
            </div>
            <p class="hint">请求：POST /wunder（user_id, question, tool_names, stream）</p>
            <div class="note">
              <strong>理念：</strong>对开发者一切是接口，对模型一切皆工具
            </div>
          </div>
        </div>
      </section>
  `);
}
