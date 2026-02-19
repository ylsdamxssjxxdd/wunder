"use strict";

(() => {

const { createSlide, registerSlide } = window.WunderPpt;

function buildSlide() {
  return createSlide(`
<section class="slide" data-title="请求主链路">
  <div class="slide-meta">
    <span class="section-tag">第2节 主链路与并发模型</span>
    <div class="section-map">
      <a class="section-chip" href="#4">架构总览</a>
      <a class="section-chip active" href="#5">请求链路</a>
      <a class="section-chip" href="#6">流式恢复</a>
      <a class="section-chip" href="#7">并发模型</a>
    </div>
  </div>
  <h2>/wunder 主链路：从请求到最终回复</h2>
  <p class="section-lead">一次调用可完整穿过“理解→执行→沉淀”闭环</p>
  <div class="grid two">
    <div class="card stack">
      <span class="pill">执行步骤</span>
      <ul>
        <li>请求进入 AgentRuntime，绑定会话与主线程</li>
        <li>Orchestrator 拉取历史、构建上下文、调用模型</li>
        <li>按需触发工具调用与知识检索</li>
        <li>流式输出中间事件与最终回复</li>
        <li>会话状态、事件与产物写回存储</li>
      </ul>
      <div class="note"><strong>结果：</strong>不仅有答案，还有完整执行轨迹。</div>
    </div>
    <div class="card media-panel is-image stack">
      <img src="assets/request-lifecycle.svg" alt="请求生命周期示意图" />
    </div>
  </div>
  <p class="hint">输入核心字段：user_id / question / tool_names / stream</p>
</section>
  `);
}

registerSlide(buildSlide);

})();
