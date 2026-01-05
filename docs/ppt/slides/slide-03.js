"use strict";

(() => {

// 通过全局命名空间获取工具函数与注册器，避免模块加载带来的 CORS 限制。
const { createSlide, registerSlide } = window.WunderPpt;

// 第 3 页：运行流程图，用于展示 /wunder 从请求到回复的链路。
function buildSlide() {
  return createSlide(`
<section class="slide" data-title="运行流程">
        <div class="slide-meta">
          <span class="section-tag">第1节 核心理念</span>
          <div class="section-map">
            <a class="section-chip" href="#2">核心理念</a>
            <a class="section-chip active" href="#3">运行流程</a>
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

// 注册页面构建函数，保持与清单一致的加载顺序。
registerSlide(buildSlide);


})();
