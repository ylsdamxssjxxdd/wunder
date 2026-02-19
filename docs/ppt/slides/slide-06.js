"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="流式恢复">
  <div class="slide-meta">
    <span class="section-tag">第2节 主链路与并发模型</span>
    <div class="section-map">
      <a class="section-chip" href="#4">架构总览</a>
      <a class="section-chip" href="#5">请求链路</a>
      <a class="section-chip active" href="#6">流式恢复</a>
      <a class="section-chip" href="#7">并发模型</a>
    </div>
  </div>
  <h2>流式策略：WebSocket 优先，SSE 兜底</h2>
  <p class="section-lead">兼顾低延迟实时体验与兼容性恢复能力</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">协议选择</span>
      <ul>
        <li>默认首选 WebSocket（/wunder/ws 与 /wunder/chat/ws）</li>
        <li>兼容通道保留 SSE，老客户端可平滑接入</li>
      </ul>
      <span class="pill">断线恢复</span>
      <ul>
        <li>事件写入 stream_events，支持 after_event_id 回放</li>
        <li>慢客户端与短暂断连可自动补齐关键事件</li>
      </ul>
      <div class="note"><strong>目标：</strong>网络波动不打断业务任务执行。</div>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/streaming-ws-sse.svg" alt="WebSocket 与 SSE 双通道示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
