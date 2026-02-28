<template>
  <section class="messenger-entity-panel desktop-container-entry" v-loading="loading">
    <div class="desktop-container-entry-head">
      <div>
        <div class="messenger-entity-title">{{ t('desktop.containers.title') }}</div>
        <div class="messenger-entity-meta">{{ t('desktop.containers.subtitle') }}</div>
      </div>
      <el-button type="primary" size="small" @click="openManager(activeContainerId)">
        {{ t('desktop.containers.manage') }}
      </el-button>
    </div>
    <div class="desktop-container-entry-current">
      <span class="desktop-container-entry-badge">
        {{ t('desktop.containers.id') }} #{{ activeContainerId }}
      </span>
      <span class="desktop-container-entry-path" :title="activeContainerRoot || '-'">
        {{ activeContainerRoot || t('messenger.files.localLocationUnknown') }}
      </span>
    </div>
  </section>

  <el-dialog
    v-model="dialogVisible"
    :title="t('desktop.containers.manageTitle', { id: selectedContainerId })"
    width="860px"
    append-to-body
  >
    <div class="desktop-container-dialog" v-loading="loading">
      <aside class="desktop-container-list">
        <div class="desktop-container-list-head">
          <el-button
            type="primary"
            plain
            size="small"
            :disabled="!nextAvailableContainerId"
            @click="addContainer"
          >
            {{ t('desktop.containers.add') }}
          </el-button>
        </div>
        <button
          v-for="row in rows"
          :key="`desktop-container-row-${row.container_id}`"
          class="desktop-container-list-item"
          :class="{ active: selectedContainerId === row.container_id }"
          type="button"
          @click="selectContainer(row.container_id)"
        >
          <span class="desktop-container-list-item-id">{{ t('desktop.containers.id') }} #{{ row.container_id }}</span>
          <span class="desktop-container-list-item-path">{{ row.root || t('common.none') }}</span>
        </button>
      </aside>

      <section v-if="selectedContainer" class="desktop-container-editor">
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
          </div>
          <span v-if="selectedContainer.container_id === USER_CONTAINER_ID" class="desktop-container-hint">
            {{ t('messenger.files.userContainerDesc', { id: USER_CONTAINER_ID }) }}
          </span>
        </label>

        <label class="desktop-container-field">
          <span>{{ t('desktop.seed.cloudWorkspaceId') }}</span>
          <el-input
            v-model="selectedCloudWorkspaceId"
            clearable
            :placeholder="t('desktop.seed.cloudWorkspacePlaceholder')"
          />
        </label>

        <div class="desktop-container-editor-actions">
          <el-button
            v-if="selectedContainer.container_id > 1"
            type="danger"
            plain
            @click="removeContainer(selectedContainer.container_id)"
          >
            {{ t('desktop.common.remove') }}
          </el-button>
        </div>
      </section>
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
  cloud_workspace_id: string;
};

const props = withDefaults(
  defineProps<{
    activeContainerId?: number;
  }>(),
  {
    activeContainerId: USER_CONTAINER_ID
  }
);

const MIN_CONTAINER_ID = USER_CONTAINER_ID;
const MAX_CONTAINER_ID = 10;

const { t } = useI18n();

const loading = ref(false);
const saving = ref(false);
const dialogVisible = ref(false);
const rows = ref<ContainerRow[]>([]);
const workspaceRoot = ref('');
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

const sortRows = () => {
  rows.value.sort((left, right) => left.container_id - right.container_id);
};

