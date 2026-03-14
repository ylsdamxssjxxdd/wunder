<template>
  <div class="app-shell" :class="{ 'app-shell--desktop': desktopChromeVisible }">
    <DesktopWindowChrome v-if="desktopChromeVisible" />
    <div class="app-shell-content">
      <router-view />
    </div>
    <MaintenanceOverlay />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, watchEffect } from 'vue';
import { useRoute } from 'vue-router';

import DesktopWindowChrome from '@/components/common/DesktopWindowChrome.vue';
import { isDesktopModeEnabled } from '@/config/desktop';
import MaintenanceOverlay from '@/components/system/MaintenanceOverlay.vue';

const route = useRoute();

// Keep viewport height stable on legacy browsers that do not support dvh reliably.
function applyViewportHeightVar() {
  if (typeof window === 'undefined' || typeof document === 'undefined') return;
  document.documentElement.style.setProperty('--app-viewport-height-js', `${window.innerHeight}px`);
}

function handleViewportResize() {
  applyViewportHeightVar();
}

const desktopChromeVisible = computed(
  () => isDesktopModeEnabled() && !route.path.startsWith('/admin')
);

watchEffect(() => {
  if (typeof document === 'undefined') return;
  document.body.classList.toggle('desktop-shell-active', desktopChromeVisible.value);
});

onMounted(() => {
  applyViewportHeightVar();
  if (typeof window === 'undefined') return;
  window.addEventListener('resize', handleViewportResize);
  window.addEventListener('orientationchange', handleViewportResize);
  window.visualViewport?.addEventListener('resize', handleViewportResize);
});

onBeforeUnmount(() => {
  if (typeof document === 'undefined') return;
  document.body.classList.remove('desktop-shell-active');
  document.documentElement.style.removeProperty('--app-viewport-height-js');
  if (typeof window === 'undefined') return;
  window.removeEventListener('resize', handleViewportResize);
  window.removeEventListener('orientationchange', handleViewportResize);
  window.visualViewport?.removeEventListener('resize', handleViewportResize);
});
</script>

<style>
:root {
  --desktop-window-chrome-height: 36px;
  --app-viewport-height-js: 100vh;
  --app-viewport-height: var(--app-viewport-height-js);
}

@supports (height: 100dvh) {
  :root {
    --app-viewport-height: 100dvh;
  }
}

body.desktop-shell-active {
  --app-viewport-height: calc(var(--app-viewport-height-js) - var(--desktop-window-chrome-height));
}

@supports (height: 100dvh) {
  body.desktop-shell-active {
    --app-viewport-height: calc(100dvh - var(--desktop-window-chrome-height));
  }
}

.app-shell {
  height: 100%;
  min-height: 0;
}

.app-shell--desktop {
  padding-top: var(--desktop-window-chrome-height);
  box-sizing: border-box;
}

.app-shell-content {
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

body.desktop-shell-active .el-message,
body.desktop-shell-active .el-notification {
  top: calc(var(--desktop-window-chrome-height, 36px) + 12px) !important;
}

body.desktop-shell-active .messenger-timeline-detail-dialog {
  margin-top: calc(var(--desktop-window-chrome-height, 36px) + 12px) !important;
  max-height: calc(var(--app-viewport-height, 100vh) - 24px);
}
</style>
