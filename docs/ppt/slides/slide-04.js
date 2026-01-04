"use strict";

import { createSlide } from "./utils.js";

// 第 4 页：运行流程图，用于展示 /wunder 从请求到回复的链路。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="运行流程">
        <div class="slide-meta">
          <span class="section-tag">第1节 核心理念</span>
          <div class="section-map">
            <a class="section-chip" href="#3">核心理念</a>
            <a class="section-chip active" href="#4">运行流程</a>
          </div>
        </div>
        <h2>从请求到回复</h2>
        <p class="section-lead">一次提问贯穿“理解 → 调用 → 产出”</p>
        <img
          class="hero-image"
          src="assets/02-request-flow.svg"
          alt="wunder 运行流程图"
        />
        <p class="hint">请求：POST /wunder（user_id, question, tool_names, stream）</p>
      </section>
  `);
}