const ensureBaseRows = () => {
  const userRoot = workspaceRoot.value.trim();
  const mapped = new Map<number, ContainerRow>();
  rows.value.forEach((item) => {
    mapped.set(normalizeContainerId(item.container_id), {
      container_id: normalizeContainerId(item.container_id),
      root: String(item.root || '').trim(),
      cloud_workspace_id: String(item.cloud_workspace_id || '').trim()
    });
  });

  const userRow = mapped.get(USER_CONTAINER_ID) || {
    container_id: USER_CONTAINER_ID,
    root: '',
    cloud_workspace_id: ''
  };
  if (!userRow.root && userRoot) {
    userRow.root = userRoot;
  }
  mapped.set(USER_CONTAINER_ID, userRow);

  if (!mapped.has(1)) {
    mapped.set(1, { container_id: 1, root: '', cloud_workspace_id: '' });
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
        root: String(item.root || '').trim(),
        cloud_workspace_id: String(item.cloud_workspace_id || '').trim()
      }))
      .filter(
        (item) =>
          Number.isFinite(item.container_id) &&
          item.container_id >= MIN_CONTAINER_ID &&
          item.container_id <= MAX_CONTAINER_ID
      );

  if (Array.isArray(data.container_mounts)) {
    return parseItems(data.container_mounts as DesktopContainerMount[]);
  }
  if (Array.isArray(data.container_roots)) {
    return parseItems(data.container_roots as DesktopContainerRoot[]);
  }
  return [];
};

const loadSettings = async () => {
  loading.value = true;
  try {
    const response = await fetchDesktopSettings();
    const data = (response?.data?.data || {}) as Record<string, unknown>;
    workspaceRoot.value = String(data.workspace_root || '').trim();
    rows.value = parseRowsFromSettings(data);
    ensureBaseRows();
    selectContainer(selectedContainerId.value);
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
    if (selectedContainer.value.container_id === USER_CONTAINER_ID) {
      workspaceRoot.value = selectedContainer.value.root;
    }
  }
});

const selectedCloudWorkspaceId = computed({
  get: () => selectedContainer.value?.cloud_workspace_id || '',
  set: (value: string) => {
    if (!selectedContainer.value) return;
    selectedContainer.value.cloud_workspace_id = String(value || '').trim();
  }
});

const activeContainerId = computed(() => normalizeContainerId(props.activeContainerId));

const activeContainerRoot = computed(() => {
  const target = rows.value.find((item) => item.container_id === activeContainerId.value);
  if (target?.root) {
    return target.root;
  }
  if (activeContainerId.value === USER_CONTAINER_ID) {
    return workspaceRoot.value.trim();
  }
  return '';
});

const nextAvailableContainerId = computed(() => {
  for (let id = 1; id <= MAX_CONTAINER_ID; id += 1) {
    if (!rows.value.some((item) => item.container_id === id)) {
      return id;
    }
  }
  return null;
});

const selectContainer = (containerId: number) => {
  const normalized = normalizeContainerId(containerId);
  let target = rows.value.find((item) => item.container_id === normalized);
  if (!target) {
    target = {
      container_id: normalized,
      root: '',
      cloud_workspace_id: ''
    };
    rows.value.push(target);
    sortRows();
  }
  selectedContainerId.value = normalized;
};

const addContainer = () => {
  const nextId = nextAvailableContainerId.value;
  if (!nextId) return;
  rows.value.push({
    container_id: nextId,
    root: '',
    cloud_workspace_id: ''
  });
  sortRows();
  selectedContainerId.value = nextId;
};

const removeContainer = (containerId: number) => {
  const normalized = normalizeContainerId(containerId);
  if (normalized <= 1) return;
  rows.value = rows.value.filter((item) => item.container_id !== normalized);
  if (selectedContainerId.value === normalized) {
    selectedContainerId.value = USER_CONTAINER_ID;
  }
};

