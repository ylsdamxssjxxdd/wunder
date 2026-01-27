<template>
  <button class="theme-toggle" type="button" :aria-label="toggleLabel" @click="toggleMode">
    <svg v-if="isDark" class="theme-toggle-icon" viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="5" />
      <path d="M12 2v2" />
      <path d="M12 20v2" />
      <path d="M4.22 4.22l1.42 1.42" />
      <path d="M18.36 18.36l1.42 1.42" />
      <path d="M2 12h2" />
      <path d="M20 12h2" />
      <path d="M4.22 19.78l1.42-1.42" />
      <path d="M18.36 5.64l1.42-1.42" />
    </svg>
    <svg v-else class="theme-toggle-icon" viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M21 14.5A8.5 8.5 0 1 1 9.5 3a7 7 0 0 0 11.5 11.5z"
      />
    </svg>
  </button>
</template>

<script setup>
import { computed } from 'vue';

import { useThemeStore } from '@/stores/theme';

const themeStore = useThemeStore();

const isDark = computed(() => themeStore.mode === 'dark');
const toggleLabel = computed(() => (isDark.value ? '切换为浅色主题' : '切换为深色主题'));

const toggleMode = () => {
  themeStore.setMode(isDark.value ? 'light' : 'dark');
};
</script>
