<template>
  <el-dialog
    :model-value="visible"
    class="workspace-dialog workspace-dialog--file-preview"
    width="min(92vw, 980px)"
    top="clamp(10px, 4vh, 36px)"
    append-to-body
    :show-close="false"
    :close-on-click-modal="!dirty"
    destroy-on-close
    @update:model-value="handleVisibleChange"
  >
    <template #header>
      <div class="messenger-dialog-header">
        <div class="messenger-dialog-header-copy">
          <strong>{{ t('workspace.preview.dialogTitle') }}</strong>
          <span :title="resolvedMeta">{{ resolvedTitle }}</span>
        </div>
        <div class="messenger-dialog-header-actions">
          <button
            v-if="canToggleMode"
            class="workspace-btn secondary"
            type="button"
            :title="toggleLabel"
            :aria-label="toggleLabel"
            @click="toggleMode"
          >
            <i :class="['fa-solid', toggleIcon]" aria-hidden="true"></i>
            {{ toggleLabel }}
          </button>
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

    <div v-if="hint" class="workspace-preview-hint">{{ hint }}</div>
    <div v-if="stalePreviewHint" class="workspace-preview-hint">{{ stalePreviewHint }}</div>
    <div v-if="mediaErrorHint" class="workspace-preview-hint">{{ mediaErrorHint }}</div>

    <div
      class="workspace-preview workspace-files-preview"
      :class="{
        embed: isEmbedded,
        'is-image': previewKind === 'image',
        'is-html': previewKind === 'html',
        'is-svg': previewKind === 'svg',
        'is-audio': previewKind === 'audio',
        'is-video': previewKind === 'video',
        'is-model': previewKind === 'model',
        'is-markdown-preview': mode === 'preview' && isMarkdownDocument
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
        :alt="resolvedTitle"
        :active="visible"
      />
      <iframe
        v-else-if="previewKind === 'svg' || previewKind === 'pdf'"
        :src="previewUrl"
      ></iframe>
      <audio
        v-else-if="previewKind === 'audio' && previewUrl"
        class="workspace-preview-audio"
        :src="previewUrl"
        controls
        preload="metadata"
        @error="markMediaError"
      ></audio>
      <video
        v-else-if="previewKind === 'video' && previewUrl"
        class="workspace-preview-video"
        :src="previewUrl"
        controls
        preload="metadata"
        @error="markMediaError"
      ></video>
      <div
        v-else-if="previewKind === 'audio' || previewKind === 'video'"
        class="workspace-preview-media-fallback"
      >
        <i class="fa-solid fa-triangle-exclamation" aria-hidden="true"></i>
        <span>{{ t('workspace.preview.mediaNotLoadedHint') }}</span>
      </div>
      <ModelPreview v-else-if="previewKind === 'model'" :src="previewUrl" :active="visible" />
      <iframe
        v-else-if="mode === 'preview' && previewKind === 'html'"
        :src="previewUrl"
        sandbox="allow-scripts"
      ></iframe>
      <div v-else-if="mode === 'preview' && isMarkdownDocument" class="workspace-document-markdown messenger-markdown">
        <div v-if="content" class="markdown-body" v-html="markdownHtml"></div>
        <div v-else class="workspace-empty">{{ t('workspace.preview.emptyContent') }}</div>
      </div>
      <CodeMirrorEditor
        v-else
        :model-value="editable ? draft : content"
        :source-path="path"
        :readonly="!editable"
        :placeholder="t('workspace.preview.emptyContent')"
        light-surface
        @update:model-value="handleDraftUpdate"
      />
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import CodeMirrorEditor from '@/components/common/CodeMirrorEditor.vue';
import ModelPreview from '@/components/common/ModelPreview.vue';
import ZoomableImagePreview from '@/components/common/ZoomableImagePreview.vue';
import { useI18n } from '@/i18n';
import { renderMarkdown } from '@/utils/markdown';
import {
  isWorkspaceHtmlDocumentPath,
  isWorkspaceMarkdownDocumentPath,
  type WorkspaceResourcePreviewKind
} from '@/utils/workspaceResourcePreview';

