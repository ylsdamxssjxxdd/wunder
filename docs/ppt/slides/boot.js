"use strict";

const CACHE_BUST = "v=20260110-09";
const withCacheBust = (path) => (path.includes("?") ? path : `${path}?${CACHE_BUST}`);

// 负责把所有页面插入到 deck，再加载翻页控制脚本。
const deck = document.querySelector("#deck");
if (!deck) {
  console.error("未找到 #deck 容器，无法渲染 PPT 页面。");
} else {
  // 以顺序方式加载脚本，保证页面注册顺序一致，兼容 file:// 场景。
  const loadScript = (path) =>
    new Promise((resolve, reject) => {
      const script = document.createElement("script");
      script.src = withCacheBust(path);
      script.async = false;
      script.onload = () => resolve();
      script.onerror = () => reject(new Error(`加载脚本失败：${path}`));
      (document.head || document.body).appendChild(script);
    });

  const loadSlides = async () => {
    const manifest = window.WunderPpt?.manifest;
    if (!Array.isArray(manifest) || manifest.length === 0) {
      console.error("页面清单为空，无法加载 PPT 页面。");
      return;
    }

    // 逐个加载页面脚本，确保注册顺序与清单一致。
    for (const path of manifest) {
      await loadScript(path);
    }

    const slideBuilders = window.WunderPpt?.slides ?? [];
    if (slideBuilders.length === 0) {
      console.error("未注册任何页面构建函数，无法渲染 PPT 页面。");
      return;
    }

    const fragment = document.createDocumentFragment();

    // 依次构建页面，确保第一页默认激活，避免出现空白。
    slideBuilders.forEach((buildSlide, index) => {
      try {
        if (typeof buildSlide !== "function") {
          console.error("页面构建函数类型异常，已跳过。");
          return;
        }
        const slide = buildSlide();
        if (!slide) {
          return;
        }
        if (index === 0) {
          slide.classList.add("active");
        }
        fragment.appendChild(slide);
      } catch (error) {
        console.error("构建 PPT 页面失败。", error);
      }
    });

    deck.appendChild(fragment);

    // 页面就绪后再加载翻页控制逻辑，确保能读取全部 .slide。
    await loadScript("app.js");
  };

  loadSlides().catch((error) => {
    console.error("加载 PPT 页面失败。", error);
  });
}
