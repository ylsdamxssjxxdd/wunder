<template>
  <div class="portal-shell desktop-settings-shell">
    <UserTopbar
      :title="t('desktop.containers.title')"
      :subtitle="t('desktop.containers.subtitle')"
      :hide-chat="true"
    />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section">
            <div class="desktop-settings-page" v-loading="loading">
              <el-card>
                <template #header>
                  <div class="desktop-settings-header">
                    <span class="desktop-settings-card-title">{{ t('desktop.containers.defaultWorkspace') }}</span>
                    <el-button @click="goBackToSettings">{{ t('desktop.common.backSettings') }}</el-button>
                  </div>
                </template>

                <el-form label-position="top" class="desktop-form">
                  <el-form-item :label="t('desktop.containers.defaultWorkspace')">
                    <el-input v-model="workspaceRoot" :placeholder="t('desktop.containers.pathPlaceholder')" />
                    <p class="desktop-settings-hint">{{ t('desktop.containers.defaultHint') }}</p>
                  </el-form-item>
                </el-form>

                <div class="desktop-container-toolbar">
                  <el-button type="primary" plain @click="addContainer">
                    {{ t('desktop.containers.add') }}
                  </el-button>
                </div>

                <el-table :data="rows" border>
                  <el-table-column prop="container_id" :label="t('desktop.containers.id')" width="120" />
                  <el-table-column :label="t('desktop.containers.path')">
                    <template #default="{ row }">
                      <el-input v-model="row.root" :placeholder="t('desktop.containers.pathPlaceholder')" />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.common.actions')" width="140" align="center">
                    <template #default="{ row }">
                      <el-button
                        v-if="row.container_id !== 1"
                        link
                        type="danger"
                        @click="removeContainer(row.container_id)"
                      >
                        {{ t('desktop.common.remove') }}
                      </el-button>
                      <span v-else class="desktop-container-fixed">{{ t('desktop.containers.fixed') }}</span>
                    </template>
                  </el-table-column>
                </el-table>

                <div class="desktop-settings-footer">
                  <el-button type="primary" :loading="saving" @click="saveSettings">
                    {{ t('desktop.common.save') }}
                  </el-button>
                </div>
              </el-card>
            </div>
          </section>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue';
import { ElMessage } from 'element-plus';
import { useRouter } from 'vue-router';

import { fetchDesktopSettings, updateDesktopSettings, type DesktopContainerRoot } from '@/api/desktop';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';

const { t } = useI18n();
const router = useRouter();

const loading = ref(false);
const saving = ref(false);
const workspaceRoot = ref('');
const rows = ref<DesktopContainerRoot[]>([]);

const sortRows = () => {
  rows.value.sort((left, right) => left.container_id - right.container_id);
};

const ensureDefaultContainer = () => {
  const first = rows.value.find((item) => item.container_id === 1);
  if (!first) {
    rows.value.unshift({ container_id: 1, root: workspaceRoot.value.trim() });
  } else if (workspaceRoot.value.trim()) {
    first.root = workspaceRoot.value.trim();
  }
  sortRows();
};

const loadSettings = async () => {
  loading.value = true;
  try {
    const response = await fetchDesktopSettings();
    const data = response?.data?.data || {};
    workspaceRoot.value = String(data.workspace_root || '').trim();
    const nextRows = Array.isArray(data.container_roots)
      ? data.container_roots
          .map((item) => ({
            container_id: Number.parseInt(String(item.container_id), 10),
            root: String(item.root || '').trim()
          }))
          .filter((item) => Number.isFinite(item.container_id) && item.container_id > 0)
      : [];
    rows.value = nextRows;
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
  rows.value.push({ container_id: maxId + 1, root: '' });
  sortRows();
};

const removeContainer = (containerId: number) => {
  rows.value = rows.value.filter((item) => item.container_id !== containerId);
};

const goBackToSettings = () => {
  router.push('/desktop/settings');
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
      root: String(item.root || '').trim()
    }))
    .filter((item) => Number.isFinite(item.container_id) && item.container_id > 0);

  const defaultContainer = normalized.find((item) => item.container_id === 1);
  if (defaultContainer) {
    defaultContainer.root = workspace;
  } else {
    normalized.unshift({ container_id: 1, root: workspace });
  }

  for (const item of normalized) {
    if (!item.root) {
      ElMessage.warning(t('desktop.containers.pathRequired', { id: item.container_id }));
      return;
    }
  }

  saving.value = true;
  try {
    await updateDesktopSettings({
      workspace_root: workspace,
      container_roots: normalized
    });
    rows.value = normalized;
    workspaceRoot.value = workspace;
    sortRows();
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    saving.value = false;
  }
};

onMounted(loadSettings);
</script>

<style scoped>
.desktop-settings-shell {
  --desktop-input-bg: rgba(255, 255, 255, 0.06);
  --desktop-table-header-bg: rgba(255, 255, 255, 0.05);
  --desktop-table-row-hover-bg: rgba(255, 255, 255, 0.04);
}

:root[data-user-theme='light'] .desktop-settings-shell {
  --desktop-input-bg: rgba(15, 23, 42, 0.04);
  --desktop-table-header-bg: rgba(15, 23, 42, 0.05);
  --desktop-table-row-hover-bg: rgba(15, 23, 42, 0.03);
}

.desktop-settings-page {
  display: grid;
  gap: 16px;
}

.desktop-settings-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
}

.desktop-settings-card-title {
  font-size: 15px;
  font-weight: 700;
}

.desktop-form {
  margin-bottom: 8px;
}

.desktop-settings-hint {
  margin: 8px 0 0;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-toolbar {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 12px;
}

.desktop-settings-footer {
  display: flex;
  justify-content: flex-end;
  margin-top: 16px;
}

.desktop-container-fixed {
  font-size: 12px;
  color: var(--portal-muted);
}


.desktop-settings-shell :deep(.el-card) {
  border: 1px solid var(--portal-border);
  background: var(--portal-panel);
  color: var(--portal-text);
}

.desktop-settings-shell :deep(.el-card__header) {
  border-bottom: 1px solid var(--portal-border);
}

.desktop-settings-shell :deep(.el-input__wrapper),
.desktop-settings-shell :deep(.el-select__wrapper),
.desktop-settings-shell :deep(.el-textarea__inner) {
  background: var(--desktop-input-bg);
  box-shadow: 0 0 0 1px var(--portal-border) inset;
}

.desktop-settings-shell :deep(.el-form-item__label),
.desktop-settings-shell :deep(.el-input__inner),
.desktop-settings-shell :deep(.el-select__placeholder),
.desktop-settings-shell :deep(.el-textarea__inner) {
  color: var(--portal-text);
}

.desktop-settings-shell :deep(.el-input__inner::placeholder),
.desktop-settings-shell :deep(.el-textarea__inner::placeholder) {
  color: var(--portal-muted);
}

.desktop-settings-shell :deep(.el-table) {
  --el-table-bg-color: transparent;
  --el-table-tr-bg-color: transparent;
  --el-table-header-bg-color: var(--desktop-table-header-bg);
  --el-table-border-color: var(--portal-border);
  --el-table-text-color: var(--portal-text);
  --el-table-header-text-color: var(--portal-muted);
}

.desktop-settings-shell :deep(.el-table__row:hover > td.el-table__cell) {
  background: var(--desktop-table-row-hover-bg);
}

:root[data-user-theme='light'] .desktop-settings-hint,
:root[data-user-theme='light'] .desktop-container-fixed {
  color: #64748b;
}
</style>
