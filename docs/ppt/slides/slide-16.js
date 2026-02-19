"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="组织治理">
  <div class="slide-meta">
    <span class="section-tag">第5节 治理与稳定性</span>
    <div class="section-map">
      <a class="section-chip active" href="#16">组织治理</a>
      <a class="section-chip" href="#17">安全边界</a>
      <a class="section-chip" href="#18">观测评测</a>
    </div>
  </div>
  <h2>组织治理：用户、单位、权限与配额</h2>
  <p class="section-lead">把“谁能做什么、能用多少”变成平台规则</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">组织模型</span>
      <p>支持组织树、用户账号、角色权限与层级管理。</p>
      <span class="pill">访问控制</span>
      <p>工具/智能体可按用户维度配置 allow/block 策略。</p>
      <span class="pill">配额治理</span>
      <p>注册用户按日额度控制，0 点重置并可视化展示。</p>
      <span class="pill">身份区分</span>
      <p>线程用户可虚构；注册用户由管理端控制治理策略。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/org-governance-model.svg" alt="组织治理与权限配额示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
