<template>
  <router-view v-slot="{ Component }">
    <KeepAlive include="MessengerView">
      <component :is="Component" />
    </KeepAlive>
  </router-view>
  <CompanionFloatingLayer v-if="showGlobalCompanionLayer" />
</template>

<script setup lang="ts">
import { KeepAlive, computed, onMounted, watch } from 'vue';
import { useRoute } from 'vue-router';

import CompanionFloatingLayer from '@/components/companions/CompanionFloatingLayer.vue';
import { useAuthStore } from '@/stores/auth';

const authStore = useAuthStore();
const route = useRoute();
const isEmbeddedChatRoute = computed(() => /\/(?:app|desktop|demo)\/embed\/chat(?:\/|$)/.test(route.path));
const isMessengerViewRoute = computed(() =>
  /\/(?:app|desktop|demo)\/(?:home|tools|cron|channels|chat|beeroom|orchestration|plaza|user-world|workspace|settings|profile)(?:\/|$)/.test(
    route.path
  )
);
const showGlobalCompanionLayer = computed(() => !isEmbeddedChatRoute.value && !isMessengerViewRoute.value);

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
