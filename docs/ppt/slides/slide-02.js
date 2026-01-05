"use strict";

(() => {

// 通过全局命名空间获取工具函数与注册器，避免模块加载带来的 CORS 限制。
const { createSlide, registerSlide } = window.WunderPpt;

// 第 2 页：核心理念，用于拆分维护本页内容。
function buildSlide() {
  return createSlide(`
<section class="slide" data-title="核心理念">
        <div class="slide-meta">
          <span class="section-tag">第1节 核心理念</span>
          <div class="section-map">
            <a class="section-chip active" href="#2">核心理念</a>
            <a class="section-chip" href="#3">运行流程</a>
          </div>
        </div>
        <h2>从“会聊”到“会做事”</h2>
        <p class="section-lead">一次提问，跑通从理解到落地的链路</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">用户看到的</span>
            <ul>
              <li>只需提出问题</li>
              <li>过程清晰可追踪</li>
              <li>结果能落成产物</li>
            </ul>
            <span class="pill">统一入口</span>
            <p>/wunder 支持流式返回过程与最终回复</p>
          </div>
          <div class="card soft stack">
            <span class="pill">核心理念</span>
            <ul>
              <li>对开发者：一切是接口（API/配置/工具）</li>
              <li>对大模型：一切皆工具（可调用、可组合、可治理）</li>
              <li>一次提问即可驱动完整执行链路</li>
            </ul>
            <div class="note">
              <strong>结果导向：</strong>让答案沉淀为可复用的产物
            </div>
          </div>
        </div>
      </section>
  `);
}

// 注册页面构建函数，保持与清单一致的加载顺序。
registerSlide(buildSlide);


})();
