"use strict";

import { createSlide } from "./utils.js";

// 第 1 页：欢迎页 + 目录页（合并展示），用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide toc" data-title="欢迎">
        <div class="stack">
          <h1>wunder 智能体调度平台</h1>
          <p class="subtitle">让大模型从“会聊”走向“会做事”</p>
        </div>
        <div class="eyebrow">目录</div>
        <div class="toc-grid">
          <a class="toc-item toc-link" href="#2">
            <div class="toc-index">01</div>
            <div>
              <div class="toc-title">核心理念</div>
              <div class="toc-desc">从“会聊”到“会做事”</div>
            </div>
          </a>
          <a class="toc-item toc-link" href="#4">
            <div class="toc-index">02</div>
            <div>
              <div class="toc-title">工具体系</div>
              <div class="toc-desc">六类工具与统一治理</div>
            </div>
          </a>
          <a class="toc-item toc-link" href="#11">
            <div class="toc-index">03</div>
            <div>
              <div class="toc-title">工作区</div>
              <div class="toc-desc">产出沉淀与可复用资产</div>
            </div>
          </a>
          <a class="toc-item toc-link" href="#13">
            <div class="toc-index">04</div>
            <div>
              <div class="toc-title">前沿特性</div>
              <div class="toc-desc">记忆/压缩 + A2UI + A2A</div>
            </div>
          </a>
          <a class="toc-item toc-link" href="#16">
            <div class="toc-index">05</div>
            <div>
              <div class="toc-title">智能体线程管理</div>
              <div class="toc-desc">稳定、可监控、可取消</div>
            </div>
          </a>
          <a class="toc-item toc-link" href="#17">
            <div class="toc-index">06</div>
            <div>
              <div class="toc-title">快速开始</div>
              <div class="toc-desc">从一个场景做起</div>
            </div>
          </a>
        </div>
      </section>
  `);
}
