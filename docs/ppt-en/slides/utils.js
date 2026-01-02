"use strict";

// 将单页 HTML 字符串转为元素，避免每页重复编写模板插入逻辑。
export function createSlide(html) {
  const template = document.createElement("template");
  // 使用 trim 避免出现多余空白文本节点，保证只有一个 section 元素。
  template.innerHTML = html.trim();
  return template.content.firstElementChild;
}
