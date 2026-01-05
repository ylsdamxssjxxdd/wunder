"use strict";

// 全局命名空间：集中放置 PPT 运行所需的方法与状态，避免污染 window 其他字段。
window.WunderPpt = window.WunderPpt || {};

// 将单页 HTML 字符串转为 DOM 元素，避免每页重复编写模板插入逻辑。
window.WunderPpt.createSlide = function createSlide(html) {
  const template = document.createElement("template");
  // 使用 trim 避免出现多余空白文本节点，确保只有一个 section 元素。
  template.innerHTML = html.trim();
  return template.content.firstElementChild;
};

// 注册页面构建函数，按加载顺序存入列表，供启动脚本统一渲染。
window.WunderPpt.registerSlide = function registerSlide(buildSlide) {
  if (typeof buildSlide !== "function") {
    console.error("页面构建函数无效，无法注册。");
    return;
  }
  if (!Array.isArray(window.WunderPpt.slides)) {
    window.WunderPpt.slides = [];
  }
  window.WunderPpt.slides.push(buildSlide);
};
