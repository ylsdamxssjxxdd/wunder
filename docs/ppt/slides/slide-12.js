"use strict";

import { createSlide } from "./utils.js";

// 第 12 页：智能体功能演示，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="智能体功能演示">
        <div class="slide-meta">
          <span class="section-tag">第3节 工作区</span>
          <div class="section-map">
            <a class="section-chip" href="#11">工作区</a>
            <a class="section-chip active" href="#12">功能演示</a>
          </div>
        </div>
        <h2>智能体功能演示：画爱心并保存</h2>
        <p class="section-lead">证明“工具 + 工作区”闭环</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">演示步骤</span>
            <ul>
              <li>提问：请用 Python 画一颗爱心</li>
              <li>执行：生成图片并保存到临时文件区</li>
              <li>下载：用户将结果保存到本地</li>
            </ul>
            <div class="note">
              <strong>结果：</strong>从一句话到可交付文件
            </div>
          </div>
          <div class="card media-panel stack">
            <h3>图片占位</h3>
            <p>建议：爱心产物截图或下载结果截图</p>
            <span class="tag">assets/demo-heart.png</span>
          </div>
        </div>
      </section>
  `);
}
