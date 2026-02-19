"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="外部互通">
  <div class="slide-meta">
    <span class="section-tag">第4节 多入口与外部互通</span>
    <div class="section-map">
      <a class="section-chip" href="#13">多入口协同</a>
      <a class="section-chip" href="#14">渠道多模态</a>
      <a class="section-chip active" href="#15">外部互通</a>
    </div>
  </div>
  <h2>外部互通：A2A / MCP / Gateway / External Auth</h2>
  <p class="section-lead">让 wunder 从内部平台延展为跨系统能力节点</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">A2A</span>
      <p>/a2a 提供 JSON-RPC + SSE，AgentCard 负责能力发现。</p>
      <span class="pill">MCP</span>
      <p>/wunder/mcp 支持自托管工具服务与跨系统调用。</p>
      <span class="pill">Gateway</span>
      <p>/wunder/gateway/ws 承担 operator/node/control plane 协作。</p>
      <span class="pill">External Auth</span>
      <p>/wunder/auth/external/* 支持外部系统免登录嵌入接入。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/external-interoperability-matrix.svg" alt="A2A MCP Gateway 外部互通示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
