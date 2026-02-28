"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="多入口协同">
  <div class="slide-meta">
    <span class="section-tag">第4节 多入口与外部互通</span>
    <div class="section-map">
      <a class="section-chip active" href="#13">多入口协同</a>
      <a class="section-chip" href="#14">渠道多模态</a>
      <a class="section-chip" href="#15">外部互通</a>
    </div>
  </div>
  <h2>多入口协同：同一核心服务不同角色</h2>
  <p class="section-lead">用户、管理员与开发者都复用同一调度底座</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">用户入口</span>
      <p>frontend 与 desktop 统一 Messenger 壳（/app/home、/desktop/home），承载会话、用户世界、工具与文件。</p>
      <span class="pill">管理入口</span>
      <p>web 负责模型、工具、权限、监控与评估治理。</p>
      <span class="pill">开发入口</span>
      <p>cli 面向本地开发、自动化与调试回放。</p>
      <span class="pill">端云协同入口</span>
      <p>desktop 可通过 remote_gateway 渠道接入云端 /wunder，失败自动回退本地。</p>
      <span class="pill">统一执行</span>
      <p>不同入口最终都回到同一 /wunder 调度链路。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/multi-entry-collaboration.svg" alt="多入口协同示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
