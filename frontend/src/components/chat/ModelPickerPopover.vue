<template>
  <div
    class="composer-model-popover"
    role="dialog"
    :aria-label="t('chat.composer.modelSelect')"
    @click.stop
  >
    <!-- 上下文占用：浮层顶部只给数字（进度条已下线），溢出时补一句提示。 -->
    <div class="composer-model-context">
      <span class="composer-model-footer-title">{{ t('profile.stats.contextTokens') }}</span>
      <span class="composer-model-context-value" :class="contextUsage.level" data-testid="model-popover-usage">
        {{ contextUsage.counts }} · {{ contextUsage.percentText }}
      </span>
    </div>
    <div v-if="contextUsage.overflow" class="composer-model-context-hint">
      {{ t('chat.composer.status.contextOverflow') }}
    </div>

    <div class="composer-model-list" role="listbox">
      <div v-if="loading && !items.length" class="composer-model-state">
        <span class="composer-model-state-spinner" aria-hidden="true"></span>
        <span>{{ t('common.loading') }}</span>
      </div>
      <button
        v-else-if="failed && !items.length"
        class="composer-model-state composer-model-state--action"
        type="button"
        @click="emit('retry')"
      >
        <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
        <span>{{ t('chat.composer.modelReload') }}</span>
      </button>
      <div v-else-if="!items.length" class="composer-model-state">
        <i class="fa-solid fa-circle-info" aria-hidden="true"></i>
        <span>{{ t('chat.composer.modelEmpty') }}</span>
      </div>
      <template v-else>
        <button
          v-for="item in visibleItems"
          :key="item.id"
          class="composer-model-row"
          :class="{ 'is-active': isActiveModel(item) }"
          type="button"
          role="option"
          :aria-selected="isActiveModel(item)"
          :disabled="busy"
          @click="emit('select', item.id)"
        >
          <span class="composer-model-row-check" aria-hidden="true">
            <i v-if="isActiveModel(item)" class="fa-solid fa-check"></i>
          </span>
          <i :class="[resolveModelIcon(item), 'composer-model-row-icon']" aria-hidden="true"></i>
          <span class="composer-model-row-main">
            <span class="composer-model-row-name" :title="item.name">{{ item.name }}</span>
            <span v-if="resolveModelTags(item).length" class="composer-model-row-tags">
              <span
                v-for="tag in resolveModelTags(item)"
                :key="tag.key"
                class="composer-model-tag"
                :class="tag.className"
              >
                {{ tag.label }}
              </span>
            </span>
          </span>
        </button>
      </template>
    </div>

    <div class="composer-model-footer">
      <div class="composer-model-footer-title">{{ t('desktop.system.reasoningEffort') }}</div>
      <div class="composer-model-effort" role="menu">
        <button
          v-for="option in effortOptions"
          :key="option.value"
          class="composer-model-effort-item"
          :class="{ 'is-active': reasoningEffort === option.value }"
          type="button"
          role="menuitemradio"
          :aria-checked="reasoningEffort === option.value"
          @click="emit('effort', option.value)"
        >
          {{ option.label }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, type PropType } from 'vue';

import { useI18n } from '@/i18n';
import type { ComposerModelOption } from '@/components/chat/composerModelCatalog';

type EffortOption = { value: string; label: string };

type ModelPickerContextUsage = {
  ratio: number | null;
  percentText: string;
  counts: string;
  level: '' | 'is-warning' | 'is-danger';
  overflow: boolean;
};

const props = defineProps({
  items: {
    type: Array as PropType<ComposerModelOption[]>,
    default: () => []
  },
  activeModelId: {
    type: String,
    default: ''
  },
  contextUsage: {
    type: Object as PropType<ModelPickerContextUsage>,
    required: true
  },
  loading: {
    type: Boolean,
    default: false
  },
  failed: {
    type: Boolean,
    default: false
  },
  busy: {
    type: Boolean,
    default: false
  },
  reasoningEffort: {
    type: String,
    default: 'default'
  },
  reasoningEffortOptions: {
    type: Array as PropType<EffortOption[]>,
    default: () => []
  }
});

const emit = defineEmits(['select', 'effort', 'retry', 'close']);

const { t } = useI18n();
const MAX_VISIBLE_ROWS = 80;

const modelItems = computed<ComposerModelOption[]>(() =>
  (Array.isArray(props.items) ? props.items : []).filter(
    (item): item is ComposerModelOption => Boolean(item && typeof item === 'object')
  )
);

// 目录有界（<=200），渲染窗口再收一道，避免一次性铺满 DOM。
const visibleItems = computed(() => modelItems.value.slice(0, MAX_VISIBLE_ROWS));

const effortOptions = computed<EffortOption[]>(() =>
  (Array.isArray(props.reasoningEffortOptions) ? props.reasoningEffortOptions : [])
    .filter((item): item is EffortOption => Boolean(item && typeof item === 'object'))
    .map((item) => ({
      value: String(item.value || '').trim(),
      label: String(item.label || item.value || '').trim()
    }))
    .filter((item) => Boolean(item.value && item.label))
);

const isActiveModel = (item: ComposerModelOption): boolean => {
  const active = String(props.activeModelId || '').trim().toLowerCase();
  if (!active) return false;
  return item.id.toLowerCase() === active || item.name.toLowerCase() === active;
};

const resolveModelIcon = (item: ComposerModelOption): string => {
  if (item.source === 'system') return 'fa-solid fa-server';
  if (item.source === 'user') return 'fa-solid fa-user-gear';
  return 'fa-solid fa-brain';
};

