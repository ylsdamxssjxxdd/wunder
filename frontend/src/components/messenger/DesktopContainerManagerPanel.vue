<template>
  <el-dialog
    v-model="dialogVisible"
    :title="t('desktop.containers.manageTitle', { id: selectedContainerId })"
    width="720px"
    append-to-body
  >
    <div class="desktop-container-editor" v-loading="loading">
      <div class="desktop-container-current">
        <span class="desktop-container-entry-badge">
          {{ t('desktop.containers.id') }} #{{ selectedContainerId }}
        </span>
        <span class="desktop-container-entry-path" :title="selectedEffectiveRoot || '-'">
          {{ selectedEffectiveRoot || t('messenger.files.localLocationUnknown') }}
        </span>
      </div>

      <label class="desktop-container-field">
        <span>{{ t('desktop.containers.path') }}</span>
        <div class="desktop-container-field-input">
          <el-input
            v-model="selectedRoot"
            clearable
            :placeholder="t('desktop.containers.pathPlaceholder')"
          />
          <el-button type="primary" plain @click="openPathPicker">
            {{ t('desktop.common.browse') }}
          </el-button>
          <el-button plain @click="resetSelectedRoot">
            {{ t('common.reset') }}
          </el-button>
        </div>
        <span class="desktop-container-hint">
          {{ t('desktop.containers.pathHint') }}
        </span>
      </label>
    </div>

    <template #footer>
      <div class="desktop-container-footer">
        <el-button @click="dialogVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" :loading="saving" @click="saveSettings">
          {{ t('desktop.common.save') }}
        </el-button>
      </div>
    </template>
  </el-dialog>

  <el-dialog
    v-model="pathPickerVisible"
    :title="t('desktop.containers.pathPickerTitle')"
    width="720px"
    append-to-body
  >
    <div class="desktop-path-picker">
      <div class="desktop-path-picker-toolbar">
        <el-button
          size="small"
          :disabled="!pathPickerParentPath"
          @click="loadDirectory(pathPickerParentPath || undefined)"
        >
          {{ t('desktop.containers.pathPickerUp') }}
        </el-button>
        <el-button size="small" type="primary" plain @click="useCurrentDirectory">
          {{ t('desktop.containers.pathPickerUseCurrent') }}
        </el-button>
      </div>
      <div class="desktop-path-picker-current" :title="pathPickerCurrentPath">{{ pathPickerCurrentPath }}</div>
      <div class="desktop-path-picker-roots">
        <button
          v-for="root in pathPickerRoots"
          :key="`desktop-path-root-${root}`"
          class="desktop-path-picker-root"
          type="button"
          @click="loadDirectory(root)"
        >
          {{ root }}
        </button>
      </div>
      <div class="desktop-path-picker-list" v-loading="pathPickerLoading">
        <button
          v-for="item in pathPickerItems"
          :key="`desktop-path-item-${item.path}`"
          class="desktop-path-picker-item"
          type="button"
          @click="loadDirectory(item.path)"
        >
          <i class="fa-regular fa-folder" aria-hidden="true"></i>
          <span>{{ item.name }}</span>
        </button>
        <div v-if="!pathPickerLoading && !pathPickerItems.length" class="desktop-path-picker-empty">
          {{ t('desktop.containers.pathPickerEmpty') }}
        </div>
      </div>
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { ElMessage } from 'element-plus';

import {
  fetchDesktopSettings,
  listDesktopDirectories,
  updateDesktopSettings,
  type DesktopContainerMount,
  type DesktopContainerRoot,
  type DesktopDirectoryEntry
} from '@/api/desktop';
import { useI18n } from '@/i18n';
import { USER_CONTAINER_ID } from '@/views/messenger/model';

type ContainerRow = {
  container_id: number;
  root: string;
};

const props = withDefaults(
  defineProps<{
    activeContainerId?: number;
  }>(),
  {
    activeContainerId: USER_CONTAINER_ID
  }
);

const emit = defineEmits<{
  (event: 'roots-change', roots: Record<number, string>): void;
}>();

const MIN_CONTAINER_ID = USER_CONTAINER_ID;
const MAX_CONTAINER_ID = 10;

const { t } = useI18n();

const loading = ref(false);
const saving = ref(false);
const dialogVisible = ref(false);
const rows = ref<ContainerRow[]>([]);
const selectedContainerId = ref(USER_CONTAINER_ID);

