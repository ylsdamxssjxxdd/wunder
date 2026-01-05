"use strict";

import { createSlide } from "./utils.js";

// 第 13 页：前沿特性（记忆与上下文压缩），用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="前沿特性">
        <div class="slide-meta">
          <span class="section-tag">第4节 前沿特性</span>
          <div class="section-map">
            <a class="section-chip active" href="#13">记忆与压缩</a>
            <a class="section-chip" href="#14">A2UI</a>
            <a class="section-chip" href="#15">A2A</a>
          </div>
        </div>
        <h2>前沿特性：记忆与上下文压缩</h2>
        <p class="section-lead">长对话稳定运行，关键结论可延续</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">上下文压缩</span>
            <ul>
              <li>触发：上下文占用达到阈值</li>
              <li>保留系统提示词与近期消息</li>
              <li>生成结构化摘要继续对话</li>
            </ul>
            <span class="pill">长期记忆</span>
            <ul>
              <li>最终回复后自动总结</li>
              <li>按时间戳写入长期记忆</li>
              <li>后续会话注入 [长期记忆]</li>
            </ul>
            <span class="pill">口径说明</span>
            <p>统计的是上下文占用 token，不是总消耗</p>
          </div>
          <div class="card media-panel is-image stack">
            <img src="assets/feature-memory.svg" alt="记忆与上下文压缩示意图" />
          </div>
        </div>
      </section>
  `);
}
