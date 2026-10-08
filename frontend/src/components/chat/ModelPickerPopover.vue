<template>
  <div
    class="composer-model-popover"
    role="dialog"
    :aria-label="t('chat.composer.modelSelect')"
    @click.stop
  >
    <div class="composer-model-search">
      <i class="fa-solid fa-magnifying-glass composer-model-search-icon" aria-hidden="true"></i>
      <input
        ref="searchRef"
        v-model="search"
        class="composer-model-search-input"
        type="text"
        :placeholder="t('chat.composer.modelSearch')"
        :aria-label="t('chat.composer.modelSearch')"
        @keydown.esc.prevent="emit('close')"
      />
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
      <div v-else-if="!filteredItems.length" class="composer-model-state">
        <i class="fa-solid fa-circle-info" aria-hidden="true"></i>
        <span>{{ emptyHint }}</span>
      </div>
      <template v-else>
        <button
          v-for="item in filteredItems"
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
          v-for="option in reasoningEffortOptions"
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
      <label
        class="composer-model-default-row"
        :class="{ 'is-disabled': !supportsUserDefault }"
        :title="supportsUserDefault ? '' : t('chat.composer.modelSetDefaultUnsupported')"
      >
        <input
          class="composer-model-default-check"
          type="checkbox"
          :checked="setAsDefault"
          :disabled="!supportsUserDefault"
          @change="handleSetAsDefaultChange"
        />
        <span class="composer-model-default-label">{{ t('chat.composer.modelSetDefault') }}</span>
        <span v-if="supportsUserDefault && userDefaultModelName" class="composer-model-default-value">
          {{ t('chat.composer.modelUserDefault', { name: userDefaultModelName }) }}
        </span>
      </label>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import type { PropType } from 'vue';

import { useI18n } from '@/i18n';
import type { ComposerModelOption } from '@/components/chat/composerModelCatalog';

type EffortOption = { value: string; label: string };

const props = defineProps({
  items: {
    type: Array as PropType<ComposerModelOption[]>,
    default: () => []
  },
  activeModelId: {
    type: String,
    default: ''
  },
  activeModelLabel: {
    type: String,
    default: ''
  },
  defaultModelName: {
    type: String,
    default: ''
  },
  userDefaultModelName: {
    type: String,
    default: ''
  },
  supportsUserDefault: {
    type: Boolean,
    default: false
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

const emit = defineEmits(['select', 'effort', 'set-default', 'retry', 'close']);

const { t } = useI18n();
const search = ref('');
const setAsDefault = ref(false);
const searchRef = ref<HTMLInputElement | null>(null);
const MAX_VISIBLE_ROWS = 80;

const modelItems = computed<ComposerModelOption[]>(() =>
  (Array.isArray(props.items) ? props.items : []).filter(
    (item): item is ComposerModelOption => Boolean(item && typeof item === 'object')
  )
);

const effortOptions = computed<EffortOption[]>(() =>
  (Array.isArray(props.reasoningEffortOptions) ? props.reasoningEffortOptions : [])
    .filter((item): item is EffortOption => Boolean(item && typeof item === 'object'))
    .map((item) => ({
      value: String(item.value || '').trim(),
      label: String(item.label || item.value || '').trim()
    }))
    .filter((item) => Boolean(item.value && item.label))
);

// The catalog is bounded (<=200) and search only runs while the popover is open.
const filteredItems = computed<ComposerModelOption[]>(() => {
  const keyword = search.value.trim().toLowerCase();
  const source = modelItems.value;
  const matched = keyword
    ? source.filter(
        (item) =>
          item.name.toLowerCase().includes(keyword) || item.id.toLowerCase().includes(keyword)
      )
    : source;
  return matched.slice(0, MAX_VISIBLE_ROWS);
});

const emptyHint = computed(() =>
  search.value.trim() ? t('chat.composer.modelNoMatch') : t('chat.composer.modelEmpty')
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

const handleSetAsDefaultChange = (event: Event) => {
  if (!props.supportsUserDefault) return;
  const target = event.target as HTMLInputElement | null;
  setAsDefault.value = target?.checked === true;
  emit('set-default', setAsDefault.value);
};

onMounted(() => {
  void nextTick(() => {
    searchRef.value?.focus();
  });
});

// Keep the checkbox honest: it mirrors the server-reported user default when the
// contract advertises it, and stays unchecked (disabled) otherwise.
watch(
  () => [props.supportsUserDefault, props.userDefaultModelName] as const,
  ([supported, userDefault]) => {
    if (!supported) {
      setAsDefault.value = false;
      return;
    }
    setAsDefault.value = Boolean(userDefault) && userDefault === props.activeModelId;
  },
  { immediate: true }
);
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
  padding: 8px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 12px;
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 12px 32px rgba(31, 35, 41, 0.16);
}

.composer-model-search {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 8px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 8px;
  background: var(--mz-bg, #fbfaf8);
}

.composer-model-search-icon {
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
}

.composer-model-search-input {
  flex: 1 1 auto;
  min-width: 0;
  height: 30px;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--mz-text, #1f2329);
  font: inherit;
  font-size: 12px;
}

.composer-model-list {
  display: flex;
  flex-direction: column;
  gap: 1px;
  max-height: 248px;
  margin-top: 6px;
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
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px solid var(--mz-border, #e8e6e3);
}

.composer-model-footer-title {
  padding: 2px 4px 4px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  font-weight: 600;
}

.composer-model-effort {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
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

.composer-model-default-row {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 8px;
  padding: 0 4px;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 12px;
  cursor: pointer;
}

.composer-model-default-row.is-disabled {
  color: var(--mz-text-muted, #8a8f99);
  cursor: not-allowed;
}

.composer-model-default-check {
  width: 13px;
  height: 13px;
  margin: 0;
  accent-color: var(--mz-primary, #c96443);
}

.composer-model-default-value {
  margin-left: auto;
  overflow: hidden;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
