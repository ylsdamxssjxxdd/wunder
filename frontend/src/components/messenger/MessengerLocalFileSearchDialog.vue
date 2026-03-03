<template>
  <el-dialog
    :model-value="visible"
    class="messenger-dialog messenger-helper-app-dialog"
    :title="t('userWorld.helperApps.localFileSearch.dialogTitle')"
    width="700px"
    destroy-on-close
    @update:model-value="handleVisibleChange"
  >
    <div class="messenger-helper-app-dialog-body">
      <div class="messenger-helper-app-toolbar">
        <input
          v-model.trim="keyword"
          class="messenger-helper-app-input"
          type="text"
          :placeholder="t('userWorld.helperApps.localFileSearch.placeholder')"
          @keydown.enter.prevent="search"
        />
        <button
          class="messenger-helper-app-btn"
          type="button"
          :disabled="loading"
          @click="search"
        >
          {{ loading ? t('common.loading') : t('userWorld.helperApps.localFileSearch.searchAction') }}
        </button>
      </div>

      <div class="messenger-helper-app-options">
        <label class="messenger-helper-app-option">
          <input v-model="includeFiles" type="checkbox" />
          <span>{{ t('userWorld.helperApps.localFileSearch.includeFiles') }}</span>
        </label>
        <label class="messenger-helper-app-option">
          <input v-model="includeDirs" type="checkbox" />
          <span>{{ t('userWorld.helperApps.localFileSearch.includeDirs') }}</span>
        </label>
      </div>

      <div class="messenger-helper-app-summary">
        <span>{{ t('userWorld.helperApps.localFileSearch.searchScope') }}</span>
        <span v-if="touched">
          {{ t('userWorld.helperApps.localFileSearch.searchResult', { count: total }) }}
        </span>
      </div>

      <div class="messenger-helper-app-results">
        <div v-if="touched && !loading && !results.length" class="messenger-helper-app-empty">
          {{ t('userWorld.helperApps.localFileSearch.empty') }}
        </div>
        <div v-else-if="!touched" class="messenger-helper-app-empty">
          {{ t('userWorld.helperApps.localFileSearch.guide') }}
        </div>
        <div
          v-for="entry in results"
          :key="`${entry.container_id}:${entry.entry_type}:${entry.path}`"
          class="messenger-helper-app-item"
        >
          <div class="messenger-helper-app-item-main">
            <span class="messenger-helper-app-container">{{ formatContainerLabel(entry.container_id) }}</span>
            <i
              :class="entry.entry_type === 'dir' ? 'fa-solid fa-folder' : 'fa-solid fa-file-lines'"
              aria-hidden="true"
            ></i>
            <span class="messenger-helper-app-item-path" :title="entry.path">{{ entry.path }}</span>
            <span class="messenger-helper-app-item-meta">{{ formatMeta(entry) }}</span>
          </div>
        </div>
      </div>

      <button
        v-if="hasMore"
        class="messenger-helper-app-more"
        type="button"
        :disabled="loading"
        @click="loadMore"
      >
        {{ loading ? t('common.loading') : t('userWorld.helperApps.localFileSearch.loadMore') }}
      </button>
    </div>
    <template #footer>
      <span class="dialog-footer">
        <button class="user-world-dialog-btn muted" type="button" @click="handleVisibleChange(false)">
          {{ t('common.close') }}
        </button>
      </span>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { searchWunderWorkspace } from '@/api/workspace';
import { useI18n } from '@/i18n';
import { normalizeWorkspacePath } from '@/utils/workspaceTreeCache';
import { AGENT_CONTAINER_IDS, USER_CONTAINER_ID } from '@/views/messenger/model';

type LocalFileSearchEntry = {
  name: string;
  path: string;
  entry_type: 'file' | 'dir';
  size: number;
  updated_time: string;
  container_id: number;
};

const SEARCH_CONTAINER_IDS = [USER_CONTAINER_ID, ...AGENT_CONTAINER_IDS];
const SEARCH_LIMIT = 50;

const props = defineProps<{
  visible: boolean;
}>();

const emit = defineEmits<{
  'update:visible': [value: boolean];
}>();

const { t } = useI18n();

const keyword = ref('');
const loading = ref(false);
const touched = ref(false);
const includeFiles = ref(true);
const includeDirs = ref(true);
const total = ref(0);
const offset = ref(0);
const results = ref<LocalFileSearchEntry[]>([]);

const hasMore = computed(() => results.value.length < total.value);

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeSearchEntry = (entry: unknown, containerId: number): LocalFileSearchEntry | null => {
  if (!entry || typeof entry !== 'object') return null;
  const source = entry as Record<string, unknown>;
  const path = normalizeWorkspacePath(String(source.path || '').trim());
  if (!path) return null;
  const name = normalizeText(source.name) || path.split('/').pop() || path;
  const typeText = String(source.type || source.entry_type || '').trim().toLowerCase();
  return {
    name,
    path,
    entry_type: typeText === 'dir' ? 'dir' : 'file',
    size: Number(source.size || 0),
    updated_time: String(source.updated_time || ''),
    container_id: containerId
  };
};

