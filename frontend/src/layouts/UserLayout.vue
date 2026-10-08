<template>
  <router-view v-slot="{ Component }">
    <KeepAlive include="MessengerView">
      <component :is="Component" />
    </KeepAlive>
  </router-view>
  <CompanionFloatingLayer v-if="showGlobalCompanionLayer" />
</template>

<script setup lang="ts">
import { KeepAlive, computed, defineAsyncComponent, watch } from 'vue';
import { useRoute } from 'vue-router';

import { useAuthStore } from '@/stores/auth';

// Keep companion UI out of every authenticated route's startup graph.
const CompanionFloatingLayer = defineAsyncComponent(
  () => import('@/components/companions/CompanionFloatingLayer.vue')
);

const authStore = useAuthStore();
const route = useRoute();
const isEmbeddedChatRoute = computed(() => /\/(?:app|demo)\/embed\/chat(?:\/|$)/.test(route.path));
const isMessengerViewRoute = computed(() =>
  /\/(?:app|demo)\/(?:home|chat|settings)(?:\/|$)/.test(route.path)
);
const showGlobalCompanionLayer = computed(() => !isEmbeddedChatRoute.value && !isMessengerViewRoute.value);

const refreshProfile = () => {
  void authStore.loadProfile().catch(() => undefined);
};

// Route guards load the initial profile; only refresh here when the active token changes.
watch(
  () => authStore.token,
  (next, previous) => {
    if (next === previous) return;
    if (!String(next || '').trim()) return;
    refreshProfile();
  }
);
</script>
