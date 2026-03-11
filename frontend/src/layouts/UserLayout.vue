<template>
  <router-view />
</template>

<script setup lang="ts">
import { onMounted, watch } from 'vue';

import { useAuthStore } from '@/stores/auth';

const authStore = useAuthStore();

const refreshProfile = () => {
  void authStore.loadProfile().catch(() => undefined);
};

// 首次挂载时拉取一次用户信息；后续仅在 token 变化时刷新，避免路由切换造成刷新风暴。
onMounted(refreshProfile);

watch(
  () => authStore.token,
  (next, previous) => {
    if (next === previous) return;
    if (!String(next || '').trim()) return;
    refreshProfile();
  }
);
</script>
