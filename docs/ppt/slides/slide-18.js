"use strict";

(() => {

// 通过全局命名空间获取工具函数与注册器，避免模块加载带来的 CORS 限制。
const { createSlide, registerSlide } = window.WunderPpt;

// 第 18 页：吞吐量测试，用于拆分维护本页内容。
function buildSlide() {
  return createSlide(`
<section class="slide" data-title="吞吐量测试">
        <div class="slide-meta">
          <span class="section-tag">第5节 智能体管理测试功能</span>
          <div class="section-map">
            <a class="section-chip" href="#16">总览</a>
            <a class="section-chip" href="#17">线程管理</a>
            <a class="section-chip active" href="#18">吞吐量测试</a>
            <a class="section-chip" href="#19">能力评估</a>
          </div>
        </div>
        <h2>吞吐量测试：并发压测与资源画像</h2>
        <p class="section-lead">模拟真实负载，量化 QPS / 延迟 / 资源占用</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">场景配置</span>
            <p>并发数、持续时长、流量模型</p>
            <span class="pill">压测指标</span>
            <p>QPS / P95 延迟 / 错误率</p>
            <span class="pill">结果产出</span>
            <p>性能基线与瓶颈定位</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/agent-throughput-testing.svg" alt="吞吐量测试示意图" />
          </div>
        </div>
      </section>
  `);
}

// 注册页面构建函数，保持与清单一致的加载顺序。
registerSlide(buildSlide);


})();
