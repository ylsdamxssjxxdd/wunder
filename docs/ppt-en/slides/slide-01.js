"use strict";

import { createSlide } from "./utils.js";

// 第 1 页：欢迎页（封面），用于拆分维护本页内容。
export default function buildSlide() {
  return createSlide(`
<section class="slide cover" data-title="Welcome">
        <h1>wunder Agent Router</h1>
        <p class="subtitle">From "chatting" to getting things done</p>
      </section>
  `);
}
