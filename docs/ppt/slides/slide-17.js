"use strict";

import { createSlide } from "./utils.js";

// 第 17 页：快速开始，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="快速开始">
        <div class="slide-meta">
          <span class="section-tag">第6节 快速开始</span>
          <div class="section-map">
            <span class="section-chip active">快速开始</span>
            <span class="section-chip">收尾</span>
          </div>
        </div>
        <h2>快速开始：三步落地</h2>
        <p class="section-lead">从一个高频场景做起，快速见效</p>
        <div class="grid three">
          <div class="card">
            <h3>1. 选场景</h3>
            <p>挑一个需求高频且明确的场景</p>
          </div>
          <div class="card">
            <h3>2. 选工具组合</h3>
            <p>工具 + 知识库 + Skills 搭配</p>
          </div>
          <div class="card">
            <h3>3. 固化流程</h3>
            <p>把成功经验沉淀为流程模板</p>
          </div>
        </div>
        <div class="card soft stack">
          <span class="pill">试点示例</span>
          <ul>
            <li>制度/流程问答（知识库）</li>
            <li>周报/纪要生成（Skills）</li>
            <li>资料整理与批量处理（内置工具）</li>
          </ul>
        </div>
      </section>
  `);
}
