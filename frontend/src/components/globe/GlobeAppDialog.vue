<template>
  <el-dialog
    :model-value="visible"
    class="globe-app-dialog"
    :title="t('userWorld.helperApps.globe.dialogTitle')"
    width="760px"
    destroy-on-close
    @update:model-value="handleVisibleChange"
  >
    <div class="globe-app-body">
      <GlobeViewer />
      <div class="globe-app-meta">
        <span>{{ t('userWorld.helperApps.globe.hint') }}</span>
        <span>{{ t('userWorld.helperApps.globe.note') }}</span>
      </div>
    </div>
    <template #footer>
      <span class="dialog-footer">
        <button class="user-world-dialog-btn muted" type="button" @click="handleVisibleChange(false)">
          {{ t('common.close') }}
        </button>
      </span>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

import GlobeViewer from './GlobeViewer.vue';

defineProps<{
  visible: boolean;
}>();

const emit = defineEmits<{
  'update:visible': [value: boolean];
}>();

const { t } = useI18n();

const handleVisibleChange = (value: boolean) => {
  emit('update:visible', Boolean(value));
};
</script>

<style scoped>
.globe-app-dialog :deep(.el-dialog) {
  max-width: 92vw;
}

.globe-app-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
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
