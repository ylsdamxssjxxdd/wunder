"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="蜂群协作">
  <div class="slide-meta">
    <span class="section-tag">第3节 能力底座</span>
    <div class="section-map">
      <a class="section-chip" href="#8">总览</a>
      <a class="section-chip" href="#9">工作区</a>
      <a class="section-chip" href="#10">提示词治理</a>
      <a class="section-chip" href="#11">记忆压缩</a>
      <a class="section-chip active" href="#12">蜂群协作</a>
    </div>
  </div>
  <h2>蜂群协作：TeamRun / TeamTask 并行收敛</h2>
  <p class="section-lead">复杂任务可拆分并行执行，再统一归并输出</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">触发方式</span>
      <p>会话内通过 agent_swarm 工具创建 TeamRun。</p>
      <span class="pill">执行过程</span>
      <p>母任务拆解 TeamTask，子智能体并行执行后汇总。</p>
      <span class="pill">可观测性</span>
      <p>team_* 事件实时回传，前端可展示任务进度面板。</p>
      <span class="pill">当前策略</span>
      <p>保留 hive_id 作用域能力，产品侧默认单蜂巢 default。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/swarm-teamrun-flow.svg" alt="蜂群协作 TeamRun 流程示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
