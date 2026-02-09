<template>
  <button class="theme-toggle" type="button" :aria-label="toggleLabel" @click="toggleMode">
    <i v-if="isDark" class="fa-solid fa-sun theme-toggle-icon" aria-hidden="true"></i>
    <i v-else class="fa-solid fa-moon theme-toggle-icon" aria-hidden="true"></i>
  </button>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import { useThemeStore } from '@/stores/theme';

const themeStore = useThemeStore();
const { t } = useI18n();

const isDark = computed(() => themeStore.mode === 'dark');
const toggleLabel = computed(() =>
  isDark.value ? t('theme.toggle.light') : t('theme.toggle.dark')
);

const toggleMode = () => {
  themeStore.setMode(isDark.value ? 'light' : 'dark');
};
</script>
