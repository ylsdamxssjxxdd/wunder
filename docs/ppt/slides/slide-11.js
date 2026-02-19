"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="记忆与压缩">
  <div class="slide-meta">
    <span class="section-tag">第3节 能力底座</span>
    <div class="section-map">
      <a class="section-chip" href="#8">总览</a>
      <a class="section-chip" href="#9">工作区</a>
      <a class="section-chip" href="#10">提示词治理</a>
      <a class="section-chip active" href="#11">记忆压缩</a>
      <a class="section-chip" href="#12">蜂群协作</a>
    </div>
  </div>
  <h2>长会话续航：上下文压缩 + 长期记忆</h2>
  <p class="section-lead">把对话长度风险转化为可控的上下文治理</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">上下文压缩</span>
      <ul>
        <li>达到阈值后自动触发结构化摘要</li>
        <li>保留关键轨迹，降低上下文膨胀</li>
      </ul>
      <span class="pill">长期记忆</span>
      <ul>
        <li>最终回复后异步总结写入记忆库</li>
        <li>后续会话按策略注入 [长期记忆]</li>
      </ul>
      <p class="hint">token 统计口径为上下文占用，不等同计费总消耗。</p>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/context-memory-compaction.svg" alt="上下文压缩与长期记忆示意图" />
    </div>
  </div>
</section>
  `);
}

registerSlide(buildSlide);

})();
