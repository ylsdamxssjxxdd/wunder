"use strict";

import { createSlide } from "./utils.js";

// 第 13 页：上下文压缩，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="上下文压缩">
        <div class="slide-meta">
          <span class="section-tag">第4节 上下文压缩</span>
          <div class="section-map">
            <span class="section-chip active">上下文压缩</span>
          </div>
        </div>
        <h2>上下文压缩：让长对话持续可用</h2>
        <p class="section-lead">对话越长越重，需要稳定机制</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">触发条件</span>
            <ul>
              <li>上下文占用 token 达阈值</li>
              <li>当前消息接近安全上限</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">处理方式</span>
            <ul>
              <li>保留系统提示词 + 最近消息</li>
              <li>生成结构化摘要并注入</li>
              <li>重置历史占用继续对话</li>
            </ul>
            <span class="pill">口径说明</span>
            <p>统计的是上下文占用 token，不是总消耗</p>
          </div>
        </div>
      </section>
  `);
}
