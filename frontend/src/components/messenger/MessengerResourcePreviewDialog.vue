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
          <span :title="resolvedMeta">{{ resolvedTitle }}</span>
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
    <div v-if="hint" class="workspace-preview-hint">{{ hint }}</div>
    <div
      class="workspace-preview messenger-image-preview-body"
      :class="{
        embed: isEmbedded,
        'is-svg': previewKind === 'svg',
        'is-audio': previewKind === 'audio',
        'is-video': previewKind === 'video',
        'is-text': previewKind === 'text' || !isEmbedded
      }"
    >
      <div v-if="loading" class="workspace-empty">{{ t('workspace.preview.loading') }}</div>
      <template v-else>
        <ZoomableImagePreview
          v-if="previewKind === 'image'"
          :image-url="src"
          :alt="resolvedTitle"
          :active="visible"
        />
        <iframe v-else-if="previewKind === 'pdf' || previewKind === 'svg'" :src="src"></iframe>
        <audio
          v-else-if="previewKind === 'audio'"
          class="workspace-preview-audio"
          :src="src"
          controls
          preload="metadata"
        ></audio>
        <video
          v-else-if="previewKind === 'video'"
          class="workspace-preview-video"
          :src="src"
          controls
          preload="metadata"
        ></video>
        <WorkspaceTextPreview
          v-else-if="previewKind === 'text'"
          :content="content"
          :source-path="sourcePath"
          wrapper-class="messenger-markdown"
          light-surface
        />
        <pre v-else class="workspace-preview-text">{{ content || t('workspace.preview.emptyContent') }}</pre>
      </template>
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import ZoomableImagePreview from '@/components/common/ZoomableImagePreview.vue';
import WorkspaceTextPreview from '@/components/common/WorkspaceTextPreview.vue';
import { isDesktopLocalModeEnabled } from '@/config/desktop';
import { useI18n } from '@/i18n';
import type { WorkspaceResourcePreviewKind } from '@/utils/workspaceResourcePreview';

const props = defineProps<{
  visible: boolean;
  loading: boolean;
  title: string;
  meta: string;
  hint: string;
  src: string;
  content: string;
  sourcePath: string;
  previewKind: WorkspaceResourcePreviewKind;
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
const resolvedTitle = computed(() => String(props.title || '').trim() || t('workspace.preview.dialogTitle'));
const resolvedMeta = computed(() => String(props.meta || '').trim() || resolvedTitle.value);
const isEmbedded = computed(() =>
  props.previewKind === 'image' ||
  props.previewKind === 'svg' ||
  props.previewKind === 'pdf' ||
  props.previewKind === 'audio' ||
  props.previewKind === 'video'
);

const handleDialogVisibleChange = (nextVisible: boolean) => {
  if (nextVisible) return;
  emit('close');
};
</script>

<style scoped>
.messenger-image-preview-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow: hidden;
}

.messenger-image-preview-body.is-text {
  overflow: auto;
}

.messenger-image-preview-body iframe {
  height: 100%;
  min-height: 0;
}

:deep(.messenger-image-preview-dialog.el-dialog) {
  max-width: min(92vw, 980px);
  max-height: calc(var(--app-viewport-height, 100vh) - 24px);
  margin: 12px auto !important;
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

:deep(.messenger-image-preview-body .zoomable-image-surface) {
  height: 100%;
}

:deep(.messenger-image-preview-body .zoomable-image-stage) {
  height: 100%;
  min-height: 0;
  max-height: none;
}

</style>
