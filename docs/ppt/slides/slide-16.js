"use strict";

(() => {

// 通过全局命名空间获取工具函数与注册器，避免模块加载带来的 CORS 限制。
const { createSlide, registerSlide } = window.WunderPpt;

// 第 16 页：智能体管理测试功能，用于拆分维护本页内容。
function buildSlide() {
  return createSlide(`
<section class="slide" data-title="智能体管理测试功能">
        <div class="slide-meta">
          <span class="section-tag">第5节 智能体管理测试功能</span>
          <div class="section-map">
            <a class="section-chip active" href="#16">总览</a>
            <a class="section-chip" href="#17">线程管理</a>
            <a class="section-chip" href="#18">吞吐量测试</a>
            <a class="section-chip" href="#19">能力评估</a>
          </div>
        </div>
        <h2>智能体管理测试功能：总览</h2>
        <p class="section-lead">第 5 节共四页：线程管理 / 吞吐量测试 / 能力评估</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">线程管理</span>
            <p>生命周期可观测，冲突可提示、可取消</p>
            <span class="pill">吞吐量测试</span>
            <p>并发压测与 QPS 指标，关注资源占用</p>
            <span class="pill">能力评估</span>
            <p>成功率与质量评分，支持回归对比</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/agent-management-overview.svg" alt="管理测试总览示意图" />
          </div>
        </div>
      </section>
  `);
}

// 注册页面构建函数，保持与清单一致的加载顺序。
registerSlide(buildSlide);


})();
