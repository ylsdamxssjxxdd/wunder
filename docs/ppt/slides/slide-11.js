"use strict";

import { createSlide } from "./utils.js";

// 第 11 页：工作区，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="工作区">
        <div class="slide-meta">
          <span class="section-tag">第3节 工作区</span>
          <div class="section-map">
            <span class="section-chip active">工作区</span>
            <span class="section-chip">功能演示</span>
          </div>
        </div>
        <h2>工作区：长期的资料落脚点</h2>
        <p class="section-lead">产出不会消失，而是持续积累</p>
        <div class="grid two">
          <div class="card stack">
            <span class="pill">定位</span>
            <p>每个用户一块持久化空间</p>
            <span class="pill">路径示例</span>
            <p>data/workspaces/&lt;user_id&gt;/files</p>
            <span class="pill">沉淀内容</span>
            <ul>
              <li>文档、脚本、分析结果</li>
              <li>工具执行产物与中间文件</li>
            </ul>
          </div>
          <div class="card soft stack">
            <span class="pill">有什么用</span>
            <ul>
              <li>对话输出直接变成资产</li>
              <li>跨会话继续使用同一资料</li>
              <li>方便分享、复用、协作</li>
            </ul>
          </div>
        </div>
      </section>
  `);
}
