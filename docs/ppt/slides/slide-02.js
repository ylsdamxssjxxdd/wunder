"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="重构定位-破题">
  <div class="slide-meta">
    <span class="section-tag">第1节 重构定位</span>
    <div class="section-map">
      <a class="section-chip active" href="#2">破题</a>
      <a class="section-chip" href="#3">定位</a>
    </div>
  </div>
  <h2>为什么要从聊天升级到执行系统</h2>
  <p class="section-lead">业务需要稳定交付结果，而不是一次性文本回答</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">旧范式痛点</span>
      <ul>
        <li>回答可读但难直接落地</li>
        <li>过程不可控，复盘成本高</li>
        <li>跨系统协作依赖人工拼接</li>
      </ul>
      <span class="pill">组织层风险</span>
      <p>能力停留在对话窗口，难以沉淀成长期资产</p>
    </div>
    <div class="card soft stack">
      <span class="pill">重构目标</span>
      <ul>
        <li>把“提问”升级为“可执行任务”</li>
        <li>把执行过程升级为“可观测链路”</li>
        <li>把结果升级为“可复用资产”</li>
      </ul>
      <div class="note">
        <strong>结论：</strong>wunder 的目标是交付能力平台，不是聊天壳。
      </div>
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
