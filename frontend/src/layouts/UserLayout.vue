<template>
  <router-view />
</template>

<script setup>
import { onMounted, watch } from 'vue';
import { useRoute } from 'vue-router';

import { useAuthStore } from '@/stores/auth';

const authStore = useAuthStore();
const route = useRoute();

const refreshProfile = () => {
  authStore.loadProfile();
};

// 路由切换时刷新用户信息，兼容演示模式与登录态切换
onMounted(refreshProfile);

watch(
  () => route.path,
  () => {
    refreshProfile();
  }
);
</script>
