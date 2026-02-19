"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="工作区资产沉淀">
  <div class="slide-meta">
    <span class="section-tag">第3节 能力底座</span>
    <div class="section-map">
      <a class="section-chip" href="#8">总览</a>
      <a class="section-chip active" href="#9">工作区</a>
      <a class="section-chip" href="#10">提示词治理</a>
      <a class="section-chip" href="#11">记忆压缩</a>
      <a class="section-chip" href="#12">蜂群协作</a>
    </div>
  </div>
  <h2>工作区：把执行结果沉淀成长期资产</h2>
  <p class="section-lead">输出不止是文本，而是可复用文件与过程记录</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">隔离与持久化</span>
      <ul>
        <li>默认路径：/workspaces/&lt;user_id&gt;</li>
        <li>可按 sandbox_container_id 继续隔离空间</li>
      </ul>
      <span class="pill">沉淀内容</span>
      <ul>
        <li>文档、脚本、产物与中间结果</li>
        <li>会话历史、工具日志与事件轨迹</li>
      </ul>
      <div class="note"><strong>价值：</strong>跨会话继续复用，避免重复劳动。</div>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/workspace-asset-lifecycle.svg" alt="工作区与资产沉淀示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
