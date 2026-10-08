<template>
  <div class="composer-file-picker" role="dialog" :aria-label="t('chat.composer.filePicker.title')" @click.stop>
    <div class="composer-file-picker-head">
      <button
        v-if="currentPath"
        class="composer-file-picker-up"
        type="button"
        :title="t('chat.composer.filePicker.up')"
        :aria-label="t('chat.composer.filePicker.up')"
        @click="navigate(parentPath)"
      >
        <i class="fa-solid fa-arrow-up" aria-hidden="true"></i>
      </button>
      <span class="composer-file-picker-crumb" :title="breadcrumbLabel">{{ breadcrumbLabel }}</span>
      <button
        class="composer-file-picker-refresh"
        type="button"
        :title="t('messenger.filesArea.retry')"
        :aria-label="t('messenger.filesArea.retry')"
        :disabled="loading"
        @click="load(currentPath, { force: true })"
      >
        <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
      </button>
    </div>

    <div class="composer-file-picker-search">
      <i class="fa-solid fa-magnifying-glass" aria-hidden="true"></i>
      <input
        v-model="keyword"
        class="composer-file-picker-search-input"
        type="text"
        :placeholder="t('chat.composer.filePicker.search')"
        :aria-label="t('chat.composer.filePicker.search')"
        @keydown.esc.prevent="emit('close')"
      />
    </div>

    <div class="composer-file-picker-list" role="listbox">
      <div v-if="loading && !rows.length" class="composer-file-picker-state">
        <span class="composer-model-state-spinner" aria-hidden="true"></span>
        <span>{{ t('common.loading') }}</span>
      </div>
      <div v-else-if="failed" class="composer-file-picker-state is-error">
        <i class="fa-solid fa-triangle-exclamation" aria-hidden="true"></i>
        <span>{{ t('chat.composer.filePicker.failed') }}</span>
      </div>
      <div v-else-if="!rows.length" class="composer-file-picker-state">
        <span>{{ keyword ? t('chat.composer.modelNoMatch') : t('chat.composer.filePicker.empty') }}</span>
      </div>
      <template v-else>
        <div v-for="row in rows" :key="row.path" class="composer-file-picker-row">
          <button
            class="composer-file-picker-entry"
            type="button"
            :title="row.path"
            @click="row.kind === 'dir' ? navigate(row.path) : quote(row)"
          >
            <i
              :class="[
                'fa-solid',
                row.kind === 'dir' ? 'fa-folder' : 'fa-file-lines',
                'composer-file-picker-icon'
              ]"
              aria-hidden="true"
            ></i>
            <span class="composer-file-picker-name">{{ row.name }}</span>
          </button>
          <button
            v-if="row.kind === 'dir'"
            class="composer-file-picker-quote"
            type="button"
            :title="t('messenger.filesArea.menu.quote')"
            :aria-label="t('messenger.filesArea.menu.quote')"
            @click="quote(row)"
          >
            <i class="fa-solid fa-at" aria-hidden="true"></i>
          </button>
        </div>
      </template>
    </div>

    <div v-if="remaining > 0" class="composer-file-picker-foot">
      {{ t('chat.composer.filePicker.truncated', { count: remaining }) }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';

import { useI18n } from '@/i18n';
import { fetchWorkspaceDirectory } from '@/views/messenger/workspace/workspaceFileApi';
import {
  workspaceBaseName,
  workspaceParentPath,
  type WorkspaceEntry
} from '@/views/messenger/workspace/workspaceFileModel';

const emit = defineEmits(['pick', 'close']);

const { t } = useI18n();
// One directory page is plenty for a popover; deeper navigation stays explicit.
const PICKER_PAGE_SIZE = 40;
const PICKER_VISIBLE_ROWS = 40;

const currentPath = ref('');
const entries = ref<WorkspaceEntry[]>([]);
const total = ref(0);
const loading = ref(false);
const failed = ref(false);
const keyword = ref('');
let requestToken = 0;

const parentPath = computed(() => workspaceParentPath(currentPath.value));

const breadcrumbLabel = computed(() => {
  const path = currentPath.value;
  if (!path) return t('chat.composer.filePicker.root');
  return `${t('chat.composer.filePicker.root')} / ${path}`;
});

const rows = computed<WorkspaceEntry[]>(() => {
  const search = keyword.value.trim().toLowerCase();
  const source = search
    ? entries.value.filter((entry) => entry.name.toLowerCase().includes(search))
    : entries.value;
  const dirs = source.filter((entry) => entry.kind === 'dir');
  const files = source.filter((entry) => entry.kind !== 'dir');
  return [...dirs, ...files].slice(0, PICKER_VISIBLE_ROWS);
});

const remaining = computed(() => Math.max(0, Number(total.value) - rows.value.length));

const load = async (path: string, options: { force?: boolean } = {}) => {
  const target = String(path || '');
  if (loading.value && !options.force) return;
  requestToken += 1;
  const token = requestToken;
  loading.value = true;
  failed.value = false;
  try {
    const page = await fetchWorkspaceDirectory(target, { offset: 0, limit: PICKER_PAGE_SIZE });
    if (token !== requestToken) return;
    currentPath.value = page.path;
    entries.value = page.entries;
    total.value = page.total;
    keyword.value = '';
  } catch {
    if (token !== requestToken) return;
    entries.value = [];
    total.value = 0;
    failed.value = true;
  } finally {
    if (token === requestToken) loading.value = false;
  }
};

const navigate = (path: string) => {
  void load(path);
};

const quote = (entry: WorkspaceEntry) => {
  const path = String(entry?.path || '').trim();
  if (!path) return;
  emit('pick', {
    path,
    name: String(entry?.name || '').trim() || workspaceBaseName(path),
    isDir: entry?.kind === 'dir'
  });
};

void load('');
</script>

<style scoped>
.composer-file-picker {
  position: absolute;
  left: 0;
  bottom: calc(100% + 8px);
  z-index: 41;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  width: 296px;
  max-width: calc(100vw - 40px);
  padding: 8px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 12px;
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 12px 32px rgba(31, 35, 41, 0.16);
}

.composer-file-picker-head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.composer-file-picker-up,
.composer-file-picker-refresh {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  cursor: pointer;
}

.composer-file-picker-up:hover,
.composer-file-picker-refresh:hover:not(:disabled) {
  background: var(--mz-hover, #f1efec);
  color: var(--mz-primary, #c96443);
}

.composer-file-picker-refresh:disabled {
  opacity: 0.5;
  cursor: progress;
}

.composer-file-picker-crumb {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  color: var(--mz-text-secondary, #3d3d3d);
  font-size: 11px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composer-file-picker-search {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 6px;
  padding: 0 8px;
  border: 1px solid var(--mz-border, #e8e6e3);
  border-radius: 8px;
  background: var(--mz-bg, #fbfaf8);
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
}

.composer-file-picker-search-input {
  flex: 1 1 auto;
  min-width: 0;
  height: 28px;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--mz-text, #1f2329);
  font: inherit;
  font-size: 12px;
}

.composer-file-picker-list {
  display: flex;
  flex-direction: column;
  gap: 1px;
  max-height: 232px;
  margin-top: 6px;
  overflow-y: auto;
}

.composer-file-picker-row {
  display: flex;
  align-items: center;
  gap: 2px;
}

.composer-file-picker-entry {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1 1 auto;
  min-width: 0;
  padding: 6px 8px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--mz-text, #1f2329);
  font: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}

.composer-file-picker-entry:hover,
.composer-file-picker-quote:hover {
  background: var(--mz-hover, #f1efec);
}

.composer-file-picker-entry:focus-visible,
.composer-file-picker-quote:focus-visible {
  outline: 2px solid var(--mz-primary-soft, #dfac9a);
  outline-offset: -2px;
}

.composer-file-picker-icon {
  flex-shrink: 0;
  width: 14px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  text-align: center;
}

.composer-file-picker-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.composer-file-picker-quote {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 26px;
  height: 26px;
  padding: 0;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
  cursor: pointer;
}

.composer-file-picker-state {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 18px 10px;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 12px;
  text-align: center;
}

.composer-file-picker-state.is-error {
  color: var(--mz-danger, #d04a43);
}

.composer-file-picker-foot {
  margin-top: 6px;
  padding: 4px 4px 0;
  border-top: 1px solid var(--mz-border, #e8e6e3);
  color: var(--mz-text-muted, #8a8f99);
  font-size: 11px;
}
</style>
