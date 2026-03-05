<template>
  <el-dialog
    :model-value="visible"
    class="messenger-dialog messenger-helper-app-dialog"
    :title="t('userWorld.helperApps.localFileSearch.dialogTitle')"
    width="700px"
    destroy-on-close
    @update:model-value="handleVisibleChange"
  >
    <MessengerLocalFileSearchPanel compact />
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

import MessengerLocalFileSearchPanel from './MessengerLocalFileSearchPanel.vue';

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
