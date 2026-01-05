"use strict";

import { createSlide } from "./utils.js";

// 第 16 页：智能体线程管理，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="智能体线程管理">
        <div class="slide-meta">
          <span class="section-tag">第5节 智能体线程管理</span>
          <div class="section-map">
            <a class="section-chip active" href="#16">线程管理</a>
          </div>
        </div>
        <h2>智能体线程管理：稳定可控</h2>
        <p class="section-lead">任务并发时，保障执行稳定</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">生命周期</span>
            <p>running → finished / error / cancelled</p>
            <span class="pill">并发规则</span>
            <p>同一用户单线程，冲突即时提示</p>
            <span class="pill">监控与取消</span>
            <p>过程可追踪，卡住可终止</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/monitor-thread.svg" alt="线程监控示意图" />
          </div>
        </div>
      </section>
  `);
}
