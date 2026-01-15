<template>
  <div class="theme-toggle">
    <span class="theme-toggle-label">主题</span>
    <el-switch
      v-model="modeProxy"
      class="theme-toggle-switch"
      inline-prompt
      active-text="暗"
      inactive-text="浅"
      active-value="dark"
      inactive-value="light"
      aria-label="主题切换"
    />
  </div>
</template>

<script setup>
import { computed } from 'vue';

import { useThemeStore } from '@/stores/theme';

const themeStore = useThemeStore();

// 使用计算属性桥接 el-switch 的 v-model，统一走 store 持久化逻辑
const modeProxy = computed({
  get: () => themeStore.mode,
  set: (value) => themeStore.setMode(value)
});
</script>
