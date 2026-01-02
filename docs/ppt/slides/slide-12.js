"use strict";

import { createSlide } from "./utils.js";

// 第 12 页：智能体功能演示，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="智能体功能演示">
        <div class="slide-meta">
          <span class="section-tag">第3节 工作区</span>
          <div class="section-map">
            <span class="section-chip">工作区</span>
            <span class="section-chip active">功能演示</span>
          </div>
        </div>
        <h2>智能体功能演示：画爱心并保存</h2>
        <p class="section-lead">证明“工具 + 工作区”闭环</p>
        <div class="grid three">
          <div class="card">
            <h3>1. 提问</h3>
            <p>请用 Python 画一颗爱心</p>
          </div>
          <div class="card">
            <h3>2. 执行</h3>
            <p>生成图片并保存到临时文件区</p>
          </div>
          <div class="card">
            <h3>3. 下载</h3>
            <p>用户将结果下载到本地</p>
          </div>
        </div>
        <div class="note">
          <strong>结果：</strong>从一句话到可交付文件
        </div>
      </section>
  `);
}
