"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="能力底座总览">
  <div class="slide-meta">
    <span class="section-tag">第3节 能力底座</span>
    <div class="section-map">
      <a class="section-chip active" href="#8">总览</a>
      <a class="section-chip" href="#9">工作区</a>
      <a class="section-chip" href="#10">提示词治理</a>
      <a class="section-chip" href="#11">记忆压缩</a>
      <a class="section-chip" href="#12">蜂群协作</a>
    </div>
  </div>
  <h2>能力底座总览：统一工具编排，统一复用方式</h2>
  <p class="section-lead">扩展能力不再散落，而是纳入同一调用协议</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">工具体系</span>
      <ul>
        <li>内置工具 / MCP / Skills / 知识库</li>
        <li>用户自建工具与共享工具</li>
        <li>A2A 服务工具与跨系统协作工具</li>
      </ul>
      <span class="pill">编排价值</span>
      <ul>
        <li>统一发现、统一调用、统一权限控制</li>
        <li>能力可持续复用，不依赖个人记忆</li>
      </ul>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/capability-foundation-map.svg" alt="能力底座工具编排示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
