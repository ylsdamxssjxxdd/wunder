"use strict";

(() => {

// 通过全局命名空间获取工具函数与注册器，避免模块加载带来的 CORS 限制。
const { createSlide, registerSlide } = window.WunderPpt;

// 第 19 页：能力评估，用于拆分维护本页内容。
function buildSlide() {
  return createSlide(`
<section class="slide" data-title="能力评估">
        <div class="slide-meta">
          <span class="section-tag">第5节 智能体管理测试功能</span>
          <div class="section-map">
            <a class="section-chip" href="#16">总览</a>
            <a class="section-chip" href="#17">线程管理</a>
            <a class="section-chip" href="#18">吞吐量测试</a>
            <a class="section-chip active" href="#19">能力评估</a>
          </div>
        </div>
        <h2>能力评估：质量评分与回归对比</h2>
        <p class="section-lead">统一用例、统一指标，持续衡量模型与流程</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">用例集</span>
            <p>真实场景覆盖，样本可追溯</p>
            <span class="pill">评分维度</span>
            <p>正确率 / 完整性 / 可用性</p>
            <span class="pill">回归对比</span>
            <p>版本差异一目了然</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/agent-capability-eval.svg" alt="能力评估示意图" />
          </div>
        </div>
      </section>
  `);
}

// 注册页面构建函数，保持与清单一致的加载顺序。
registerSlide(buildSlide);


})();
