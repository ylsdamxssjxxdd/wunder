<template>
  <el-dialog
    :model-value="visible"
    :title="t('workspace.preview.dialogTitle')"
    width="720px"
    class="workspace-dialog"
    append-to-body
    @update:model-value="handleDialogVisibleChange"
  >
    <div class="workspace-preview-title">
      {{ resolvedTitle }}
    </div>
    <div class="workspace-preview-meta">{{ resolvedWorkspacePath }}</div>
    <div class="workspace-preview embed">
      <img v-if="imageUrl" :src="imageUrl" :alt="resolvedTitle" />
    </div>
    <template #footer>
      <button
        class="workspace-btn secondary"
        type="button"
        :disabled="!workspacePath"
        @click="emit('download')"
      >
        {{ t('common.download') }}
      </button>
      <button class="workspace-btn secondary" type="button" @click="emit('close')">
        {{ t('common.close') }}
      </button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';

const props = defineProps<{
  visible: boolean;
  imageUrl: string;
  title: string;
  workspacePath: string;
}>();

const emit = defineEmits<{
  close: [];
  download: [];
}>();

const { t } = useI18n();

const resolvedTitle = computed(() => String(props.title || '').trim() || t('chat.imagePreview'));
const resolvedWorkspacePath = computed(
  () => String(props.workspacePath || '').trim() || t('chat.imagePreview')
);

const handleDialogVisibleChange = (nextVisible: boolean) => {
  if (nextVisible) return;
  emit('close');
};
</script>
