<template>
  <section class="messenger-help-manual-panel">
    <iframe
      class="messenger-help-manual-frame"
      :src="docsSiteSrc"
      title="wunder docs"
      referrerpolicy="no-referrer"
    ></iframe>
  </section>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { resolveApiBase } from '@/config/runtime';

const docsSiteSrc = computed(() => {
  const fallback = '/docs/?embed=user';
  const apiBase = String(resolveApiBase() || '').trim();
  if (!apiBase) {
    return fallback;
  }
  if (!/^https?:\/\//i.test(apiBase)) {
    return fallback;
  }
  try {
    const url = new URL(apiBase);
    return `${url.origin}/docs/?embed=user`;
  } catch {
    return fallback;
  }
});
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
