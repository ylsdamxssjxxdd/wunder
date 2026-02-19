"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="渠道与多模态">
  <div class="slide-meta">
    <span class="section-tag">第4节 多入口与外部互通</span>
    <div class="section-map">
      <a class="section-chip" href="#13">多入口协同</a>
      <a class="section-chip active" href="#14">渠道多模态</a>
      <a class="section-chip" href="#15">外部互通</a>
    </div>
  </div>
  <h2>渠道与多模态：消息接入即业务入口</h2>
  <p class="section-lead">统一 webhook + 绑定路由 + outbox 重试，支撑跨平台交付</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">接入范围</span>
      <ul>
        <li>飞书、WhatsApp Cloud、QQ Bot 等渠道</li>
        <li>账号绑定后自动映射到用户与会话</li>
      </ul>
      <span class="pill">处理链路</span>
      <ul>
        <li>入站校验 → 会话路由 → 调度执行</li>
        <li>出站进入 channel_outbox 异步重试投递</li>
      </ul>
      <span class="pill">多模态能力</span>
      <p>支持语音转写、图片识别、地理描述与语音回包。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/channel-multimodal-pipeline.svg" alt="渠道接入与多模态处理示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
