"use strict";

import { createSlide } from "./utils.js";

// 第 14 页：长期记忆，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="长期记忆">
        <div class="slide-meta">
          <span class="section-tag">第5节 长期记忆</span>
          <div class="section-map">
            <span class="section-chip active">长期记忆</span>
          </div>
        </div>
        <h2>长期记忆：跨会话保持一致</h2>
        <p class="section-lead">把关键结论延续到下一次协作</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">生成方式</span>
            <ul>
              <li>最终回复完成后自动总结</li>
              <li>写入长期记忆记录（最多 30 条）</li>
              <li>带时间戳便于回溯</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">使用与控制</span>
            <ul>
              <li>后续会话自动注入 [长期记忆]</li>
              <li>可开关、可清空、可删除</li>
              <li>让偏好与约束持续一致</li>
            </ul>
          </div>
        </div>
      </section>
  `);
}
