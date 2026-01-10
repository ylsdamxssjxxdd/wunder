"use strict";

(() => {

// 通过全局命名空间获取工具函数与注册器，避免模块加载带来的 CORS 限制。
const { createSlide, registerSlide } = window.WunderPpt;

// 第 1 页：欢迎页 + 目录页（合并展示），用于拆分维护本页内容。
function buildSlide() {
  return createSlide(`
<section class="slide toc intro-split" data-title="欢迎">
        <div class="grid two toc-split">
          <div class="stack lg cover-intro">
            <h1>wunder 智能体调度平台</h1>
            <p class="subtitle">让大模型从“会聊”走向“会做事”</p>
          </div>
          <div class="stack toc-panel">
            <div class="eyebrow">目录</div>
            <div class="toc-grid">
              <a class="toc-item toc-link" href="#2">
                <div class="toc-index">01</div>
                <div>
                  <div class="toc-title">核心理念</div>
                  <div class="toc-desc">从“会聊”到“会做事”</div>
                </div>
              </a>
              <a class="toc-item toc-link" href="#4">
                <div class="toc-index">02</div>
                <div>
                  <div class="toc-title">工具体系</div>
                  <div class="toc-desc">六类工具与统一治理</div>
                </div>
              </a>
              <a class="toc-item toc-link" href="#11">
                <div class="toc-index">03</div>
                <div>
                  <div class="toc-title">工作区</div>
                  <div class="toc-desc">产出沉淀与可复用资产</div>
                </div>
              </a>
              <a class="toc-item toc-link" href="#13">
                <div class="toc-index">04</div>
                <div>
                  <div class="toc-title">前沿特性</div>
                  <div class="toc-desc">记忆/压缩 + A2UI + A2A</div>
                </div>
              </a>
              <a class="toc-item toc-link" href="#16">
                <div class="toc-index">05</div>
                <div>
                  <div class="toc-title">智能体管理测试功能</div>
                  <div class="toc-desc">线程管理 / 吞吐量测试 / 能力评估</div>
                </div>
              </a>
              <a class="toc-item toc-link" href="#20">
                <div class="toc-index">06</div>
                <div>
                  <div class="toc-title">快速开始</div>
                  <div class="toc-desc">从一个场景做起</div>
                </div>
              </a>
            </div>
          </div>
        </div>
      </section>
  `);
}

// 注册页面构建函数，保持与清单一致的加载顺序。
registerSlide(buildSlide);


})();
