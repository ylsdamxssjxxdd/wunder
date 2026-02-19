"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="提示词治理">
  <div class="slide-meta">
    <span class="section-tag">第3节 能力底座</span>
    <div class="section-map">
      <a class="section-chip" href="#8">总览</a>
      <a class="section-chip" href="#9">工作区</a>
      <a class="section-chip active" href="#10">提示词治理</a>
      <a class="section-chip" href="#11">记忆压缩</a>
      <a class="section-chip" href="#12">蜂群协作</a>
    </div>
  </div>
  <h2>提示词治理：模板包、双语目录、形态分段</h2>
  <p class="section-lead">提示词从“散文件”升级为可管理配置资产</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">模板包机制</span>
      <p>prompt_templates.active 决定生效包，支持管理端切换。</p>
      <span class="pill">双语目录</span>
      <p>提示词按 prompts/zh 与 prompts/en 维护，减少跨语言歧义。</p>
      <span class="pill">形态分段</span>
      <p>运行环境段区分 server 与 cli/desktop，保证上下文口径一致。</p>
      <span class="pill">治理原则</span>
      <p>默认模板包只读，鼓励新建包演进并保留可追踪版本。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/prompt-template-governance.svg" alt="提示词模板治理示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
