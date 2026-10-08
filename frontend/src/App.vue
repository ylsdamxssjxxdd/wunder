<template>
  <div class="app-shell">
    <div class="app-shell-content">
      <MaintenanceBanner />
      <router-view />
    </div>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue';

import MaintenanceBanner from '@/components/system/MaintenanceBanner.vue';

// Keep viewport height stable on legacy browsers that do not support dvh reliably.
function applyViewportHeightVar() {
  if (typeof window === 'undefined' || typeof document === 'undefined') return;
  document.documentElement.style.setProperty('--app-viewport-height-js', `${window.innerHeight}px`);
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
});

onBeforeUnmount(() => {
  if (typeof document === 'undefined') return;
  document.documentElement.style.removeProperty('--app-viewport-height-js');
  if (typeof window === 'undefined') return;
  window.removeEventListener('resize', handleViewportResize);
  window.removeEventListener('orientationchange', handleViewportResize);
  window.visualViewport?.removeEventListener('resize', handleViewportResize);
});
</script>

<style>
:root {
  --app-viewport-height-js: 100vh;
  --app-viewport-height: var(--app-viewport-height-js);
}

@supports (height: 100dvh) {
  :root {
    --app-viewport-height: 100dvh;
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