const formatContext = (tokens: number): string => {
  if (tokens >= 1000000) {
    const millions = tokens / 1000000;
    return `${Number.isInteger(millions) ? millions : millions.toFixed(1)}M`;
  }
  if (tokens >= 1000) return `${Math.round(tokens / 1000)}K`;
  return String(tokens);
};

const resolveModelTags = (item: ComposerModelOption): Array<{
  key: string;
  label: string;
  className: string;
}> => {
  const tags: Array<{ key: string; label: string; className: string }> = [];
  if (item.context !== null) {
    tags.push({
      key: 'context',
      label: formatContext(item.context),
      className: 'composer-model-tag--context'
    });
  }
  if (item.source === 'system') {
    tags.push({
      key: 'source',
      label: t('chat.composer.modelSourceSystem'),
      className: 'composer-model-tag--source'
    });
  } else if (item.source === 'user') {
    tags.push({
      key: 'source',
      label: t('chat.composer.modelSourceUser'),
      className: 'composer-model-tag--source'
    });
  }
  if (item.isDefault) {
    tags.push({
      key: 'default',
      label: t('chat.composer.modelDefault'),
      className: 'composer-model-tag--default'
    });
  }
  return tags;
};
</script>

<style scoped>
/* §8.3 popover: ~300px, above the trigger, white card with 12px radius. */
.composer-model-popover {
  position: absolute;
  right: 0;
  bottom: calc(100% + 8px);
  z-index: 40;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  width: 300px;
  max-width: calc(100vw - 40px);
  /* 矮窗口下浮层向上长过视口：整卡限高，模型列表先让位。 */
  max-height: calc(100vh - 32px);
  padding: 8px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 12px;
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 12px 32px rgba(31, 35, 41, 0.16);
}

.composer-model-list {
  display: flex;
  flex: 0 1 auto;
  flex-direction: column;
  gap: 1px;
  min-height: 0;
  max-height: 248px;
  overflow-y: auto;
}

.composer-model-row {
  display: flex;
  align-items: center;
  gap: 8px;
  box-sizing: border-box;
  width: 100%;
  padding: 6px 8px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--mz-text, #1f2329);
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.composer-model-row:hover:not(:disabled) {
  background: var(--mz-hover, #f1efec);
}

.composer-model-row.is-active {
  color: var(--mz-primary, #c96443);
}

.composer-model-row:disabled {
  cursor: progress;
  opacity: 0.7;
}

.composer-model-row:focus-visible {
  outline: 2px solid var(--mz-primary-soft, #dfac9a);
  outline-offset: -2px;
}

.composer-model-row-check {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 12px;
  color: var(--mz-primary, #c96443);
  font-size: 10px;
}

.composer-model-row-icon {
  flex-shrink: 0;
  width: 14px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  text-align: center;
}

.composer-model-row-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.composer-model-row-name {
  min-width: 0;
  overflow: hidden;
  font-size: 13px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composer-model-row-tags {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: wrap;
}

.composer-model-tag {
  padding: 0 5px;
  border-radius: 4px;
  background: var(--mz-hover, #f1efec);
  color: var(--mz-text-muted, #8a8f99);
  font-size: 10px;
  line-height: 15px;
}

.composer-model-tag--default {
  background: var(--mz-primary-tint, #f6e9e3);
  color: var(--mz-primary, #c96443);
}

.composer-model-state {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 18px 10px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  line-height: 1.4;
  text-align: center;
}

.composer-model-state--action {
  width: 100%;
  border: 0;
  background: transparent;
  color: var(--mz-primary, #c96443);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.composer-model-state-spinner {
  width: 12px;
  height: 12px;
  border: 2px solid rgba(31, 35, 41, 0.14);
  border-top-color: var(--mz-primary, #c96443);
  border-radius: 50%;
  animation: composer-model-state-spin 640ms linear infinite;
}

@keyframes composer-model-state-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .composer-model-state-spinner {
    animation: none;
  }
}

.composer-model-footer {
  flex-shrink: 0;
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px solid var(--mz-border, #e8e6e3);
}

.composer-model-footer-title {
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-weight: 600;
}

/* 浮层顶部一行：标题在左、数字在右。进度条已下线，占用只用数字表达。 */
.composer-model-context {
  display: flex;
  flex: 0 0 auto;
  align-items: baseline;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 6px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--mz-border, #e8e6e3);
}

.composer-model-context-value {
  min-width: 0;
  overflow: hidden;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composer-model-context-value.is-warning {
  color: var(--mz-warning, #c98a2b);
}

.composer-model-context-value.is-danger {
  color: var(--mz-danger, #d04a43);
}

.composer-model-context-hint {
  color: var(--mz-danger, #d04a43);
  font-size: 11px;
}

.composer-model-effort {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 4px;
}

.composer-model-effort-item {
  padding: 3px 8px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 999px;
  background: transparent;
  color: var(--mz-text-secondary, #3d3d3d);
  font: inherit;
  font-size: 11px;
  cursor: pointer;
}

.composer-model-effort-item:hover {
  border-color: var(--mz-primary-soft, #dfac9a);
}

.composer-model-effort-item.is-active {
  border-color: transparent;
  background: var(--mz-primary-tint, #f6e9e3);
  color: var(--mz-primary, #c96443);
  font-weight: 600;
}

.composer-model-effort-item:focus-visible {
  outline: 2px solid var(--mz-primary-soft, #dfac9a);
  outline-offset: 1px;
}
</style>
