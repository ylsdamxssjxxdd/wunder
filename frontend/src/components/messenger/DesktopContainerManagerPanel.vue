<template>
  <section class="messenger-entity-panel desktop-container-manager-panel" v-loading="loading">
    <div class="desktop-container-manager-head">
      <div>
        <div class="messenger-entity-title">{{ t('desktop.containers.title') }}</div>
        <div class="messenger-entity-meta">{{ t('desktop.containers.subtitle') }}</div>
      </div>
      <div class="desktop-container-manager-toolbar">
        <el-button type="primary" plain size="small" @click="addContainer">
          {{ t('desktop.containers.add') }}
        </el-button>
        <el-button type="primary" size="small" :loading="saving" @click="saveSettings">
          {{ t('desktop.common.save') }}
        </el-button>
      </div>
    </div>

    <div class="desktop-container-manager-list">
      <article
        v-for="row in rows"
        :key="`desktop-container-${row.container_id}`"
        class="desktop-container-manager-item"
      >
        <div class="desktop-container-manager-item-main">
          <div class="desktop-container-manager-item-head">
            <span>{{ t('desktop.containers.id') }} #{{ row.container_id }}</span>
            <span v-if="row.container_id === 1" class="desktop-container-manager-fixed">
              {{ t('desktop.containers.fixed') }}
            </span>
          </div>
          <div class="desktop-container-manager-item-locations">
            <div class="desktop-container-manager-item-location">
              <span class="desktop-container-manager-item-label">{{ t('messenger.files.localLocation') }}</span>
              <span class="desktop-container-manager-item-value" :title="row.root || '-'">{{ row.root || '-' }}</span>
            </div>
            <div class="desktop-container-manager-item-location">
              <span class="desktop-container-manager-item-label">{{ t('messenger.files.cloudLocation') }}</span>
              <span class="desktop-container-manager-item-value" :title="row.cloud_workspace_id || '-'">
                {{ row.cloud_workspace_id || '-' }}
              </span>
            </div>
          </div>
        </div>
        <div class="desktop-container-manager-item-actions">
          <el-button size="small" @click="openContainerEditor(row.container_id)">
            {{ t('desktop.containers.manage') }}
          </el-button>
          <el-button
            v-if="row.container_id !== 1"
            link
            type="danger"
            size="small"
            @click="removeContainer(row.container_id)"
          >
            {{ t('desktop.common.remove') }}
          </el-button>
        </div>
      </article>
    </div>
  </section>

  <el-dialog
    v-model="containerEditorVisible"
    :title="t('desktop.containers.manageTitle', { id: editorForm.container_id || '-' })"
    width="560px"
    append-to-body
  >
    <div class="desktop-container-editor">
      <label class="desktop-container-manager-field">
        <span>{{ editorForm.container_id === 1 ? t('desktop.containers.defaultWorkspace') : t('desktop.containers.path') }}</span>
        <el-input v-model="editorForm.root" :placeholder="t('desktop.containers.pathPlaceholder')" />
        <span v-if="editorForm.container_id === 1" class="desktop-container-manager-hint">
          {{ t('desktop.containers.defaultHint') }}
        </span>
      </label>
      <label class="desktop-container-manager-field">
        <span>{{ t('desktop.seed.cloudWorkspaceId') }}</span>
        <el-input
          v-model="editorForm.cloud_workspace_id"
          :placeholder="t('desktop.seed.cloudWorkspacePlaceholder')"
        />
      </label>
    </div>
    <template #footer>
      <div class="desktop-container-editor-footer">
        <el-button
          v-if="editorForm.container_id !== 1"
          type="danger"
          plain
          @click="removeContainer(editorForm.container_id, true)"
        >
          {{ t('desktop.common.remove') }}
        </el-button>
        <span class="desktop-container-editor-footer-spacer"></span>
        <el-button @click="containerEditorVisible = false">{{ t('common.cancel') }}</el-button>
        <el-button type="primary" @click="applyContainerEditor">{{ t('desktop.common.save') }}</el-button>
      </div>
    </template>
  </el-dialog>
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
const containerEditorVisible = ref(false);
const editorForm = ref<ContainerRow>({
  container_id: 1,
  root: '',
  cloud_workspace_id: ''
});

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
  editorForm.value = {
    container_id: maxId + 1,
    root: '',
    cloud_workspace_id: ''
  };
  containerEditorVisible.value = true;
};

