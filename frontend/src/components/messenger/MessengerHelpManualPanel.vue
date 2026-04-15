<template>
  <section class="messenger-help-manual-panel">
    <iframe
      class="messenger-help-manual-frame"
      :src="docsSiteSrc"
      title="wunder docs"
      referrerpolicy="no-referrer"
      @load="handleFrameLoad"
      @error="handleFrameLoad"
    ></iframe>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, watch } from 'vue';

import { resolveApiBase } from '@/config/runtime';

const DOCS_SITE_VERSION = '20260415-01';

const emit = defineEmits<{
  'loading-change': [loading: boolean];
}>();

const docsSiteSrc = computed(() => {
  const fallback = `/docs/?embed=user&v=${DOCS_SITE_VERSION}`;
  const apiBase = String(resolveApiBase() || '').trim();
  if (!apiBase) {
    return fallback;
  }
  if (!/^https?:\/\//i.test(apiBase)) {
    return fallback;
  }
  try {
    const url = new URL(apiBase);
    return `${url.origin}/docs/?embed=user&v=${DOCS_SITE_VERSION}`;
  } catch {
    return fallback;
  }
});

const emitLoadingChange = (loading: boolean) => {
  emit('loading-change', loading === true);
};

const handleFrameLoad = () => {
  emitLoadingChange(false);
};

onMounted(() => {
  emitLoadingChange(true);
});

watch(
  () => docsSiteSrc.value,
  () => {
    emitLoadingChange(true);
  },
  { immediate: true }
);
</script>

<style scoped>
.messenger-help-manual-panel {
  display: flex;
  flex: 1;
  min-height: 0;
  height: 100%;
}

.messenger-help-manual-frame {
  border: 0;
  width: 100%;
  height: 100%;
  display: block;
  background: #ffffff;
}
</style>
