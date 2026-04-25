<template>
  <el-dialog
    v-model="visibleProxy"
    width="min(1160px, calc(100vw - 20px))"
    top="clamp(8px, 2vh, 20px)"
    class="messenger-modal messenger-modal--beeroom orchestration-theme-dialog orchestration-artifact-preview-dialog"
    append-to-body
    destroy-on-close
  >
    <template #header>
      <div class="messenger-modal-header orchestration-artifact-preview-header">
        <div class="orchestration-artifact-preview-heading">
          <div class="messenger-modal-title">{{ resolvedTitle }}</div>
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
          <button
            class="beeroom-canvas-icon-btn orchestration-artifact-preview-btn"
            type="button"
            :title="t('common.download')"
            :aria-label="t('common.download')"
            :disabled="loading || !resolvedPath"
            @click="emit('download')"
          >
            <i class="fa-solid fa-download" aria-hidden="true"></i>
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
      <div
        v-else
        ref="previewBodyRef"
        class="orchestration-artifact-preview-body"
        :style="previewBodyStyle"
      >
        <div class="orchestration-artifact-preview-bubble messenger-message-bubble messenger-markdown">
          <BeeroomCanvasChatMarkdown
            :cache-key="`${resolvedPath}:${sourceContent.length}`"
            :content="fallbackContent"
          />
        </div>
      </div>
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, nextTick, ref } from 'vue';

import BeeroomCanvasChatMarkdown from '@/components/beeroom/BeeroomCanvasChatMarkdown.vue';
import { useI18n } from '@/i18n';

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
  (event: 'download'): void;
}>();

const { t } = useI18n();
const scale = ref(1);
const previewBodyRef = ref<HTMLDivElement | null>(null);
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

const canZoomOut = computed(() => scale.value > MIN_SCALE + 0.001);
const canZoomIn = computed(() => scale.value < MAX_SCALE - 0.001);
const scaleLabel = computed(() => `${Math.round(scale.value * 100)}%`);
const previewBodyStyle = computed(() => ({
  '--artifact-preview-scale': String(scale.value)
}));

const clampScale = (value: number) => Math.min(MAX_SCALE, Math.max(MIN_SCALE, value));
const captureScrollAnchor = () => {
  const container = previewBodyRef.value;
  if (!container) return null;
  return {
    centerXRatio:
      container.scrollWidth > 0
        ? (container.scrollLeft + container.clientWidth / 2) / container.scrollWidth
        : 0.5,
    centerYRatio:
      container.scrollHeight > 0
        ? (container.scrollTop + container.clientHeight / 2) / container.scrollHeight
        : 0.5
  };
};

const restoreScrollAnchor = (anchor: ReturnType<typeof captureScrollAnchor>) => {
  if (!anchor) return;
  nextTick(() => {
    const container = previewBodyRef.value;
    if (!container) return;
    const nextScrollLeft = Math.round(anchor.centerXRatio * container.scrollWidth - container.clientWidth / 2);
    const nextScrollTop = Math.round(anchor.centerYRatio * container.scrollHeight - container.clientHeight / 2);
    container.scrollLeft = Math.max(0, nextScrollLeft);
    container.scrollTop = Math.max(0, nextScrollTop);
    if (typeof window !== 'undefined') {
      window.requestAnimationFrame(() => {
        const currentContainer = previewBodyRef.value;
        if (!currentContainer) return;
        currentContainer.scrollLeft = Math.max(0, nextScrollLeft);
        currentContainer.scrollTop = Math.max(0, nextScrollTop);
      });
    }
  });
};

const setScale = (nextScale: number) => {
  const roundedNextScale = Math.round(clampScale(nextScale) * 100) / 100;
  if (Math.abs(roundedNextScale - scale.value) < 0.001) return;
  const anchor = captureScrollAnchor();
  scale.value = roundedNextScale;
  restoreScrollAnchor(anchor);
};

const zoomOut = () => {
  setScale(scale.value - SCALE_STEP);
};
const zoomIn = () => {
  setScale(scale.value + SCALE_STEP);
};
const resetZoom = () => {
  setScale(1);
};
</script>

