<template>
  <el-dialog
    :model-value="visible"
    :width="dialogWidth"
    top="clamp(10px, 4vh, 36px)"
    class="workspace-dialog messenger-image-preview-dialog"
    :show-close="false"
    append-to-body
    @update:model-value="handleDialogVisibleChange"
  >
    <template #header>
      <div class="messenger-dialog-header">
        <div class="messenger-dialog-header-copy">
          <strong>{{ t('workspace.preview.dialogTitle') }}</strong>
          <span :title="resolvedWorkspacePath">{{ resolvedTitle }}</span>
        </div>
        <div class="messenger-dialog-header-actions">
          <button class="workspace-btn secondary" type="button" @click="emit('download')">
            <i class="fa-solid fa-download" aria-hidden="true"></i>
            {{ actionLabel }}
          </button>
          <button class="messenger-dialog-close" type="button" :aria-label="t('common.close')" @click="emit('close')">
            <i class="fa-solid fa-xmark" aria-hidden="true"></i>
          </button>
        </div>
      </div>
    </template>
    <div class="workspace-preview embed messenger-image-preview-body">
      <ZoomableImagePreview :image-url="imageUrl" :alt="resolvedTitle" :active="visible" />
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import ZoomableImagePreview from '@/components/common/ZoomableImagePreview.vue';
import { isDesktopLocalModeEnabled } from '@/config/desktop';
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
const actionLabel = computed(() =>
  isDesktopLocalModeEnabled() ? t('workspace.action.exportCopy') : t('common.download')
);
const resolvedTitle = computed(() => String(props.title || '').trim() || t('chat.imagePreview'));
const resolvedWorkspacePath = computed(
  () => String(props.workspacePath || '').trim() || t('chat.imagePreview')
);

const handleDialogVisibleChange = (nextVisible: boolean) => {
  if (nextVisible) return;
  emit('close');
};
</script>

/* Layout note: el-dialog uses append-to-body, so this dialog is teleported out
   of the component tree. Scoped `:deep()` selectors cannot reach it because the
   teleported subtree carries no data-v-* attribute. All dialog/stage sizing
   therefore lives in the global stylesheet under
   `.messenger-image-preview-dialog` in styles/messenger.css. */
<style scoped>
.messenger-image-preview-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow: hidden;
}
</style>
