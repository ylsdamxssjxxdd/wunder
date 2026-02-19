"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="落地路线">
  <div class="slide-meta">
    <span class="section-tag">第6节 运行与落地</span>
    <div class="section-map">
      <a class="section-chip" href="#19">运行形态</a>
      <a class="section-chip active" href="#20">落地路线</a>
    </div>
  </div>
  <h2>落地路线：从一个高频场景开始，逐步平台化</h2>
  <p class="section-lead">先跑通闭环，再复制到更多业务线</p>
  <div class="grid three">
    <div class="card">
      <h3>1. 选场景</h3>
      <p>优先“高频、标准化、可量化”的任务。</p>
    </div>
    <div class="card">
      <h3>2. 配能力</h3>
      <p>工具 + 知识 + 提示词 + 权限一起设计。</p>
    </div>
    <div class="card">
      <h3>3. 固化流程</h3>
      <p>接入监控评估，沉淀为可复用模板。</p>
    </div>
  </div>
  <div class="card media-panel is-image stack fill">
    <img src="assets/rollout-roadmap.svg" alt="wunder 落地路线图" />
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
