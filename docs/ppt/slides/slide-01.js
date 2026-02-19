"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide toc intro-split" data-title="欢迎">
  <div class="grid two toc-split">
    <div class="stack lg cover-intro">
      <h1>wunder 智能体调度系统（重构版）</h1>
      <p class="subtitle">从“会回答”到“可执行、可治理、可持续交付”</p>
    </div>
    <div class="stack toc-panel">
      <div class="eyebrow">目录</div>
      <div class="toc-grid">
        <a class="toc-item toc-link" href="#2">
          <div class="toc-index">01</div>
          <div>
            <div class="toc-title">重构定位</div>
            <div class="toc-desc">为什么升级、升级成什么</div>
          </div>
        </a>
        <a class="toc-item toc-link" href="#4">
          <div class="toc-index">02</div>
          <div>
            <div class="toc-title">主链路与并发</div>
            <div class="toc-desc">请求执行与流式恢复</div>
          </div>
        </a>
        <a class="toc-item toc-link" href="#8">
          <div class="toc-index">03</div>
          <div>
            <div class="toc-title">能力底座</div>
            <div class="toc-desc">工具、工作区、记忆、蜂群</div>
          </div>
        </a>
        <a class="toc-item toc-link" href="#13">
          <div class="toc-index">04</div>
          <div>
            <div class="toc-title">多入口互通</div>
            <div class="toc-desc">多端接入与跨系统协作</div>
          </div>
        </a>
        <a class="toc-item toc-link" href="#16">
          <div class="toc-index">05</div>
          <div>
            <div class="toc-title">治理与稳定</div>
            <div class="toc-desc">权限、安全、观测、评估</div>
          </div>
        </a>
        <a class="toc-item toc-link" href="#19">
          <div class="toc-index">06</div>
          <div>
            <div class="toc-title">运行与落地</div>
            <div class="toc-desc">三形态选型与试点路线</div>
          </div>
        </a>
      </div>
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
