"use strict";

// ==========================================================
// 轻量级 PPT 控制脚本
// 目标：键盘/鼠标/触摸切页 + 分步展示 + 进度提示
// ==========================================================

// 获取所有幻灯片与 HUD 元素
const slides = Array.from(document.querySelectorAll(".slide"));
const progressBar = document.querySelector("#progress-bar");
const counter = document.querySelector("#counter");

// 当前页索引与每一页的分步展示进度
const state = {
  index: 0,
};
const fragmentState = slides.map(() => 0);

// 读取某一页的分步节点
function getFragments(slide) {
  return Array.from(slide.querySelectorAll(".fragment"));
}

// 限制索引边界，避免越界报错
function clampIndex(index) {
  if (index < 0) {
    return 0;
  }
  if (index > slides.length - 1) {
    return slides.length - 1;
  }
  return index;
}

// 更新分步显示状态
function applyFragments(slide, visibleCount) {
  const fragments = getFragments(slide);
  fragments.forEach((fragment, idx) => {
    fragment.classList.toggle("visible", idx < visibleCount);
  });
}

// 更新 HUD 进度条与页码
function updateHud() {
  if (progressBar) {
    const ratio = slides.length === 0 ? 0 : (state.index + 1) / slides.length;
    progressBar.style.width = `${Math.round(ratio * 100)}%`;
  }
  if (counter) {
    counter.textContent = `${state.index + 1} / ${slides.length}`;
  }
}

// 更新地址栏的 hash，方便定位到指定页
function updateHash() {
  const targetHash = `#${state.index + 1}`;
  if (window.location.hash !== targetHash) {
    window.location.hash = targetHash;
  }
}

// 根据当前状态刷新页面
function updateView() {
  slides.forEach((slide, idx) => {
    slide.classList.toggle("active", idx === state.index);
  });

  // 保证当前页的分步进度不超过实际数量
  const currentSlide = slides[state.index];
  if (currentSlide) {
    const fragments = getFragments(currentSlide);
    fragmentState[state.index] = Math.min(
      fragmentState[state.index],
      fragments.length,
    );
    applyFragments(currentSlide, fragmentState[state.index]);
  }

  // 其他页面隐藏分步内容，避免视觉干扰
  slides.forEach((slide, idx) => {
    if (idx === state.index) {
      return;
    }
    applyFragments(slide, 0);
  });

  updateHud();
  updateHash();
}

// 跳转到指定页面
function goTo(index) {
  state.index = clampIndex(index);
  updateView();
}

// 下一步：优先展示分步内容，否则进入下一页
function next() {
  const currentSlide = slides[state.index];
  if (!currentSlide) {
    return;
  }
  const fragments = getFragments(currentSlide);
  const currentFragments = fragmentState[state.index];
  if (currentFragments < fragments.length) {
    fragmentState[state.index] += 1;
    updateView();
    return;
  }
  if (state.index < slides.length - 1) {
    state.index += 1;
    updateView();
  }
}

// 上一步：先回退分步内容，再回到上一页
function prev() {
  if (fragmentState[state.index] > 0) {
    fragmentState[state.index] -= 1;
    updateView();
    return;
  }
  if (state.index > 0) {
    state.index -= 1;
    updateView();
  }
}

// 键盘控制：方向键、空格、PageUp/Down 等
function handleKeydown(event) {
  switch (event.key) {
    case "ArrowRight":
    case "ArrowDown":
    case "PageDown":
    case " ":
    case "Enter":
      event.preventDefault();
      next();
      break;
    case "ArrowLeft":
    case "ArrowUp":
    case "PageUp":
      event.preventDefault();
      prev();
      break;
    case "Home":
      event.preventDefault();
      goTo(0);
      break;
    case "End":
      event.preventDefault();
      goTo(slides.length - 1);
      break;
    default:
      break;
  }
}

// 鼠标点击翻页，避免选中文本时误触
function handleClick(event) {
  if (event.target.closest("a, button, input, textarea, pre, code")) {
    return;
  }
  const selection = window.getSelection();
  if (selection && selection.toString().length > 0) {
    return;
  }
  next();
}

// 触摸滑动翻页，适配移动端展示
let touchStartX = 0;
function handleTouchStart(event) {
  touchStartX = event.changedTouches[0]?.clientX ?? 0;
}

function handleTouchEnd(event) {
  const touchEndX = event.changedTouches[0]?.clientX ?? 0;
  const deltaX = touchEndX - touchStartX;
  if (Math.abs(deltaX) < 50) {
    return;
  }
  if (deltaX < 0) {
    next();
  } else {
    prev();
  }
}

// 根据地址栏 hash 定位页面
function handleHashChange() {
  const value = Number.parseInt(window.location.hash.replace("#", ""), 10);
  if (!Number.isNaN(value) && value >= 1) {
    goTo(value - 1);
  } else {
    updateView();
  }
}

// 初始化事件绑定
document.addEventListener("keydown", handleKeydown);
document.addEventListener("click", handleClick);
document.addEventListener("touchstart", handleTouchStart, { passive: true });
document.addEventListener("touchend", handleTouchEnd, { passive: true });
window.addEventListener("hashchange", handleHashChange);

// 启动时读取 hash 并刷新页面
handleHashChange();
