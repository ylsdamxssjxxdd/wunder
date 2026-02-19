"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="架构总览">
  <div class="slide-meta">
    <span class="section-tag">第2节 主链路与并发模型</span>
    <div class="section-map">
      <a class="section-chip active" href="#4">架构总览</a>
      <a class="section-chip" href="#5">请求链路</a>
      <a class="section-chip" href="#6">流式恢复</a>
      <a class="section-chip" href="#7">并发模型</a>
    </div>
  </div>
  <h2>重构后架构：入口层 × 调度层 × 能力层 × 治理层</h2>
  <p class="section-lead">主链路稳定与能力扩展，都通过统一分层实现</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">入口层</span>
      <p>frontend / web / cli / desktop / channel / gateway</p>
      <span class="pill">调度层</span>
      <p>AgentRuntime + Orchestrator 统一编排模型与工具</p>
      <span class="pill">能力层</span>
      <p>工具系统、知识库、工作区、记忆、蜂群协作</p>
      <span class="pill">治理层</span>
      <p>权限、安全、配额、监控、压测与评估</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/architecture-four-layers.svg" alt="系统四层架构示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
