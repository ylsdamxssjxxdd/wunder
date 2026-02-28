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
import { computed } from 'vue';
import { useRoute } from 'vue-router';

import DesktopWindowChrome from '@/components/common/DesktopWindowChrome.vue';
import { isDesktopModeEnabled } from '@/config/desktop';
import MaintenanceOverlay from '@/components/system/MaintenanceOverlay.vue';

const route = useRoute();

const desktopChromeVisible = computed(
  () => isDesktopModeEnabled() && !route.path.startsWith('/admin')
);
</script>

<style>
.app-shell {
  height: 100%;
  min-height: 0;
}

.app-shell--desktop {
  padding-top: var(--desktop-window-chrome-height, 36px);
  box-sizing: border-box;
}

.app-shell-content {
  height: 100%;
  min-height: 0;
}
</style>
