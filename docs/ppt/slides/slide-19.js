"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="运行形态策略">
  <div class="slide-meta">
    <span class="section-tag">第6节 运行与落地</span>
    <div class="section-map">
      <a class="section-chip active" href="#19">运行形态</a>
      <a class="section-chip" href="#20">落地路线</a>
    </div>
  </div>
  <h2>运行策略：server / cli / desktop 选型协同</h2>
  <p class="section-lead">同一核心能力，按场景选择最合适运行形态</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">server（平台部署）</span>
      <p>面向多用户协作与组织治理，主存储为 PostgreSQL。</p>
      <span class="pill">cli（本地开发）</span>
      <p>面向开发调试与自动化，默认持久化到 WUNDER_TEMP/。</p>
      <span class="pill">desktop（本地可视化）</span>
      <p>Tauri + 本地桥接，默认持久化到 WUNDER_TEMPD/。</p>
      <div class="note"><strong>原则：</strong>先选场景，再选形态，避免过度部署。</div>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/runtime-modes-selection.svg" alt="三种运行形态与持久化示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
