"use strict";

import { createSlide } from "./utils.js";

// 第 3 页：核心理念，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="核心理念">
        <div class="slide-meta">
          <span class="section-tag">第1节 核心理念</span>
          <div class="section-map">
            <span class="section-chip active">核心理念</span>
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
            <span class="pill">执行链路</span>
            <div class="flow">
              <div class="flow-item">提问</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">任务规划</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">工具执行</div>
              <div class="flow-arrow">→</div>
              <div class="flow-item">产物沉淀</div>
            </div>
            <div class="note">
              <strong>理念：</strong>对开发者一切是接口，对模型一切皆工具
            </div>
          </div>
        </div>
      </section>
  `);
}
