"use strict";

import { createSlide } from "./utils.js";

// 第 1 页：欢迎页（封面），用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide cover" data-title="欢迎">
        <h1>wunder 智能体路由器</h1>
        <p class="subtitle">让大模型从“会聊”走向“会做事”</p>
      </section>
  `);
}
