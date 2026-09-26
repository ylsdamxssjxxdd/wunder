<template>
  <el-dialog
    :model-value="visible"
    class="globe-app-dialog"
    :title="t('userWorld.helperApps.globe.dialogTitle')"
    width="760px"
    destroy-on-close
    @update:model-value="handleVisibleChange"
  >
    <GlobeAppPanel />
  </el-dialog>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

import GlobeAppPanel from './GlobeAppPanel.vue';

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
</style>