const props = defineProps<{
  visible: boolean;
  loading: boolean;
  title: string;
  path: string;
  meta: string;
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
/** 空串表示沿用文档类型的默认面，用户点过切换后才有明确偏好。 */
const userMode = ref<'preview' | 'source' | ''>('');

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
  () => [props.visible, props.path, props.previewKind],
  () => {
    userMode.value = '';
    if (!props.visible) {
      dirty.value = false;
    }
  }
);

const resolvedTitle = computed(() => String(props.title || '').trim() || t('workspace.preview.dialogTitle'));
const resolvedMeta = computed(() => String(props.meta || '').trim() || resolvedTitle.value);
const isMarkdownDocument = computed(() => isWorkspaceMarkdownDocumentPath(props.path || props.title));
const isHtmlDocument = computed(
  () => props.previewKind === 'html' || isWorkspaceHtmlDocumentPath(props.path || props.title)
);

/**
 * 两种文档各有默认面：HTML 默认看渲染结果，Markdown 默认看源码；
 * 只有能真的给出另一面时才展示切换按钮，避免点到空白页。
 */
const canToggleMode = computed(() => {
  if (props.loading || props.error || props.tooLarge) return false;
  if (props.previewKind === 'image' || props.previewKind === 'svg' || props.previewKind === 'pdf') return false;
  if (props.previewKind === 'audio' || props.previewKind === 'video' || props.previewKind === 'model') return false;
  if (props.previewKind === 'unsupported') return false;
  if (isHtmlDocument.value) return Boolean(props.previewUrl && props.content);
  if (isMarkdownDocument.value) return Boolean(props.content);
  return false;
});

const mode = computed<'preview' | 'source'>(() => {
  if (userMode.value) return userMode.value;
  // HTML 的成品是页面，默认给渲染结果；Markdown 的成品往往还在改，默认给源码。
  if (isHtmlDocument.value && props.previewUrl) return 'preview';
  return 'source';
});

const toggleLabel = computed(() => {
  if (mode.value === 'preview') {
    return props.editable ? t('workspace.preview.modeEdit') : t('workspace.preview.modeSource');
  }
  return t('workspace.preview.modePreview');
});

const toggleIcon = computed(() => (mode.value === 'preview' ? 'fa-pen' : 'fa-eye'));

const markdownHtml = computed(() =>
  mode.value === 'preview' && isMarkdownDocument.value ? renderMarkdown(String(draft.value || '')) : ''
);

/**
 * Markdown 预览直接吃草稿，所见即所写；HTML 预览页是打开时构建的产物，
 * 有未保存改动时明确提示，避免把旧渲染结果当成当前内容。
 */
const stalePreviewHint = computed(() =>
  props.previewKind === 'html' && mode.value === 'preview' && dirty.value
    ? t('workspace.preview.unsavedRenderingHint')
    : ''
);

/** 音视频解码失败时不要留一块沉默的黑舞台，直接说明是编码/文件问题。 */
const mediaError = ref(false);
const mediaErrorHint = computed(() => (mediaError.value ? t('workspace.preview.mediaDecodeFailedHint') : ''));

watch(
  () => [props.visible, props.path, props.previewUrl],
  () => {
    mediaError.value = false;
  }
);

const markMediaError = () => {
  mediaError.value = true;
};

const toggleMode = () => {
  if (!canToggleMode.value) return;
  userMode.value = mode.value === 'preview' ? 'source' : 'preview';
};

const isEmbedded = computed(
  () =>
    props.previewKind === 'image' ||
    props.previewKind === 'svg' ||
    props.previewKind === 'pdf' ||
    props.previewKind === 'audio' ||
    props.previewKind === 'video' ||
    props.previewKind === 'model' ||
    (props.previewKind === 'html' && mode.value === 'preview')
);

const handleDraftUpdate = (value: string) => {
  draft.value = String(value || '');
  dirty.value = draft.value !== String(props.content || '');
};

const handleVisibleChange = (nextVisible: boolean) => {
  if (nextVisible) return;
  emit('close');
};
</script>
