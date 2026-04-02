<template>
  <button class="theme-toggle" type="button" :aria-label="label" @click="cycle">
    <i class="fa-solid fa-palette theme-toggle-icon" aria-hidden="true"></i>
  </button>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useThemeStore } from '@/stores/theme';
import { THEME_PALETTES } from '@/utils/themeAppearance';

const themeStore = useThemeStore();

const label = computed(() => themeStore.palette);

const cycle = () => {
  const idx = THEME_PALETTES.indexOf(themeStore.palette);
  const next = THEME_PALETTES[(idx + 1) % THEME_PALETTES.length];
  themeStore.setPalette(next);
};
</script>