const searchInternal = async (append: boolean) => {
  const keywordValue = normalizeText(keyword.value);
  if (!keywordValue) {
    ElMessage.warning(t('userWorld.helperApps.localFileSearch.keywordRequired'));
    return;
  }
  if (!includeFiles.value && !includeDirs.value) {
    ElMessage.warning(t('userWorld.helperApps.localFileSearch.typeRequired'));
    return;
  }
  loading.value = true;
  touched.value = true;
  const currentOffset = append ? offset.value : 0;
  const requestLimit = Math.max(SEARCH_LIMIT, currentOffset + SEARCH_LIMIT);
  try {
    const settled = await Promise.allSettled(
      SEARCH_CONTAINER_IDS.map(async (containerId) => {
        const { data } = await searchWunderWorkspace({
          keyword: keywordValue,
          container_id: containerId,
          offset: 0,
          limit: requestLimit,
          include_files: includeFiles.value,
          include_dirs: includeDirs.value
        });
        const entries = Array.isArray(data?.entries) ? data.entries : [];
        const normalizedEntries = entries
          .map((item) => normalizeSearchEntry(item, containerId))
          .filter((item): item is LocalFileSearchEntry => Boolean(item));
        return {
          total: Number(data?.total || 0),
          entries: normalizedEntries
        };
      })
    );
    let merged: LocalFileSearchEntry[] = [];
    let mergedTotal = 0;
    let failureCount = 0;
    settled.forEach((item) => {
      if (item.status !== 'fulfilled') {
        failureCount += 1;
        return;
      }
      mergedTotal += item.value.total;
      merged = merged.concat(item.value.entries);
    });
    if (failureCount > 0) {
      ElMessage.warning(t('userWorld.helperApps.localFileSearch.partialFailed', { count: failureCount }));
    }
    const dedup = new Map<string, LocalFileSearchEntry>();
    merged.forEach((entry) => {
      const key = `${entry.container_id}:${entry.entry_type}:${entry.path}`;
      if (!dedup.has(key)) {
        dedup.set(key, entry);
      }
    });
    const ordered = [...dedup.values()].sort((left, right) => {
      if (left.container_id !== right.container_id) {
        return left.container_id - right.container_id;
      }
      return left.path.localeCompare(right.path, 'zh-CN');
    });
    const page = ordered.slice(currentOffset, currentOffset + SEARCH_LIMIT);
    results.value = append ? [...results.value, ...page] : page;
    total.value = mergedTotal;
    offset.value = results.value.length;
  } finally {
    loading.value = false;
  }
};

const search = async () => {
  await searchInternal(false);
};

const loadMore = async () => {
  if (!hasMore.value || loading.value) return;
  await searchInternal(true);
};

const formatMeta = (entry: LocalFileSearchEntry): string => {
  const kind =
    entry.entry_type === 'dir'
      ? t('userWorld.helperApps.localFileSearch.dir')
      : t('userWorld.helperApps.localFileSearch.file');
  const updated = normalizeText(entry.updated_time);
  if (!updated) return kind;
  const date = new Date(updated);
  if (Number.isNaN(date.getTime())) return kind;
  return `${kind} · ${date.toLocaleString()}`;
};

const formatContainerLabel = (containerId: number): string =>
  containerId === USER_CONTAINER_ID
    ? t('userWorld.helperApps.localFileSearch.userContainerTag')
    : `C${containerId}`;

const handleVisibleChange = (value: boolean) => {
  emit('update:visible', Boolean(value));
};
</script>

<style scoped>
.messenger-helper-app-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.messenger-helper-app-toolbar {
  display: flex;
  gap: 8px;
}

.messenger-helper-app-input {
  flex: 1;
  height: 38px;
  border: 1px solid var(--hula-border, rgba(148, 163, 184, 0.28));
  border-radius: 10px;
  padding: 0 10px;
  background: var(--hula-card, #fff);
  color: var(--hula-text, #0f172a);
}

.messenger-helper-app-btn,
.messenger-helper-app-more {
  height: 38px;
  min-width: 96px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.35);
  border-radius: 10px;
  background: rgba(var(--ui-accent-rgb), 0.14);
  color: var(--hula-text, #0f172a);
  cursor: pointer;
}

.messenger-helper-app-btn:disabled,
.messenger-helper-app-more:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.messenger-helper-app-options {
  display: flex;
  align-items: center;
  gap: 12px;
}

.messenger-helper-app-option {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--hula-muted, #64748b);
}

.messenger-helper-app-summary {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  font-size: 12px;
  color: var(--hula-muted, #64748b);
}

.messenger-helper-app-results {
  max-height: 360px;
  overflow-y: auto;
  border: 1px solid var(--hula-border, rgba(148, 163, 184, 0.28));
  border-radius: 10px;
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.messenger-helper-app-empty {
  text-align: center;
  color: var(--hula-muted, #64748b);
  padding: 12px 6px;
}

.messenger-helper-app-item {
  border: 1px solid rgba(var(--ui-accent-rgb), 0.18);
  border-radius: 8px;
  padding: 6px 8px;
  background: rgba(var(--ui-accent-rgb), 0.06);
}

.messenger-helper-app-item-main {
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
}

.messenger-helper-app-container {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 30px;
  height: 18px;
  padding: 0 6px;
  border-radius: 999px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.5);
  background: rgba(var(--ui-accent-rgb), 0.2);
  color: var(--hula-muted, #64748b);
  font-size: 10px;
  font-weight: 600;
  flex-shrink: 0;
}

.messenger-helper-app-item-path,
.messenger-helper-app-item-meta {
  margin-top: 0;
  font-size: 11px;
  color: var(--hula-muted, #64748b);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.messenger-helper-app-item-path {
  flex: 1;
  min-width: 0;
}

.messenger-helper-app-item-meta {
  margin-left: auto;
  flex-shrink: 0;
}
</style>
