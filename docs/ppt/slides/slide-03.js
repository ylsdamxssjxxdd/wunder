"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="重构定位-一核三形态">
  <div class="slide-meta">
    <span class="section-tag">第1节 重构定位</span>
    <div class="section-map">
      <a class="section-chip" href="#2">破题</a>
      <a class="section-chip active" href="#3">定位</a>
    </div>
  </div>
  <h2>一核三形态：同一 Rust 核心，多端分发</h2>
  <p class="section-lead">desktop 为当前产品重心，server / cli 协同复用同一调度与工具体系</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">三种运行形态</span>
      <ul>
        <li><strong>server：</strong>多用户、组织治理、渠道与网关</li>
        <li><strong>cli：</strong>本地开发与自动化执行</li>
        <li><strong>desktop：</strong>通信/办公化主交付，支持端云协同接入</li>
      </ul>
      <span class="pill">能力表达框架</span>
      <p>形态协同 / 租户治理 / 智能体协作 / 工具生态 / 接口开放</p>
      <span class="pill">统一调用口径</span>
      <ul>
        <li>统一核心入口：/wunder</li>
        <li>user_id 可为虚拟标识，不要求注册</li>
      </ul>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/core-three-modes.svg" alt="一核三形态与统一核心示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
