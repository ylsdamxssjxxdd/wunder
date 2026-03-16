<template>
  <el-dialog
    :model-value="visible"
    :title="t('workspace.preview.dialogTitle')"
    :width="dialogWidth"
    top="4vh"
    class="workspace-dialog messenger-image-preview-dialog"
    append-to-body
    @update:model-value="handleDialogVisibleChange"
  >
    <div class="messenger-image-preview-head">
      <div class="workspace-preview-title">
        {{ resolvedTitle }}
      </div>
      <div class="workspace-preview-meta" :title="resolvedWorkspacePath">{{ resolvedWorkspacePath }}</div>
    </div>
    <div class="workspace-preview embed messenger-image-preview-body">
      <ZoomableImagePreview :image-url="imageUrl" :alt="resolvedTitle" :active="visible" />
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

import ZoomableImagePreview from '@/components/common/ZoomableImagePreview.vue';
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

const dialogWidth = 'min(92vw, 980px)';
const resolvedTitle = computed(() => String(props.title || '').trim() || t('chat.imagePreview'));
const resolvedWorkspacePath = computed(
  () => String(props.workspacePath || '').trim() || t('chat.imagePreview')
);

const handleDialogVisibleChange = (nextVisible: boolean) => {
  if (nextVisible) return;
  emit('close');
};
</script>

<style scoped>
.messenger-image-preview-head {
  display: grid;
  gap: 6px;
  margin-bottom: 10px;
}

.messenger-image-preview-head .workspace-preview-meta {
  margin-bottom: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.messenger-image-preview-body {
  min-height: 0;
  height: clamp(260px, 62vh, 760px);
  max-height: calc(92vh - 180px);
  overflow: hidden;
}

:deep(.messenger-image-preview-dialog.el-dialog) {
  max-width: min(92vw, 980px);
  max-height: 92vh;
  margin-bottom: 0;
  display: flex;
  flex-direction: column;
}

:deep(.messenger-image-preview-dialog .el-dialog__body) {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

:deep(.messenger-image-preview-body .zoomable-image-preview) {
  height: 100%;
}

:deep(.messenger-image-preview-body .zoomable-image-stage) {
  height: 100%;
  min-height: 0;
  max-height: none;
}

@media (max-width: 960px) {
  .messenger-image-preview-body {
    height: clamp(220px, 58vh, 700px);
    max-height: calc(94vh - 170px);
  }
}
</style>
