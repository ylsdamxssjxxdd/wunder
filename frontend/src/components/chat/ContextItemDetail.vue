<template>
  <div class="cx-detail-overlay" @mousedown.self="emit('close')">
    <div
      class="cx-detail-card cx-detail-card--item"
      :style="{ height: state.text !== '' ? '520px' : '220px' }"
      role="dialog"
      aria-modal="true"
    >
      <div class="cx-detail-head">
        <span class="cx-detail-dot" :style="{ background: palette.ink }"></span>
        <span class="cx-detail-title">{{ state.label }}</span>
        <button class="cx-detail-close" type="button" :aria-label="t('common.close')" @click="emit('close')">
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
      </div>
      <div class="cx-detail-sub">
        {{ partLabel }} · {{ state.tokensText }} token · {{ state.pctText }}
      </div>
      <div class="cx-detail-divider"></div>
      <div class="cx-detail-body">
        <div v-if="state.loading" class="cx-detail-note">{{ t('common.loading') }}</div>
        <div v-else-if="state.text === ''" class="cx-detail-note">
          {{ t('chat.contextWindow.textReleased') }}
        </div>
        <pre v-else class="cx-detail-text">{{ state.text }}</pre>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import { useI18n } from '@/i18n';
import { paletteFor, type ItemDetailState } from '@/components/chat/contextWindowView';

const props = defineProps<{
  state: ItemDetailState;
  partLabel: string;
}>();

const emit = defineEmits<{ close: [] }>();

const { t } = useI18n();

const palette = computed(() => paletteFor(props.state.slot));
</script>

<style scoped>
.cx-detail-overlay {
  position: fixed;
  inset: 0;
  z-index: 2100;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 12px;
  background: rgba(15, 18, 22, 0.35);
}

.cx-detail-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  box-sizing: border-box;
  width: min(560px, calc(100vw - 24px));
  padding: 20px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 12px;
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 16px 48px rgba(15, 18, 22, 0.24);
}

.cx-detail-head {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 24px;
}

.cx-detail-dot {
  flex: 0 0 auto;
  width: 8px;
  height: 8px;
  border-radius: 4px;
}

.cx-detail-title {
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 15px;
  font-weight: 700;
  color: var(--mz-text, #1f2329);
}

.cx-detail-close {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 16px;
  cursor: pointer;
}

.cx-detail-close:hover {
  background: var(--mz-hover, #f1efec);
}

.cx-detail-sub {
  height: 18px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  line-height: 18px;
}

.cx-detail-divider {
  height: 1px;
  background: var(--mz-border, #e8e6e3);
}

.cx-detail-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
}

.cx-detail-note {
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  line-height: 1.6;
}

.cx-detail-text {
  margin: 0;
  padding: 8px 10px;
  border-radius: 6px;
  background: var(--mz-hover, #f1efec);
  color: var(--mz-text, #1f2329);
  font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
  font-size: 11.5px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
