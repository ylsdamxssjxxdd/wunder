"use strict";

import slideManifest from "./manifest.js?v=20260104-06";

const CACHE_BUST = "v=20260104-06";
const withCacheBust = (path) => (path.includes("?") ? path : `${path}?${CACHE_BUST}`);

// 负责把所有页面插入到 deck，再加载翻页控制脚本。
const deck = document.querySelector("#deck");
if (!deck) {
  console.error("Cannot find #deck container, failed to render PPT pages.");
} else {
  // 异步加载页面模块，保证按清单顺序渲染。
  const loadSlides = async () => {
    const fragment = document.createDocumentFragment();

    // 逐个动态 import，确保页面顺序与清单一致。
    for (const [index, path] of slideManifest.entries()) {
      try {
        const module = await import(withCacheBust(path));
        const buildSlide = module?.default;
        if (typeof buildSlide !== "function") {
          console.error(`Slide module missing default export: ${path}`);
          continue;
        }
        const slide = buildSlide();
        if (!slide) {
          continue;
        }
        // 先给第一页添加 active，避免加载期间出现空白。
        if (index === 0) {
          slide.classList.add("active");
        }
        fragment.appendChild(slide);
      } catch (error) {
        console.error(`Failed to load slide module: ${path}`, error);
      }
    }

    deck.appendChild(fragment);

    // 页面就绪后再加载控制脚本，保证能正确读取所有 .slide。
    await import(withCacheBust("../app.js"));
  };

  loadSlides().catch((error) => {
    console.error("Failed to load PPT pages", error);
  });
}