<style scoped>
.orchestration-artifact-preview-dialog :deep(.el-dialog) {
  max-height: calc(100vh - 16px);
}

.orchestration-artifact-preview-dialog :deep(.el-dialog__body) {
  padding: 8px 24px 12px;
}

.orchestration-artifact-preview-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  min-height: 30px;
}

.orchestration-artifact-preview-heading {
  flex: 1 1 auto;
  min-width: 0;
  padding-right: 8px;
}

.orchestration-artifact-preview-heading .messenger-modal-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.orchestration-artifact-preview-toolbar {
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  flex: 0 0 auto;
  gap: 6px;
  padding: 4px;
  border-radius: 12px;
  background: rgba(15, 23, 42, 0.72);
  box-shadow:
    inset 0 0 0 1px rgba(148, 163, 184, 0.16),
    0 8px 18px rgba(2, 6, 23, 0.16);
}

.orchestration-artifact-preview-btn {
  width: 32px;
  height: 32px;
  min-width: 32px;
  min-height: 32px;
  border: 1px solid rgba(148, 163, 184, 0.18);
  border-radius: 10px;
  outline: none;
  appearance: none;
  background: rgba(30, 41, 59, 0.72);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.05),
    0 4px 10px rgba(2, 6, 23, 0.12);
  color: rgba(241, 245, 249, 0.96);
  transition:
    background-color 0.18s ease,
    border-color 0.18s ease,
    box-shadow 0.18s ease,
    color 0.18s ease,
    opacity 0.18s ease,
    transform 0.18s ease;
}

.orchestration-artifact-preview-btn:hover:not(:disabled),
.orchestration-artifact-preview-btn:focus-visible:not(:disabled) {
  background: rgba(59, 130, 246, 0.2);
  border-color: rgba(96, 165, 250, 0.4);
  color: #ffffff;
  transform: translateY(-1px);
  box-shadow:
    inset 0 1px 0 rgba(255, 255, 255, 0.08),
    0 10px 18px rgba(30, 64, 175, 0.18);
}

.orchestration-artifact-preview-btn:disabled {
  background: rgba(15, 23, 42, 0.28);
  border-color: rgba(148, 163, 184, 0.1);
  color: rgba(148, 163, 184, 0.42);
  cursor: not-allowed;
  box-shadow: none;
}

.orchestration-artifact-preview-scale {
  width: auto;
  min-width: 52px;
  padding: 0 10px;
  font-size: 12px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
  color: rgba(248, 250, 252, 0.94);
}

.orchestration-artifact-preview-shell {
  min-height: min(80vh, 860px);
  max-height: min(80vh, 860px);
  display: flex;
  flex-direction: column;
}

.orchestration-artifact-preview-state {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: min(80vh, 860px);
  color: rgba(148, 163, 184, 0.92);
  font-size: 14px;
}

.orchestration-artifact-preview-state.is-error {
  color: #fca5a5;
}

.orchestration-artifact-preview-body {
  flex: 1 1 auto;
  display: flex;
  justify-content: center;
  align-items: flex-start;
  min-height: 0;
  overflow: auto;
  padding: 0 0 8px;
}

.orchestration-artifact-preview-bubble {
  margin: 0;
  transform: scale(var(--artifact-preview-scale, 1));
  transform-origin: top center;
  border-radius: 20px;
  width: calc(min(100%, 1120px) / var(--artifact-preview-scale, 1));
  max-width: calc(100% / var(--artifact-preview-scale, 1));
  padding: 18px 22px 24px;
  box-sizing: border-box;
  box-shadow: 0 14px 30px rgba(15, 23, 42, 0.12);
}

.orchestration-artifact-preview-bubble :deep(.beeroom-chat-markdown),
.orchestration-artifact-preview-bubble :deep(.markdown-body) {
  width: 100%;
}

@media (max-width: 900px) {
  .orchestration-artifact-preview-header {
    align-items: center;
  }

  .orchestration-artifact-preview-toolbar {
    justify-content: flex-end;
  }
}
</style>
