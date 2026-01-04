"use strict";

import { createSlide } from "./utils.js";

// 第 14 页：A2UI，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="A2UI">
        <div class="slide-meta">
          <span class="section-tag">第4节 前沿特性</span>
          <div class="section-map">
            <a class="section-chip" href="#13">记忆与压缩</a>
            <a class="section-chip active" href="#14">A2UI</a>
            <a class="section-chip" href="#15">A2A</a>
          </div>
        </div>
        <h2>A2UI：让回答直接变成界面</h2>
        <p class="section-lead">结构化输出，前端可直接渲染</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">是什么</span>
            <ul>
              <li>模型输出 A2UI JSON 消息</li>
              <li>前端渲染卡片、表单、按钮</li>
              <li>结构化展示过程与结果</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">接入方式</span>
            <ul>
              <li>显式启用 a2ui 工具</li>
              <li>SSE 输出 a2ui 事件</li>
              <li>前端按组件规范渲染</li>
            </ul>
            <span class="pill">价值</span>
            <p>降低二次开发成本，体验更直观</p>
          </div>
        </div>
      </section>
  `);
}