const saveSettings = async () => {
  const workspace = String(
    rows.value.find((item) => item.container_id === USER_CONTAINER_ID)?.root || workspaceRoot.value
  ).trim();
  if (!workspace) {
    ElMessage.warning(t('desktop.containers.workspaceRequired'));
    return;
  }

  const normalizedRows = rows.value
    .map((item) => ({
      container_id: normalizeContainerId(item.container_id),
      root: String(item.root || '').trim(),
      cloud_workspace_id: String(item.cloud_workspace_id || '').trim()
    }))
    .filter((item) => item.container_id >= MIN_CONTAINER_ID && item.container_id <= MAX_CONTAINER_ID)
    .filter(
      (item) =>
        item.container_id === USER_CONTAINER_ID ||
        Boolean(item.root) ||
        Boolean(item.cloud_workspace_id)
    );

  const userRow = normalizedRows.find((item) => item.container_id === USER_CONTAINER_ID);
  if (userRow) {
    userRow.root = workspace;
  } else {
    normalizedRows.push({
      container_id: USER_CONTAINER_ID,
      root: workspace,
      cloud_workspace_id: ''
    });
  }

  const rootRows = normalizedRows.filter((item) => Boolean(item.root));
  saving.value = true;
  try {
    const response = await updateDesktopSettings({
      workspace_root: workspace,
      container_mounts: normalizedRows,
      container_roots: rootRows.map((item) => ({
        container_id: item.container_id,
        root: item.root
      }))
    });
    const data = (response?.data?.data || {}) as Record<string, unknown>;
    workspaceRoot.value = String(data.workspace_root || workspace).trim();
    rows.value = parseRowsFromSettings(data);
    ensureBaseRows();
    selectContainer(selectedContainerId.value);
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
  const initial = selectedRoot.value || workspaceRoot.value || undefined;
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
.desktop-container-entry {
  gap: 12px;
}

.desktop-container-entry-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.desktop-container-entry-current {
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

.desktop-container-dialog {
  display: grid;
  grid-template-columns: 240px 1fr;
  gap: 14px;
  min-height: 360px;
}

.desktop-container-list {
  display: grid;
  grid-template-rows: auto 1fr;
  gap: 10px;
  min-height: 0;
}

.desktop-container-list-head {
  display: flex;
  justify-content: flex-end;
}

.desktop-container-list-item {
  display: grid;
  gap: 4px;
  width: 100%;
  text-align: left;
  border: 1px solid var(--portal-border);
  border-radius: 10px;
  background: var(--portal-surface);
  color: var(--portal-text);
  padding: 8px 10px;
  margin-bottom: 8px;
  cursor: pointer;
  transition: border-color 0.2s ease;
}

.desktop-container-list-item:hover {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
}

.desktop-container-list-item.active {
  border-color: rgba(var(--ui-accent-rgb), 0.65);
  background: var(--ui-accent-soft-2);
}

.desktop-container-list-item-id {
  font-size: 12px;
}

.desktop-container-list-item-path {
  font-size: 11px;
  color: var(--portal-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-container-editor {
  display: grid;
  align-content: start;
  gap: 14px;
}

.desktop-container-field {
  display: grid;
  gap: 8px;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-field-input {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 8px;
}

.desktop-container-hint {
  font-size: 11px;
  line-height: 1.45;
}

.desktop-container-editor-actions {
  display: flex;
  justify-content: flex-start;
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
  background: transparent;
  color: var(--portal-text);
  border-radius: 8px;
  padding: 8px;
  cursor: pointer;
  text-align: left;
}

.desktop-path-picker-item:hover {
  border-color: rgba(var(--ui-accent-rgb), 0.4);
  background: var(--ui-accent-soft-2);
}

.desktop-path-picker-empty {
  padding: 22px 10px;
  text-align: center;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-entry :deep(.el-input__wrapper) {
  background: var(--portal-surface, rgba(255, 255, 255, 0.86));
  box-shadow: 0 0 0 1px var(--portal-border) inset;
  border-radius: 10px;
}

.desktop-container-entry :deep(.el-input__wrapper.is-focus),
.desktop-container-entry :deep(.el-input__wrapper:hover) {
  box-shadow: 0 0 0 1px rgba(var(--ui-accent-rgb), 0.44) inset;
}

@media (max-width: 900px) {
  .desktop-container-dialog {
    grid-template-columns: 1fr;
    min-height: 0;
  }

  .desktop-container-list {
    max-height: 220px;
    overflow: auto;
  }
}
</style>
