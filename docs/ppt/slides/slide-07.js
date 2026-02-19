"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="并发模型">
  <div class="slide-meta">
    <span class="section-tag">第2节 主链路与并发模型</span>
    <div class="section-map">
      <a class="section-chip" href="#4">架构总览</a>
      <a class="section-chip" href="#5">请求链路</a>
      <a class="section-chip" href="#6">流式恢复</a>
      <a class="section-chip active" href="#7">并发模型</a>
    </div>
  </div>
  <h2>并发控制：主线程、队列与轮次统计</h2>
  <p class="section-lead">把“可并发”与“可治理”放在同一机制里</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">主线程机制</span>
      <p>按 user_id + agent_id 维护主线程与主会话。</p>
      <span class="pill">队列机制</span>
      <p>忙时进入 agent_tasks，由 AgentRuntime 异步调度执行。</p>
      <span class="pill">轮次口径</span>
      <ul>
        <li>用户轮次：每条用户消息 +1</li>
        <li>模型轮次：每次模型/工具/最终回复动作 +1</li>
      </ul>
      <p class="hint">token 统计口径为“上下文占用量”，不是总消耗量。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/concurrency-thread-queue.svg" alt="主线程与队列并发模型示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
