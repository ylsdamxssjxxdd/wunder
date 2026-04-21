<template>
  <el-dialog
    v-model="visibleProxy"
    width="min(1100px, calc(100vw - 28px))"
    top="clamp(10px, 3vh, 28px)"
    class="messenger-modal messenger-modal--beeroom orchestration-theme-dialog orchestration-artifact-preview-dialog"
    append-to-body
    destroy-on-close
  >
    <template #header>
      <div class="messenger-modal-header orchestration-artifact-preview-header">
        <div class="orchestration-artifact-preview-heading">
          <div class="messenger-modal-title">{{ resolvedTitle }}</div>
          <div class="messenger-modal-subtitle">{{ resolvedPath }}</div>
        </div>
        <div class="orchestration-artifact-preview-toolbar">
          <button
            class="beeroom-canvas-icon-btn orchestration-artifact-preview-btn"
            type="button"
            :title="t('common.zoomOut')"
            :aria-label="t('common.zoomOut')"
            :disabled="!canZoomOut"
            @click="zoomOut"
          >
            <i class="fa-solid fa-magnifying-glass-minus" aria-hidden="true"></i>
          </button>
          <button
            class="beeroom-canvas-icon-btn orchestration-artifact-preview-btn orchestration-artifact-preview-scale"
            type="button"
            :title="t('common.reset')"
            :aria-label="t('common.reset')"
            @click="resetZoom"
          >
            {{ scaleLabel }}
          </button>
          <button
            class="beeroom-canvas-icon-btn orchestration-artifact-preview-btn"
            type="button"
            :title="t('common.zoomIn')"
            :aria-label="t('common.zoomIn')"
            :disabled="!canZoomIn"
            @click="zoomIn"
          >
            <i class="fa-solid fa-magnifying-glass-plus" aria-hidden="true"></i>
          </button>
        </div>
      </div>
    </template>

    <div class="orchestration-artifact-preview-shell">
      <div v-if="loading" class="orchestration-artifact-preview-state">
        {{ t('workspace.preview.loading') }}
      </div>
      <div v-else-if="error" class="orchestration-artifact-preview-state is-error">
        {{ error }}
      </div>
      <div v-else class="orchestration-artifact-preview-body" :style="previewBodyStyle">
        <div
          v-if="renderedHtml"
          ref="contentRef"
          class="orchestration-artifact-preview-markdown messenger-markdown"
          v-html="renderedHtml"
        ></div>
        <pre v-else class="orchestration-artifact-preview-plain">{{ fallbackContent }}</pre>
      </div>
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

import { useI18n } from '@/i18n';
import { renderMarkdown } from '@/utils/markdown';

const props = defineProps<{
  visible: boolean;
  title?: string;
  path?: string;
  content?: string;
  loading?: boolean;
  error?: string;
}>();

const emit = defineEmits<{
  (event: 'update:visible', value: boolean): void;
}>();

const { t } = useI18n();
const contentRef = ref<HTMLElement | null>(null);
const scale = ref(1);
const MIN_SCALE = 0.8;
const MAX_SCALE = 1.8;
const SCALE_STEP = 0.1;

const visibleProxy = computed({
  get: () => props.visible,
  set: (value: boolean) => emit('update:visible', value)
});

const resolvedTitle = computed(() => String(props.title || '').trim() || t('workspace.preview.dialogTitle'));
const resolvedPath = computed(() => String(props.path || '').trim());
const sourceContent = computed(() => String(props.content || ''));
const fallbackContent = computed(() => sourceContent.value || t('workspace.preview.emptyContent'));
const renderedHtml = computed(() => {
  const source = sourceContent.value.trim();
  if (!source) return '';
  return renderMarkdown(source);
});

const canZoomOut = computed(() => scale.value > MIN_SCALE + 0.001);
const canZoomIn = computed(() => scale.value < MAX_SCALE - 0.001);
const scaleLabel = computed(() => `${Math.round(scale.value * 100)}%`);
const previewBodyStyle = computed(() => ({
  '--artifact-preview-scale': String(scale.value)
}));

const clampScale = (value: number) => Math.min(MAX_SCALE, Math.max(MIN_SCALE, value));
const zoomOut = () => {
  scale.value = Math.round(clampScale(scale.value - SCALE_STEP) * 100) / 100;
};
const zoomIn = () => {
  scale.value = Math.round(clampScale(scale.value + SCALE_STEP) * 100) / 100;
};
const resetZoom = () => {
  scale.value = 1;
};
</script>

<style scoped>
.orchestration-artifact-preview-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.orchestration-artifact-preview-heading {
  min-width: 0;
}

.orchestration-artifact-preview-toolbar {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.orchestration-artifact-preview-btn {
  min-width: 38px;
}

.orchestration-artifact-preview-scale {
  min-width: 72px;
  font-weight: 700;
}

.orchestration-artifact-preview-shell {
  min-height: min(72vh, 760px);
  max-height: min(72vh, 760px);
  display: flex;
  flex-direction: column;
}

.orchestration-artifact-preview-state {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: min(72vh, 760px);
  color: rgba(148, 163, 184, 0.92);
  font-size: 14px;
}

.orchestration-artifact-preview-state.is-error {
  color: #fca5a5;
}

.orchestration-artifact-preview-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow: auto;
  padding: 20px 22px 28px;
  border-radius: 20px;
  background:
    linear-gradient(180deg, rgba(15, 23, 42, 0.92), rgba(15, 23, 42, 0.98)),
    radial-gradient(circle at top left, rgba(16, 185, 129, 0.14), transparent 40%);
  border: 1px solid rgba(148, 163, 184, 0.16);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

.orchestration-artifact-preview-markdown,
.orchestration-artifact-preview-plain {
  transform: scale(var(--artifact-preview-scale, 1));
  transform-origin: top left;
  width: calc(100% / var(--artifact-preview-scale, 1));
}

.orchestration-artifact-preview-plain {
  margin: 0;
  color: rgba(241, 245, 249, 0.96);
  white-space: pre-wrap;
  word-break: break-word;
  font-family: 'JetBrains Mono', 'Cascadia Code', 'Consolas', monospace;
  line-height: 1.68;
}

.orchestration-artifact-preview-markdown :deep(.markdown-body) {
  color: rgba(241, 245, 249, 0.96);
}

.orchestration-artifact-preview-markdown :deep(h1),
.orchestration-artifact-preview-markdown :deep(h2),
.orchestration-artifact-preview-markdown :deep(h3),
.orchestration-artifact-preview-markdown :deep(h4) {
  color: #f8fafc;
}

.orchestration-artifact-preview-markdown :deep(pre),
.orchestration-artifact-preview-markdown :deep(code) {
  font-family: 'JetBrains Mono', 'Cascadia Code', 'Consolas', monospace;
}

.orchestration-artifact-preview-markdown :deep(pre) {
  background: rgba(2, 6, 23, 0.82);
  border: 1px solid rgba(148, 163, 184, 0.14);
}

.orchestration-artifact-preview-markdown :deep(blockquote) {
  border-left-color: rgba(16, 185, 129, 0.72);
  background: rgba(15, 118, 110, 0.12);
}

.orchestration-artifact-preview-markdown :deep(table) {
  background: rgba(15, 23, 42, 0.56);
}

@media (max-width: 900px) {
  .orchestration-artifact-preview-header {
    flex-direction: column;
    align-items: stretch;
  }

  .orchestration-artifact-preview-toolbar {
    justify-content: flex-end;
  }
}
</style>
