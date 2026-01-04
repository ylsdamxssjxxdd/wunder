"use strict";

import { createSlide } from "./utils.js";

// 第 19 页：收尾，用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide" data-title="收尾">
        <div class="slide-meta">
          <span class="section-tag">第6节 快速开始</span>
          <div class="section-map">
            <a class="section-chip" href="#18">快速开始</a>
            <a class="section-chip active" href="#19">收尾</a>
          </div>
        </div>
        <h2>谢谢</h2>
        <p class="section-lead">欢迎提问，也欢迎一起做试点</p>
        <div class="card">
          <h3>wunder：让大模型会做事，并能长期复用</h3>
          <p>从一个场景开始，把成功经验沉淀成工具与流程</p>
        </div>
      </section>
  `);
}
