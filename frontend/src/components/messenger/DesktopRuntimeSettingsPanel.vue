<template>
  <DesktopRuntimePreferencesPanel v-if="desktopLocalMode" :desktop-local-mode="desktopLocalMode" />
</template>

<script setup lang="ts">
import DesktopRuntimePreferencesPanel from '@/components/messenger/DesktopRuntimePreferencesPanel.vue';

withDefaults(
  defineProps<{
    desktopLocalMode?: boolean;
  }>(),
  {
    desktopLocalMode: true
  }
);
</script>