const openContainerEditor = (containerId: number) => {
  const target = rows.value.find((item) => item.container_id === containerId);
  if (!target) return;
  editorForm.value = {
    container_id: target.container_id,
    root: String(target.root || '').trim(),
    cloud_workspace_id: String(target.cloud_workspace_id || '').trim()
  };
  containerEditorVisible.value = true;
};

const applyContainerEditor = () => {
  const containerId = Number.parseInt(String(editorForm.value.container_id), 10);
  if (!Number.isFinite(containerId) || containerId <= 0) return;
  const root = String(editorForm.value.root || '').trim();
  const cloudWorkspaceId = String(editorForm.value.cloud_workspace_id || '').trim();
  if (!root) {
    ElMessage.warning(t('desktop.containers.pathRequired', { id: containerId }));
    return;
  }
  const target = rows.value.find((item) => item.container_id === containerId);
  if (!target) {
    rows.value.push({
      container_id: containerId,
      root,
      cloud_workspace_id: cloudWorkspaceId
    });
  } else {
    target.root = root;
    target.cloud_workspace_id = cloudWorkspaceId;
  }
  if (containerId === 1) {
    workspaceRoot.value = root;
  }
  sortRows();
  containerEditorVisible.value = false;
};

const removeContainer = (containerId: number, closeEditor = false) => {
  if (containerId === 1) return;
  rows.value = rows.value.filter((item) => item.container_id !== containerId);
  if (closeEditor) {
    containerEditorVisible.value = false;
  }
};

const saveSettings = async () => {
  const defaultContainer = rows.value.find((item) => item.container_id === 1);
  const workspace = String(defaultContainer?.root || workspaceRoot.value || '').trim();
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

  const normalizedDefault = normalized.find((item) => item.container_id === 1);
  if (normalizedDefault) {
    normalizedDefault.root = workspace;
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
  gap: 12px;
}

.desktop-container-manager-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
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
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.desktop-container-manager-list {
  display: grid;
  gap: 8px;
}

.desktop-container-manager-item {
  border: 1px solid var(--portal-border);
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.desktop-container-manager-item-main {
  flex: 1;
  min-width: 0;
  display: grid;
  gap: 6px;
}

.desktop-container-manager-item-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--portal-text);
}

.desktop-container-manager-item-locations {
  display: grid;
  gap: 4px;
}

.desktop-container-manager-item-location {
  display: flex;
  align-items: baseline;
  gap: 6px;
  min-width: 0;
}

.desktop-container-manager-item-label {
  font-size: 11px;
  color: var(--portal-muted);
  flex-shrink: 0;
}

.desktop-container-manager-item-value {
  font-size: 12px;
  color: var(--portal-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-container-manager-item-actions {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}

.desktop-container-manager-fixed {
  font-size: 11px;
  color: var(--hula-accent);
  border: 1px solid rgba(var(--ui-accent-rgb), 0.35);
  background: var(--ui-accent-soft-2);
  border-radius: 999px;
  padding: 1px 8px;
}

.desktop-container-editor {
  display: grid;
  gap: 12px;
}

.desktop-container-editor-footer {
  width: 100%;
  display: flex;
  align-items: center;
}

.desktop-container-editor-footer-spacer {
  flex: 1;
}

.desktop-container-manager-panel :deep(.el-input__wrapper) {
  background: var(--portal-surface, rgba(255, 255, 255, 0.86));
  box-shadow: 0 0 0 1px var(--portal-border) inset;
  border-radius: 10px;
}

.desktop-container-manager-panel :deep(.el-input__wrapper.is-focus),
.desktop-container-manager-panel :deep(.el-input__wrapper:hover) {
  box-shadow: 0 0 0 1px rgba(var(--ui-accent-rgb), 0.44) inset;
}

@media (max-width: 900px) {
  .desktop-container-manager-item {
    flex-direction: column;
    align-items: stretch;
  }

  .desktop-container-manager-item-actions {
    justify-content: flex-end;
  }
}
</style>
