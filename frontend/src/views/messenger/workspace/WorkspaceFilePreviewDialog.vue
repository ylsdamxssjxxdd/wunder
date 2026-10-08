<template>
  <el-dialog
    :model-value="visible"
    class="workspace-dialog workspace-dialog--file-preview"
    width="min(92vw, 980px)"
    top="clamp(10px, 4vh, 36px)"
    append-to-body
    :show-close="false"
    :close-on-click-modal="false"
    destroy-on-close
    @update:model-value="handleVisibleChange"
  >
    <template #header>
      <div class="messenger-dialog-header">
        <div class="messenger-dialog-header-copy">
          <strong>{{ t('messenger.filesArea.preview.title') }}</strong>
          <span :title="title">{{ title }}</span>
        </div>
        <div class="messenger-dialog-header-actions">
          <button
            v-if="editable"
            class="workspace-btn workspace-btn--primary"
            type="button"
            :disabled="saving || !dirty"
            @click="emit('save', draft)"
          >
            <i class="fa-solid fa-floppy-disk" aria-hidden="true"></i>
            {{ saving ? t('common.saving') : t('common.save') }}
          </button>
          <button class="workspace-btn secondary" type="button" :disabled="loading" @click="emit('download')">
            <i class="fa-solid fa-download" aria-hidden="true"></i>
            {{ t('common.download') }}
          </button>
          <button
            class="messenger-dialog-close"
            type="button"
            :aria-label="t('common.close')"
            @click="emit('close')"
          >
            <i class="fa-solid fa-xmark" aria-hidden="true"></i>
          </button>
        </div>
      </div>
    </template>

    <div v-if="metaLabel" class="workspace-preview-hint">{{ metaLabel }}</div>

    <div
      class="workspace-preview workspace-files-preview"
      :class="{
        embed: isEmbedded,
        'is-image': previewKind === 'image',
        'is-svg': previewKind === 'svg',
        'is-audio': previewKind === 'audio',
        'is-video': previewKind === 'video'
      }"
    >
      <div v-if="loading" class="workspace-files-preview-state">
        <span class="workspace-files-spinner" aria-hidden="true"></span>
        <span>{{ t('workspace.preview.loading') }}</span>
      </div>
      <div v-else-if="error" class="workspace-files-preview-state is-error">
        <span>{{ error }}</span>
      </div>
      <div v-else-if="tooLarge || previewKind === 'unsupported'" class="workspace-files-preview-state">
        <i class="fa-regular fa-file" aria-hidden="true"></i>
        <span>{{ hint || t('workspace.preview.unsupportedHint') }}</span>
      </div>
      <ZoomableImagePreview
        v-else-if="previewKind === 'image'"
        :image-url="previewUrl"
        :alt="title"
        :active="visible"
      />
      <iframe v-else-if="previewKind === 'pdf' || previewKind === 'svg'" :src="previewUrl"></iframe>
      <audio
        v-else-if="previewKind === 'audio'"
        class="workspace-preview-audio"
        :src="previewUrl"
        controls
        preload="metadata"
      ></audio>
      <video
        v-else-if="previewKind === 'video'"
        class="workspace-preview-video"
        :src="previewUrl"
        controls
        preload="metadata"
      ></video>
      <CodeMirrorEditor
        v-else
        :model-value="editable ? draft : content"
        :source-path="path"
        :readonly="!editable"
        light-surface
        @update:model-value="handleDraftUpdate"
      />
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import CodeMirrorEditor from '@/components/common/CodeMirrorEditor.vue';
import ZoomableImagePreview from '@/components/common/ZoomableImagePreview.vue';
import { useI18n } from '@/i18n';
import type { WorkspaceResourcePreviewKind } from '@/utils/workspaceResourcePreview';

const props = defineProps<{
  visible: boolean;
  loading: boolean;
  title: string;
  path: string;
  metaLabel: string;
  hint: string;
  error: string;
  tooLarge: boolean;
  editable: boolean;
  saving: boolean;
  previewKind: WorkspaceResourcePreviewKind;
  previewUrl: string;
  content: string;
}>();

const emit = defineEmits<{
  close: [];
  download: [];
  save: [content: string];
}>();

const { t } = useI18n();

const draft = ref('');
const dirty = ref(false);

// Keep the local draft in sync with the freshly loaded document only.
watch(
  () => props.content,
  (value) => {
    draft.value = String(value || '');
    dirty.value = false;
  },
  { immediate: true }
);

watch(
  () => props.visible,
  (visible) => {
    if (!visible) {
      dirty.value = false;
    }
  }
);

const handleDraftUpdate = (value: string) => {
  draft.value = String(value || '');
  dirty.value = draft.value !== String(props.content || '');
};

const isEmbedded = computed(() =>
  ['image', 'svg', 'pdf', 'audio', 'video'].includes(props.previewKind)
);

const handleVisibleChange = (nextVisible: boolean) => {
  if (nextVisible) return;
  emit('close');
};
</script>