const pathPickerVisible = ref(false);
const pathPickerLoading = ref(false);
const pathPickerCurrentPath = ref('');
const pathPickerParentPath = ref<string | null>(null);
const pathPickerRoots = ref<string[]>([]);
const pathPickerItems = ref<DesktopDirectoryEntry[]>([]);

const normalizeContainerId = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? USER_CONTAINER_ID), 10);
  if (!Number.isFinite(parsed)) return USER_CONTAINER_ID;
  return Math.min(MAX_CONTAINER_ID, Math.max(MIN_CONTAINER_ID, parsed));
};

const normalizePathForCompare = (value: string): string => {
  let normalized = String(value || '').trim().replace(/\\/g, '/').replace(/\/+$/, '');
  if (normalized === '/') return normalized;
  if (/^[A-Za-z]:$/.test(normalized)) normalized += '/';
  normalized = normalized.replace(/\/{2,}/g, '/');
  if (typeof window !== 'undefined' && navigator.userAgent.toLowerCase().includes('windows')) {
    normalized = normalized.toLowerCase();
  }
  return normalized;
};

const sortRows = () => {
  rows.value.sort((left, right) => left.container_id - right.container_id);
};

const ensureAllRows = () => {
  const mapped = new Map<number, ContainerRow>();
  rows.value.forEach((item) => {
    const containerId = normalizeContainerId(item.container_id);
    mapped.set(containerId, {
      container_id: containerId,
      root: String(item.root || '').trim()
    });
  });
  for (let id = MIN_CONTAINER_ID; id <= MAX_CONTAINER_ID; id += 1) {
    if (!mapped.has(id)) {
      mapped.set(id, { container_id: id, root: '' });
    }
  }
  rows.value = Array.from(mapped.values());
  sortRows();
};

const parseRowsFromSettings = (data: Record<string, unknown>): ContainerRow[] => {
  const parseItems = (items: unknown[]): ContainerRow[] =>
    items
      .map((item) => item as Record<string, unknown>)
      .map((item) => ({
        container_id: normalizeContainerId(item.container_id),
        root: String(item.root || '').trim()
      }))
      .filter(
        (item) =>
          Number.isFinite(item.container_id) &&
          item.container_id >= MIN_CONTAINER_ID &&
          item.container_id <= MAX_CONTAINER_ID
      );

  if (Array.isArray(data.container_roots)) {
    return parseItems(data.container_roots as DesktopContainerRoot[]);
  }
  if (Array.isArray(data.container_mounts)) {
    return parseItems(data.container_mounts as DesktopContainerMount[]);
  }
  return [];
};

const emitRootsChange = () => {
  const payload: Record<number, string> = {};
  rows.value.forEach((row) => {
    payload[row.container_id] = String(row.root || '').trim();
  });
  emit('roots-change', payload);
};

