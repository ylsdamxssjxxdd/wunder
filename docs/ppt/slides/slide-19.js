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
  <h2>运行策略：desktop 优先，server / cli 协同</h2>
  <p class="section-lead">同一核心能力，按场景选择运行形态并支持端云协同部署</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">server（平台部署）</span>
      <p>面向多用户协作与组织治理，主存储为 PostgreSQL。</p>
      <span class="pill">cli（本地开发）</span>
      <p>面向开发调试与自动化，默认持久化到 WUNDER_TEMP/。</p>
      <span class="pill">desktop（主交付形态）</span>
      <p>Tauri + 本地桥接（默认 127.0.0.1:18123），默认持久化到 WUNDER_TEMPD/。</p>
      <span class="pill">端云协同</span>
      <p>本地优先执行；按需经 remote_gateway 渠道切云端，失败自动回退本地。</p>
      <div class="note"><strong>原则：</strong>先选场景，再定部署：本地可用性优先，云端治理可扩展。</div>
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
