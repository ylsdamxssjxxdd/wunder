"use strict";

(() => {

// 通过全局命名空间获取工具函数与注册器，避免模块加载带来的 CORS 限制。
const { createSlide, registerSlide } = window.WunderPpt;

// 第 20 页：快速开始，用于拆分维护本页内容。
function buildSlide() {
  return createSlide(`
<section class="slide" data-title="快速开始">
        <div class="slide-meta">
          <span class="section-tag">第6节 快速开始</span>
          <div class="section-map">
            <a class="section-chip active" href="#20">快速开始</a>
          </div>
        </div>
        <h2>快速开始：三步落地</h2>
        <p class="section-lead">从一个高频场景做起，快速见效</p>
        <div class="grid three">
          <div class="card">
            <h3>1. 选场景</h3>
            <p>挑一个需求高频且明确的场景</p>
          </div>
          <div class="card">
            <h3>2. 选工具组合</h3>
            <p>工具 + 知识库 + Skills 搭配</p>
          </div>
          <div class="card">
            <h3>3. 固化流程</h3>
            <p>把成功经验沉淀为流程模板</p>
          </div>
        </div>
        <div class="card media-panel is-image stack fill">
          <img src="assets/quickstart-pilots.svg" alt="试点示例示意图" />
        </div>
      </section>
  `);
}

// 注册页面构建函数，保持与清单一致的加载顺序。
registerSlide(buildSlide);


})();
