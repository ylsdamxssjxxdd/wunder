"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="观测评测">
  <div class="slide-meta">
    <span class="section-tag">第5节 治理与稳定性</span>
    <div class="section-map">
      <a class="section-chip" href="#16">组织治理</a>
      <a class="section-chip" href="#17">安全边界</a>
      <a class="section-chip active" href="#18">观测评测</a>
    </div>
  </div>
  <h2>可观测与评测：监控、压测、评估闭环</h2>
  <p class="section-lead">让“系统改了是否更好”具备可量化依据</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">运行观测</span>
      <p>MonitorState + stream_events 覆盖执行事件、状态与回放。</p>
      <span class="pill">吞吐压测</span>
      <p>并发压测输出 QPS / 延迟 / 资源占用与性能基线。</p>
      <span class="pill">能力评估</span>
      <p>统一用例、评分与 SSE 进度流，支持回归对比。</p>
      <span class="pill">仿真实验</span>
      <p>sim_lab 支持批量运行与状态跟踪，降低上线试错成本。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/observability-evaluation-loop.svg" alt="可观测与评测体系示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
