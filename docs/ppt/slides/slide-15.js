"use strict";

(() => {

// 通过全局命名空间获取工具函数与注册器，避免模块加载带来的 CORS 限制。
const { createSlide, registerSlide } = window.WunderPpt;

// 第 15 页：A2A 标准接口，用于拆分维护本页内容。
function buildSlide() {
  return createSlide(`
<section class="slide" data-title="A2A 标准接口">
        <div class="slide-meta">
          <span class="section-tag">第4节 前沿特性</span>
          <div class="section-map">
            <a class="section-chip" href="#13">记忆与压缩</a>
            <a class="section-chip" href="#14">A2UI</a>
            <a class="section-chip active" href="#15">A2A</a>
          </div>
        </div>
        <h2>A2A：对外标准化智能体接口</h2>
        <p class="section-lead">JSON-RPC + SSE，让智能体能力可互通</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">接口能力</span>
            <ul>
              <li>SendMessage / SendStreamingMessage</li>
              <li>GetTask / ListTasks / SubscribeToTask</li>
              <li>AgentCard 服务发现</li>
            </ul>
            <span class="pill">接入方式</span>
            <ul>
              <li>入口：/a2a（JSON-RPC 2.0）</li>
              <li>发现：/.well-known/agent-card.json</li>
              <li>鉴权：API Key 统一校验</li>
            </ul>
            <span class="pill">价值</span>
            <p>统一跨系统调用，复用 Wunder 工具链</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/feature-a2a.svg" alt="A2A 标准接口示意图" />
          </div>
        </div>
      </section>
  `);
}

// 注册页面构建函数，保持与清单一致的加载顺序。
registerSlide(buildSlide);


})();
