<template>
  <div class="globe-app-panel">
    <GlobeViewer fill />
    <div class="globe-app-meta">
      <span>{{ t('userWorld.helperApps.globe.hint') }}</span>
      <span>{{ t('userWorld.helperApps.globe.note') }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

import GlobeViewer from './GlobeViewer.vue';

const { t } = useI18n();
</script>

<style scoped>
.globe-app-panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
  flex: 1;
  min-height: 0;
}

.globe-app-meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  color: var(--hula-muted, #64748b);
}

.globe-app-meta span:last-child {
  text-align: right;
}

@media (max-width: 720px) {
  .globe-app-meta {
    flex-direction: column;
    align-items: flex-start;
  }

  .globe-app-meta span:last-child {
    text-align: left;
  }
}
</style>
