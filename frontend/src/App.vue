<template>
  <div class="app-shell">
    <div class="app-shell-content">
      <MaintenanceBanner />
      <router-view />
    </div>
    <!-- 主题弹窗宿主：mzConfirm / mzPrompt 的渲染点，全站只挂一处。 -->
    <MzDialogHost />
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue';

import MaintenanceBanner from '@/components/system/MaintenanceBanner.vue';
import MzDialogHost from '@/components/common/MzDialogHost.vue';

// Keep the shell height in sync with the *visible* viewport. `dvh` alone does
// not shrink when a soft keyboard opens on iOS Safari, so the composer would be
// hidden behind the keyboard; visualViewport reports the keyboard-shrunk height.
function applyViewportHeightVar() {
  if (typeof window === 'undefined' || typeof document === 'undefined') return;
  const visual = window.visualViewport;
  const height =
    visual && visual.scale <= 1.01
      ? Math.min(window.innerHeight, Math.round(visual.height))
      : window.innerHeight;
  document.documentElement.style.setProperty('--app-viewport-height-js', `${height}px`);
}

function handleViewportResize() {
  applyViewportHeightVar();
}

onMounted(() => {
  applyViewportHeightVar();
  if (typeof window === 'undefined') return;
  window.addEventListener('resize', handleViewportResize);
  window.addEventListener('orientationchange', handleViewportResize);
  window.visualViewport?.addEventListener('resize', handleViewportResize);
  // iOS 关闭键盘时偶尔只触发 visualViewport 的 scroll 而不触发 resize。
  window.visualViewport?.addEventListener('scroll', handleViewportResize);
});

onBeforeUnmount(() => {
  if (typeof document === 'undefined') return;
  document.documentElement.style.removeProperty('--app-viewport-height-js');
  if (typeof window === 'undefined') return;
  window.removeEventListener('resize', handleViewportResize);
  window.removeEventListener('orientationchange', handleViewportResize);
  window.visualViewport?.removeEventListener('resize', handleViewportResize);
  window.visualViewport?.removeEventListener('scroll', handleViewportResize);
});
</script>

<style>
:root {
  --app-viewport-height-js: 100vh;
  --app-viewport-height: var(--app-viewport-height-js);
}

@supports (height: 100dvh) {
  :root {
    /* dvh 跟随地址栏收放，visualViewport 实测值在软键盘弹出时收缩；
       取两者较小值，手机浏览器里输入区才不会被键盘盖住。 */
    --app-viewport-height: min(100dvh, var(--app-viewport-height-js));
  }
}

.app-shell {
  width: 100%;
  max-width: 100%;
  height: 100%;
  min-height: 0;
}

.app-shell-content {
  width: 100%;
  max-width: 100%;
  min-width: 0;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.app-shell-content > * {
  flex: 1 1 auto;
  min-width: 0;
  min-height: 0;
}
</style>
