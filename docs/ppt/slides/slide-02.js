"use strict";

import { createSlide } from "./utils.js";

// 第 2 页：目录页（明确章节结构），用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide toc" data-title="目录">
        <div class="eyebrow">目录</div>
        <h2>今天的 7 个部分</h2>
        <div class="toc-grid">
          <div class="toc-item">
            <div class="toc-index">01</div>
            <div>
              <div class="toc-title">核心理念</div>
              <div class="toc-desc">从“会聊”到“会做事”</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">02</div>
            <div>
              <div class="toc-title">工具体系</div>
              <div class="toc-desc">六类工具与统一治理</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">03</div>
            <div>
              <div class="toc-title">工作区</div>
              <div class="toc-desc">产出沉淀与可复用资产</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">04</div>
            <div>
              <div class="toc-title">上下文压缩</div>
              <div class="toc-desc">长对话持续可用</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">05</div>
            <div>
              <div class="toc-title">长期记忆</div>
              <div class="toc-desc">跨会话保持一致</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">06</div>
            <div>
              <div class="toc-title">智能体线程管理</div>
              <div class="toc-desc">稳定、可监控、可取消</div>
            </div>
          </div>
          <div class="toc-item">
            <div class="toc-index">07</div>
            <div>
              <div class="toc-title">快速开始</div>
              <div class="toc-desc">从一个场景做起</div>
            </div>
          </div>
        </div>
      </section>
  `);
}
