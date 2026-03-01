<template>
  <div class="app-shell" :class="{ 'app-shell--desktop': desktopChromeVisible }">
    <DesktopWindowChrome v-if="desktopChromeVisible" />
    <div class="app-shell-content">
      <router-view />
    </div>
    <ToolApprovalDialog />
    <MaintenanceOverlay />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, watchEffect } from 'vue';
import { useRoute } from 'vue-router';

import DesktopWindowChrome from '@/components/common/DesktopWindowChrome.vue';
import ToolApprovalDialog from '@/components/chat/ToolApprovalDialog.vue';
import { isDesktopModeEnabled } from '@/config/desktop';
import MaintenanceOverlay from '@/components/system/MaintenanceOverlay.vue';

const route = useRoute();

const desktopChromeVisible = computed(
  () => isDesktopModeEnabled() && !route.path.startsWith('/admin')
);

watchEffect(() => {
  if (typeof document === 'undefined') return;
  document.body.classList.toggle('desktop-shell-active', desktopChromeVisible.value);
});

onBeforeUnmount(() => {
  if (typeof document === 'undefined') return;
  document.body.classList.remove('desktop-shell-active');
});
</script>

<style>
.app-shell {
  --desktop-window-chrome-height: 36px;
  --app-viewport-height: 100vh;
  --app-viewport-height-dvh: 100dvh;
  height: 100%;
  min-height: 0;
}

.app-shell--desktop {
  --app-viewport-height: calc(100vh - var(--desktop-window-chrome-height));
  --app-viewport-height-dvh: calc(100dvh - var(--desktop-window-chrome-height));
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
</style>
