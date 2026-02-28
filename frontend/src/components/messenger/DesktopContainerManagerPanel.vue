<template>
  <section class="messenger-entity-panel desktop-container-manager-panel" v-loading="loading">
    <div class="messenger-entity-title">{{ t('desktop.containers.title') }}</div>
    <div class="messenger-entity-meta">{{ t('desktop.containers.subtitle') }}</div>

    <label class="desktop-container-manager-field">
      <span>{{ t('desktop.containers.defaultWorkspace') }}</span>
      <el-input v-model="workspaceRoot" :placeholder="t('desktop.containers.pathPlaceholder')" />
      <span class="desktop-container-manager-hint">{{ t('desktop.containers.defaultHint') }}</span>
    </label>

    <div class="desktop-container-manager-toolbar">
      <el-button type="primary" plain size="small" @click="addContainer">
        {{ t('desktop.containers.add') }}
      </el-button>
      <el-button type="primary" size="small" :loading="saving" @click="saveSettings">
        {{ t('desktop.common.save') }}
      </el-button>
    </div>

    <div class="desktop-container-manager-list">
      <article
        v-for="row in rows"
        :key="`desktop-container-${row.container_id}`"
        class="desktop-container-manager-item"
      >
        <div class="desktop-container-manager-item-head">
          <span>{{ t('desktop.containers.id') }} #{{ row.container_id }}</span>
          <el-button
            v-if="row.container_id !== 1"
            link
            type="danger"
            size="small"
            @click="removeContainer(row.container_id)"
          >
            {{ t('desktop.common.remove') }}
          </el-button>
          <span v-else class="desktop-container-manager-fixed">{{ t('desktop.containers.fixed') }}</span>
        </div>
        <label class="desktop-container-manager-field">
          <span>{{ t('desktop.containers.path') }}</span>
          <el-input v-model="row.root" :placeholder="t('desktop.containers.pathPlaceholder')" />
        </label>
        <label class="desktop-container-manager-field">
          <span>{{ t('desktop.seed.cloudWorkspaceId') }}</span>
          <el-input
            v-model="row.cloud_workspace_id"
            :placeholder="t('desktop.seed.cloudWorkspacePlaceholder')"
          />
        </label>
      </article>
    </div>
  </section>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { ElMessage } from 'element-plus';

import {
  fetchDesktopSettings,
  updateDesktopSettings,
  type DesktopContainerMount,
  type DesktopContainerRoot
} from '@/api/desktop';
import { useI18n } from '@/i18n';

type ContainerRow = {
  container_id: number;
  root: string;
  cloud_workspace_id: string;
};

const { t } = useI18n();

const loading = ref(false);
const saving = ref(false);
const workspaceRoot = ref('');
const rows = ref<ContainerRow[]>([]);

const sortRows = () => {
  rows.value.sort((left, right) => left.container_id - right.container_id);
};

const ensureDefaultContainer = () => {
  const first = rows.value.find((item) => item.container_id === 1);
  if (!first) {
    rows.value.unshift({
      container_id: 1,
      root: workspaceRoot.value.trim(),
      cloud_workspace_id: ''
    });
  } else if (workspaceRoot.value.trim()) {
    first.root = workspaceRoot.value.trim();
  }
  sortRows();
};

const parseRowsFromSettings = (data: Record<string, any>): ContainerRow[] => {
  const mountRows = Array.isArray(data.container_mounts)
    ? (data.container_mounts as DesktopContainerMount[])
        .map((item) => ({
          container_id: Number.parseInt(String(item.container_id), 10),
          root: String(item.root || '').trim(),
          cloud_workspace_id: String(item.cloud_workspace_id || '').trim()
        }))
        .filter((item) => Number.isFinite(item.container_id) && item.container_id > 0)
    : [];

  if (mountRows.length) {
    return mountRows;
  }

  return Array.isArray(data.container_roots)
    ? (data.container_roots as DesktopContainerRoot[])
        .map((item) => ({
          container_id: Number.parseInt(String(item.container_id), 10),
          root: String(item.root || '').trim(),
          cloud_workspace_id: ''
        }))
        .filter((item) => Number.isFinite(item.container_id) && item.container_id > 0)
    : [];
};

const loadSettings = async () => {
  loading.value = true;
  try {
    const response = await fetchDesktopSettings();
    const data = (response?.data?.data || {}) as Record<string, any>;
    workspaceRoot.value = String(data.workspace_root || '').trim();
    rows.value = parseRowsFromSettings(data);
    ensureDefaultContainer();
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const addContainer = () => {
  const maxId = rows.value.reduce((max, item) => Math.max(max, item.container_id), 1);
  rows.value.push({
    container_id: maxId + 1,
    root: '',
    cloud_workspace_id: ''
  });
  sortRows();
};

const removeContainer = (containerId: number) => {
  rows.value = rows.value.filter((item) => item.container_id !== containerId);
};

const saveSettings = async () => {
  const workspace = workspaceRoot.value.trim();
  if (!workspace) {
    ElMessage.warning(t('desktop.containers.workspaceRequired'));
    return;
  }

  const normalized = rows.value
    .map((item) => ({
      container_id: Number.parseInt(String(item.container_id), 10),
      root: String(item.root || '').trim(),
      cloud_workspace_id: String(item.cloud_workspace_id || '').trim()
    }))
    .filter((item) => Number.isFinite(item.container_id) && item.container_id > 0);

  const defaultContainer = normalized.find((item) => item.container_id === 1);
  if (defaultContainer) {
    defaultContainer.root = workspace;
  } else {
    normalized.unshift({ container_id: 1, root: workspace, cloud_workspace_id: '' });
  }

  for (const item of normalized) {
    if (!item.root) {
      ElMessage.warning(t('desktop.containers.pathRequired', { id: item.container_id }));
      return;
    }
  }

  saving.value = true;
  try {
    const response = await updateDesktopSettings({
      workspace_root: workspace,
      container_mounts: normalized,
      container_roots: normalized.map((item) => ({
        container_id: item.container_id,
        root: item.root
      }))
    });
    const data = (response?.data?.data || {}) as Record<string, any>;
    workspaceRoot.value = String(data.workspace_root || workspace).trim();
    rows.value = parseRowsFromSettings({
      container_mounts: data.container_mounts,
      container_roots: data.container_roots
    });
    ensureDefaultContainer();
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    saving.value = false;
  }
};

onMounted(() => {
  void loadSettings();
});
</script>

<style scoped>
.desktop-container-manager-panel {
  gap: 10px;
}

.desktop-container-manager-field {
  display: grid;
  gap: 6px;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-manager-hint {
  font-size: 12px;
  color: var(--portal-muted);
  line-height: 1.45;
}

.desktop-container-manager-toolbar {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.desktop-container-manager-list {
  display: grid;
  gap: 10px;
}

.desktop-container-manager-item {
  border: 1px solid var(--portal-border);
  border-radius: 10px;
  background: var(--portal-panel);
  padding: 10px;
  display: grid;
  gap: 8px;
}

.desktop-container-manager-item-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-manager-fixed {
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-manager-panel :deep(.el-input__wrapper) {
  background: var(--portal-surface, rgba(255, 255, 255, 0.86));
  box-shadow: 0 0 0 1px var(--portal-border) inset;
}
</style>