const loadSettings = async () => {
  loading.value = true;
  try {
    const response = await fetchDesktopSettings();
    const data = (response?.data?.data || {}) as Record<string, unknown>;
    rows.value = parseRowsFromSettings(data);
    ensureAllRows();
    selectContainer(selectedContainerId.value);
    emitRootsChange();
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const selectedContainer = computed(
  () => rows.value.find((item) => item.container_id === selectedContainerId.value) || null
);

const selectedRoot = computed({
  get: () => selectedContainer.value?.root || '',
  set: (value: string) => {
    if (!selectedContainer.value) return;
    selectedContainer.value.root = String(value || '').trim();
  }
});

const selectedEffectiveRoot = computed(() => selectedContainer.value?.root || '');

const activeContainerId = computed(() => normalizeContainerId(props.activeContainerId));

const selectContainer = (containerId: number) => {
  const normalized = normalizeContainerId(containerId);
  let target = rows.value.find((item) => item.container_id === normalized);
  if (!target) {
    target = { container_id: normalized, root: '' };
    rows.value.push(target);
    sortRows();
  }
  selectedContainerId.value = normalized;
};

const resetSelectedRoot = () => {
  selectedRoot.value = '';
};

const saveSettings = async () => {
  const normalizedRows = rows.value
    .map((item) => ({
      container_id: normalizeContainerId(item.container_id),
      root: String(item.root || '').trim()
    }))
    .filter((item) => item.container_id >= MIN_CONTAINER_ID && item.container_id <= MAX_CONTAINER_ID);

  const seen = new Map<string, number>();
  for (const row of normalizedRows) {
    if (!row.root) continue;
    const key = normalizePathForCompare(row.root);
    if (!key) continue;
    if (seen.has(key)) {
      ElMessage.warning(
        t('desktop.containers.pathDuplicate', {
          left: seen.get(key),
          right: row.container_id
        })
      );
      return;
    }
    seen.set(key, row.container_id);
  }

  const rootRows = normalizedRows
    .filter((item) => Boolean(item.root))
    .map((item) => ({
      container_id: item.container_id,
      root: item.root
    }));

  saving.value = true;
  try {
    const response = await updateDesktopSettings({
      container_roots: rootRows
    });
    const data = (response?.data?.data || {}) as Record<string, unknown>;
    rows.value = parseRowsFromSettings(data);
    ensureAllRows();
    selectContainer(selectedContainerId.value);
    emitRootsChange();
    ElMessage.success(t('desktop.common.saveSuccess'));
    dialogVisible.value = false;
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    saving.value = false;
  }
};

const loadDirectory = async (path?: string) => {
  pathPickerLoading.value = true;
  try {
    const response = await listDesktopDirectories(path);
    const data = (response?.data?.data || {}) as Record<string, unknown>;
    pathPickerCurrentPath.value = String(data.current_path || '').trim();
    pathPickerParentPath.value = data.parent_path ? String(data.parent_path) : null;
    pathPickerRoots.value = Array.isArray(data.roots)
      ? data.roots.map((item) => String(item || '').trim()).filter(Boolean)
      : [];
    pathPickerItems.value = Array.isArray(data.items)
      ? (data.items as unknown[])
          .map((item) => item as Record<string, unknown>)
          .map((item) => ({
            name: String(item.name || '').trim(),
            path: String(item.path || '').trim()
          }))
          .filter((item) => item.name && item.path)
      : [];
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.containers.pathPickerLoadFailed'));
  } finally {
    pathPickerLoading.value = false;
  }
};

const openPathPicker = async () => {
  pathPickerVisible.value = true;
  const initial = selectedRoot.value || undefined;
  await loadDirectory(initial);
};

const useCurrentDirectory = () => {
  if (!pathPickerCurrentPath.value) return;
  selectedRoot.value = pathPickerCurrentPath.value;
  pathPickerVisible.value = false;
};

const openManager = async (containerId?: number) => {
  await loadSettings();
  selectContainer(containerId ?? activeContainerId.value);
  dialogVisible.value = true;
};

defineExpose({
  openManager,
  refreshSettings: loadSettings
});

onMounted(() => {
  void loadSettings();
});
</script>

<style scoped>
.desktop-container-editor {
  display: grid;
  gap: 12px;
}

.desktop-container-current {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.desktop-container-entry-badge {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--portal-text);
  border: 1px solid var(--portal-border);
  background: var(--portal-surface);
  border-radius: 999px;
  padding: 2px 10px;
}

.desktop-container-entry-path {
  min-width: 0;
  font-size: 12px;
  color: var(--portal-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-container-field {
  display: grid;
  gap: 8px;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-field-input {
  display: grid;
  grid-template-columns: 1fr auto auto;
  gap: 8px;
}

.desktop-container-hint {
  font-size: 11px;
  line-height: 1.45;
}

.desktop-container-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.desktop-path-picker {
  display: grid;
  gap: 10px;
}

.desktop-path-picker-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
}

.desktop-path-picker-current {
  font-size: 12px;
  color: var(--portal-muted);
  border: 1px solid var(--portal-border);
  background: var(--portal-surface);
  border-radius: 8px;
  padding: 6px 10px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-path-picker-roots {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.desktop-path-picker-root {
  border: 1px solid var(--portal-border);
  background: var(--portal-surface);
  color: var(--portal-text);
  border-radius: 8px;
  padding: 4px 10px;
  font-size: 12px;
  cursor: pointer;
}

.desktop-path-picker-list {
  border: 1px solid var(--portal-border);
  background: var(--portal-surface);
  border-radius: 10px;
  max-height: 320px;
  overflow: auto;
  padding: 8px;
  display: grid;
  gap: 6px;
}

.desktop-path-picker-item {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--portal-text);
  font-size: 12px;
  padding: 8px 10px;
  cursor: pointer;
  text-align: left;
}

.desktop-path-picker-item:hover {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
  background: var(--ui-accent-soft-2);
}

.desktop-path-picker-empty {
  font-size: 12px;
  color: var(--portal-muted);
  padding: 12px 4px;
}

@media (max-width: 820px) {
  .desktop-container-field-input {
    grid-template-columns: 1fr;
  }
}
</style>
