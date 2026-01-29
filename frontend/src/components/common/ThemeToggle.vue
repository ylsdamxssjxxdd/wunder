<template>
  <button class="theme-toggle" type="button" :aria-label="toggleLabel" @click="toggleMode">
    <i v-if="isDark" class="fa-solid fa-sun theme-toggle-icon" aria-hidden="true"></i>
    <i v-else class="fa-solid fa-moon theme-toggle-icon" aria-hidden="true"></i>
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
