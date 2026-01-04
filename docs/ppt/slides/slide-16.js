"use strict";

import { createSlide } from "./utils.js";

// 第 16 页：智能体线程管理，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="智能体线程管理">
        <div class="slide-meta">
          <span class="section-tag">第5节 智能体线程管理</span>
          <div class="section-map">
            <span class="section-chip active">线程管理</span>
          </div>
        </div>
        <h2>智能体线程管理：稳定可控</h2>
        <p class="section-lead">任务并发时，保障执行稳定</p>
        <div class="grid three">
          <div class="card">
            <h3>生命周期</h3>
            <p>running → finished / error / cancelled</p>
          </div>
          <div class="card">
            <h3>并发规则</h3>
            <p>同一用户单线程，冲突即时提示</p>
          </div>
          <div class="card">
            <h3>监控与取消</h3>
            <p>过程可追踪，卡住可终止</p>
          </div>
        </div>
      </section>
  `);
}
