<template>
  <div class="portal-shell desktop-system-shell">
    <UserTopbar
      :title="t('desktop.system.title')"
      :subtitle="t('desktop.system.subtitle')"
      :hide-chat="true"
    />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section">
            <div class="desktop-system-layout" v-loading="loading">
              <aside class="desktop-system-sidebar">
                <div class="desktop-system-sidebar-title">{{ t('desktop.system.title') }}</div>
                <div class="desktop-system-sidebar-nav">
                  <button
                    type="button"
                    class="desktop-system-sidebar-item"
                    :class="{ active: activeSection === 'model' }"
                    @click="setSection('model')"
                  >
                    <i class="fa-solid fa-robot" aria-hidden="true"></i>
                    <span>{{ t('desktop.system.llm') }}</span>
                  </button>
                  <button
                    type="button"
                    class="desktop-system-sidebar-item"
                    :class="{ active: activeSection === 'containers' }"
                    @click="setSection('containers')"
                  >
                    <i class="fa-solid fa-box-archive" aria-hidden="true"></i>
                    <span>{{ t('desktop.settings.containers') }}</span>
                  </button>
                  <button
                    type="button"
                    class="desktop-system-sidebar-item"
                    :class="{ active: activeSection === 'remote' }"
                    @click="setSection('remote')"
                  >
                    <i class="fa-solid fa-link" aria-hidden="true"></i>
                    <span>{{ t('desktop.system.remote.title') }}</span>
                  </button>
                </div>
                <div class="desktop-system-sidebar-foot">
                  <p>{{ currentSection.description }}</p>
                </div>
              </aside>

              <section class="desktop-system-content">
                <header class="desktop-system-header">
                  <div class="desktop-system-header-meta">
                    <h3>{{ currentSection.title }}</h3>
                    <p>{{ currentSection.description }}</p>
                  </div>
                  <div class="desktop-system-header-actions">
                    <template v-if="activeSection === 'model'">
                      <el-button type="primary" plain @click="addModel">
                        {{ t('desktop.system.modelAdd') }}
                      </el-button>
                      <el-button type="primary" :loading="savingModel" @click="saveModelSettings">
                        {{ t('desktop.common.save') }}
                      </el-button>
                    </template>
                    <template v-else-if="activeSection === 'containers'">
                      <el-button type="primary" plain @click="addContainer">
                        {{ t('desktop.containers.add') }}
                      </el-button>
                      <el-button type="primary" :loading="savingContainers" @click="saveContainerSettings">
                        {{ t('desktop.common.save') }}
                      </el-button>
                    </template>
                  </div>
                </header>

                <div v-show="activeSection === 'model'" class="desktop-system-panel">
                  <el-card>
                    <el-form label-position="top" class="desktop-form-grid">
                      <el-form-item :label="t('desktop.system.language')">
                        <el-select v-model="language" class="desktop-full-width">
                          <el-option
                            v-for="item in supportedLanguages"
                            :key="item"
                            :label="getLanguageLabel(item)"
                            :value="item"
                          />
                        </el-select>
                      </el-form-item>
                      <el-form-item :label="t('desktop.system.defaultModel')">
                        <el-select v-model="defaultModel" class="desktop-full-width" filterable allow-create>
                          <el-option
                            v-for="item in modelRows"
                            :key="item.key"
                            :label="item.key || t('desktop.system.modelUnnamed')"
                            :value="item.key"
                          />
                        </el-select>
                      </el-form-item>
                    </el-form>
                  </el-card>

                  <el-card>
                    <el-table :data="modelRows" border>
                      <el-table-column :label="t('desktop.system.modelKey')" width="200">
                        <template #default="{ row }">
                          <el-input v-model="row.key" />
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.system.baseUrl')" min-width="260">
                        <template #default="{ row }">
                          <el-input
                            v-model="row.base_url"
                            :placeholder="t('desktop.system.baseUrlPlaceholder')"
                          />
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.system.apiKey')" min-width="220">
                        <template #default="{ row }">
                          <el-input v-model="row.api_key" show-password />
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.system.modelName')" min-width="220">
                        <template #default="{ row }">
                          <el-input
                            v-model="row.model"
                            :placeholder="t('desktop.system.modelNamePlaceholder')"
                          />
                        </template>
                      </el-table-column>
                      <el-table-column :label="t('desktop.common.actions')" width="120" align="center">
                        <template #default="{ row }">
                          <el-button link type="danger" @click="removeModel(row)">
                            {{ t('desktop.common.remove') }}
                          </el-button>
                        </template>
                      </el-table-column>
                    </el-table>
                    <p class="desktop-settings-hint">{{ t('desktop.system.llmHint') }}</p>
                  </el-card>
                </div>

                <div v-show="activeSection === 'containers'" class="desktop-system-panel">
                  <el-card>
                    <el-form label-position="top">
                      <el-form-item :label="t('desktop.containers.defaultWorkspace')">
                        <el-input
                          v-model="workspaceRoot"
                          :placeholder="t('desktop.containers.pathPlaceholder')"
                        />
                        <p class="desktop-settings-hint">{{ t('desktop.containers.defaultHint') }}</p>
                      </el-form-item>
                    </el-form>

                    <el-table :data="containerRows" border>
                      <el-table-column
                        prop="container_id"
                        :label="t('desktop.containers.id')"
                        width="120"
                      />
                      <el-table-column :label="t('desktop.containers.path')">
                        <template #default="{ row }">
                          <el-input
                            v-model="row.root"
                            :placeholder="t('desktop.containers.pathPlaceholder')"
                          />
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
                  </el-card>
                </div>

                <div v-show="activeSection === 'remote'" class="desktop-system-panel">
                  <el-card>
                    <template #header>
                      <div class="desktop-settings-title-row">
                        <span>{{ t('desktop.system.remote.title') }}</span>
                        <span class="desktop-settings-remote-state" :class="{ connected: remoteConnected }">
                          {{
                            remoteConnected
                              ? t('desktop.system.remote.connected')
                              : t('desktop.system.remote.disconnected')
                          }}
                        </span>
                      </div>
                    </template>

                    <el-form label-position="top" class="desktop-form-grid">
                      <el-form-item :label="t('desktop.system.remote.serverBaseUrl')">
                        <el-input
                          v-model="remoteServerBaseUrl"
                          :placeholder="t('desktop.system.remote.serverPlaceholder')"
                        />
                      </el-form-item>
                    </el-form>

                    <div class="desktop-settings-remote-actions">
                      <el-button
                        type="primary"
                        :loading="connectingRemote"
                        @click="connectRemoteServer"
                      >
                        {{ t('desktop.system.remote.connect') }}
                      </el-button>
                      <el-button :disabled="!remoteConnected" @click="disconnectRemoteServer">
                        {{ t('desktop.system.remote.disconnect') }}
                      </el-button>
                    </div>
                    <p class="desktop-settings-hint">{{ t('desktop.system.remote.hint') }}</p>
                  </el-card>
                </div>
              </section>
            </div>
          </section>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';
import { useRoute, useRouter } from 'vue-router';

import {
  fetchDesktopSettings,
  updateDesktopSettings,
  type DesktopContainerRoot,
  type DesktopRemoteGatewaySettings
} from '@/api/desktop';
import {
  clearDesktopRemoteApiBaseOverride,
  getDesktopLocalToken,
  isDesktopRemoteAuthMode,
  setDesktopRemoteApiBaseOverride
} from '@/config/desktop';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n, getLanguageLabel, setLanguage } from '@/i18n';

type SectionKey = 'model' | 'containers' | 'remote';

type ModelRow = {
  key: string;
  base_url: string;
  api_key: string;
  model: string;
  raw: Record<string, unknown>;
};

const parseModelRows = (models: Record<string, Record<string, unknown>>): ModelRow[] =>
  Object.entries(models || {}).map(([key, raw]) => ({
    key,
    base_url: String(raw.base_url || ''),
    api_key: String(raw.api_key || ''),
    model: String(raw.model || ''),
    raw: { ...raw }
  }));

const buildModelPayload = (row: ModelRow): Record<string, unknown> => {
  const output: Record<string, unknown> = { ...row.raw };

  const setText = (key: string, value: string) => {
    const cleaned = String(value || '').trim();
    if (cleaned) {
      output[key] = cleaned;
    } else {
      delete output[key];
    }
  };

  setText('base_url', row.base_url);
  setText('api_key', row.api_key);
  setText('model', row.model);

  return output;
};

const normalizeSection = (value: unknown): SectionKey => {
  const cleaned = String(value || '').trim().toLowerCase();
  if (cleaned === 'containers') {
    return 'containers';
  }
  if (cleaned === 'remote') {
    return 'remote';
  }
  return 'model';
};

const { t } = useI18n();
const router = useRouter();
const route = useRoute();

const loading = ref(false);
const savingModel = ref(false);
const savingContainers = ref(false);
const connectingRemote = ref(false);

const activeSection = ref<SectionKey>('model');

const language = ref('zh-CN');
const supportedLanguages = ref<string[]>(['zh-CN', 'en-US']);
const defaultModel = ref('');
const modelRows = ref<ModelRow[]>([]);

const workspaceRoot = ref('');
const containerRows = ref<DesktopContainerRoot[]>([]);

const remoteServerBaseUrl = ref('');
const remoteConnected = ref(false);

const currentSection = computed(() => {
  if (activeSection.value === 'containers') {
    return {
      title: t('desktop.settings.containers'),
      description: t('desktop.containers.subtitle')
    };
  }
  if (activeSection.value === 'remote') {
    return {
      title: t('desktop.system.remote.title'),
      description: t('desktop.system.remote.hint')
    };
  }
  return {
    title: t('desktop.system.llm'),
    description: t('desktop.system.llmHint')
  };
});

const refreshRemoteConnected = () => {
  remoteConnected.value = isDesktopRemoteAuthMode();
};

const sortContainerRows = () => {
  containerRows.value.sort((left, right) => left.container_id - right.container_id);
};

const ensureDefaultContainer = () => {
  const workspace = workspaceRoot.value.trim();
  const first = containerRows.value.find((item) => item.container_id === 1);
  if (!first) {
    containerRows.value.unshift({ container_id: 1, root: workspace });
  } else if (workspace) {
    first.root = workspace;
  }
  sortContainerRows();
};

const parseContainerRows = (raw: unknown): DesktopContainerRoot[] => {
  if (!Array.isArray(raw)) {
    return [];
  }
  return raw
    .map((item) => ({
      container_id: Number.parseInt(String(item?.container_id), 10),
      root: String(item?.root || '').trim()
    }))
    .filter((item) => Number.isFinite(item.container_id) && item.container_id > 0);
};

const applySettingsData = (data: Record<string, any>) => {
  const loadedLanguages = Array.isArray(data.supported_languages)
    ? data.supported_languages.map((item: unknown) => String(item || '').trim()).filter(Boolean)
    : [];
  supportedLanguages.value = loadedLanguages.length ? loadedLanguages : ['zh-CN', 'en-US'];
  language.value = String(data.language || supportedLanguages.value[0] || 'zh-CN');

  const llm = data.llm || {};
  defaultModel.value = String(llm.default || '').trim();
  modelRows.value = parseModelRows((llm.models as Record<string, Record<string, unknown>>) || {});
  if (!modelRows.value.length) {
    addModel();
  }
  if (!defaultModel.value) {
    defaultModel.value = modelRows.value[0]?.key || '';
  }

  workspaceRoot.value = String(data.workspace_root || '').trim();
  containerRows.value = parseContainerRows(data.container_roots);
  ensureDefaultContainer();

  remoteServerBaseUrl.value = String(data.remote_gateway?.server_base_url || '').trim();
  refreshRemoteConnected();
};

const setSection = (section: SectionKey) => {
  activeSection.value = section;
  const nextQuery = { ...route.query, section };
  router.replace({ path: '/desktop/system', query: nextQuery });
};

watch(
  () => route.query.section,
  (value) => {
    activeSection.value = normalizeSection(value);
  },
  { immediate: true }
);

const addModel = () => {
  modelRows.value.push({
    key: '',
    base_url: '',
    api_key: '',
    model: '',
    raw: {}
  });
};

const removeModel = (target: ModelRow) => {
  modelRows.value = modelRows.value.filter((item) => item !== target);
  if (!modelRows.value.some((item) => item.key.trim() === defaultModel.value.trim())) {
    defaultModel.value = modelRows.value[0]?.key || '';
  }
};

const addContainer = () => {
  const maxId = containerRows.value.reduce((max, item) => Math.max(max, item.container_id), 1);
  containerRows.value.push({ container_id: maxId + 1, root: '' });
  sortContainerRows();
};

const removeContainer = (containerId: number) => {
  if (containerId === 1) {
    return;
  }
  containerRows.value = containerRows.value.filter((item) => item.container_id !== containerId);
};

const loadSettings = async () => {
  loading.value = true;
  try {
    const response = await fetchDesktopSettings();
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data);
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const saveModelSettings = async () => {
  const models: Record<string, Record<string, unknown>> = {};

  for (const row of modelRows.value) {
    const key = row.key.trim();
    if (!key) {
      ElMessage.warning(t('desktop.system.modelKeyRequired'));
      return;
    }
    if (models[key]) {
      ElMessage.warning(t('desktop.system.modelKeyDuplicate', { key }));
      return;
    }
    models[key] = buildModelPayload(row);
  }

  const currentDefaultModel = defaultModel.value.trim() || Object.keys(models)[0] || '';
  if (!currentDefaultModel) {
    ElMessage.warning(t('desktop.system.defaultModelRequired'));
    return;
  }
  if (!models[currentDefaultModel]) {
    ElMessage.warning(t('desktop.system.defaultModelMissing'));
    return;
  }

  const defaultModelConfig = models[currentDefaultModel] || {};
  const defaultBaseUrl = String(defaultModelConfig.base_url || '').trim();
  const defaultModelName = String(defaultModelConfig.model || '').trim();
  if (!defaultBaseUrl || !defaultModelName) {
    ElMessage.warning(t('desktop.system.defaultModelConfigRequired'));
    return;
  }

  const selectedLanguage = language.value.trim();
  if (!selectedLanguage) {
    ElMessage.warning(t('desktop.system.languageRequired'));
    return;
  }

  savingModel.value = true;
  try {
    const response = await updateDesktopSettings({
      language: selectedLanguage,
      llm: {
        default: currentDefaultModel,
        models
      }
    });
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data);
    setLanguage(language.value, { force: true });
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    savingModel.value = false;
  }
};

const saveContainerSettings = async () => {
  const workspace = workspaceRoot.value.trim();
  if (!workspace) {
    ElMessage.warning(t('desktop.containers.workspaceRequired'));
    return;
  }

  const normalized = containerRows.value
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

  savingContainers.value = true;
  try {
    const response = await updateDesktopSettings({
      workspace_root: workspace,
      container_roots: normalized
    });
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data);
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    savingContainers.value = false;
  }
};

const connectRemoteServer = async () => {
  const rawUrl = remoteServerBaseUrl.value.trim();
  if (!rawUrl) {
    ElMessage.warning(t('desktop.system.remote.serverRequired'));
    return;
  }

  const normalizedApiBase = setDesktopRemoteApiBaseOverride(rawUrl);
  if (!normalizedApiBase) {
    ElMessage.warning(t('desktop.system.remote.serverInvalid'));
    return;
  }

  connectingRemote.value = true;
  try {
    const payload: { remote_gateway: DesktopRemoteGatewaySettings } = {
      remote_gateway: {
        enabled: true,
        server_base_url: rawUrl
      }
    };
    await updateDesktopSettings(payload);

    try {
      localStorage.removeItem('access_token');
    } catch {
      // ignore localStorage failures
    }

    refreshRemoteConnected();
    ElMessage.success(t('desktop.system.remote.connectSuccess'));
    router.push('/login');
  } catch (error) {
    clearDesktopRemoteApiBaseOverride();
    console.error(error);
    ElMessage.error(t('desktop.system.remote.connectFailed'));
  } finally {
    connectingRemote.value = false;
  }
};

const disconnectRemoteServer = async () => {
  connectingRemote.value = true;
  try {
    await updateDesktopSettings({
      remote_gateway: {
        enabled: false,
        server_base_url: ''
      }
    });

    clearDesktopRemoteApiBaseOverride();
    const localToken = getDesktopLocalToken();
    if (localToken) {
      try {
        localStorage.setItem('access_token', localToken);
      } catch {
        // ignore localStorage failures
      }
    }

    remoteServerBaseUrl.value = '';
    refreshRemoteConnected();
    ElMessage.success(t('desktop.system.remote.disconnectSuccess'));
    router.push('/desktop/home');
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.system.remote.disconnectFailed'));
  } finally {
    connectingRemote.value = false;
  }
};

onMounted(() => {
  refreshRemoteConnected();
  loadSettings();
});
</script>

<style scoped>
.desktop-system-shell {
  --desktop-input-bg: rgba(255, 255, 255, 0.06);
  --desktop-table-header-bg: rgba(255, 255, 255, 0.05);
  --desktop-table-row-hover-bg: rgba(255, 255, 255, 0.04);
}

:root[data-user-theme='light'] .desktop-system-shell {
  --desktop-input-bg: rgba(15, 23, 42, 0.04);
  --desktop-table-header-bg: rgba(15, 23, 42, 0.05);
  --desktop-table-row-hover-bg: rgba(15, 23, 42, 0.03);
}

.desktop-system-layout {
  display: grid;
  grid-template-columns: 220px minmax(0, 1fr);
  gap: 16px;
  min-height: calc(100vh - 170px);
  align-items: stretch;
}

.desktop-system-sidebar {
  border: 1px solid var(--portal-border);
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  align-self: stretch;
  height: 100%;
  min-height: calc(100vh - 194px);
}

.desktop-system-sidebar-title {
  font-size: 13px;
  font-weight: 700;
  color: var(--portal-muted);
  padding: 4px 8px;
}

.desktop-system-sidebar-nav {
  display: grid;
  gap: 8px;
}

.desktop-system-sidebar-item {
  width: 100%;
  border: 1px solid transparent;
  background: transparent;
  color: var(--portal-text);
  padding: 10px 12px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.2s ease;
}

.desktop-system-sidebar-item:hover {
  border-color: var(--portal-border);
  background: rgba(255, 255, 255, 0.04);
}

.desktop-system-sidebar-item.active {
  border-color: rgba(var(--portal-primary-rgb), 0.5);
  background: rgba(var(--portal-primary-rgb), 0.15);
}

.desktop-system-sidebar-foot {
  margin-top: 0;
  flex: 1;
  border: 1px dashed var(--portal-border);
  border-radius: 10px;
  background: rgba(var(--portal-primary-rgb), 0.08);
  padding: 10px 12px;
  display: flex;
  align-items: flex-end;
}

.desktop-system-sidebar-foot p {
  margin: 0;
  width: 100%;
  color: var(--portal-muted);
  font-size: 12px;
  line-height: 1.5;
}

:root[data-user-theme='light'] .desktop-system-sidebar-item:hover {
  background: rgba(15, 23, 42, 0.03);
}

:root[data-user-theme='light'] .desktop-system-sidebar-item.active {
  background: rgba(var(--portal-primary-rgb), 0.12);
}

:root[data-user-theme='light'] .desktop-system-sidebar-foot {
  background: rgba(var(--portal-primary-rgb), 0.06);
}

.desktop-system-content {
  min-width: 0;
  display: grid;
  gap: 16px;
  align-content: start;
}

.desktop-system-header {
  border: 1px solid var(--portal-border);
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 14px 16px;
  display: flex;
  justify-content: space-between;
  gap: 16px;
  align-items: flex-start;
  position: sticky;
  top: 12px;
  z-index: 4;
}

.desktop-system-header-meta h3 {
  margin: 0;
  font-size: 16px;
  font-weight: 700;
}

.desktop-system-header-meta p {
  margin: 8px 0 0;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-system-header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.desktop-system-panel {
  display: grid;
  gap: 16px;
}

.desktop-form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
  gap: 12px;
}

.desktop-full-width {
  width: 100%;
}

.desktop-settings-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.desktop-settings-remote-state {
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-settings-remote-state.connected {
  color: #22c55e;
}

.desktop-settings-remote-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.desktop-settings-hint {
  margin: 12px 0 0;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-container-fixed {
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-system-shell :deep(.el-card) {
  border: 1px solid var(--portal-border);
  background: var(--portal-panel);
  color: var(--portal-text);
}

.desktop-system-shell :deep(.el-card__header) {
  border-bottom: 1px solid var(--portal-border);
}

.desktop-system-shell :deep(.el-input__wrapper),
.desktop-system-shell :deep(.el-select__wrapper),
.desktop-system-shell :deep(.el-textarea__inner) {
  background: var(--desktop-input-bg);
  box-shadow: 0 0 0 1px var(--portal-border) inset;
}

.desktop-system-shell :deep(.el-form-item__label),
.desktop-system-shell :deep(.el-input__inner),
.desktop-system-shell :deep(.el-select__placeholder),
.desktop-system-shell :deep(.el-textarea__inner) {
  color: var(--portal-text);
}

.desktop-system-shell :deep(.el-input__inner::placeholder),
.desktop-system-shell :deep(.el-textarea__inner::placeholder) {
  color: var(--portal-muted);
}

.desktop-system-shell :deep(.el-table) {
  --el-table-bg-color: transparent;
  --el-table-tr-bg-color: transparent;
  --el-table-header-bg-color: var(--desktop-table-header-bg);
  --el-table-border-color: var(--portal-border);
  --el-table-text-color: var(--portal-text);
  --el-table-header-text-color: var(--portal-muted);
}

.desktop-system-shell :deep(.el-table__row:hover > td.el-table__cell) {
  background: var(--desktop-table-row-hover-bg);
}

:root[data-user-theme='light'] .desktop-settings-remote-state.connected {
  color: #16a34a;
}

@media (max-width: 1100px) {
  .desktop-system-layout {
    grid-template-columns: 1fr;
  }

  .desktop-system-sidebar {
    min-height: 0;
    height: auto;
  }

  .desktop-system-sidebar-nav {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .desktop-system-header {
    position: static;
  }
}

@media (max-width: 760px) {
  .desktop-system-sidebar-nav {
    grid-template-columns: 1fr;
  }

  .desktop-system-header {
    flex-direction: column;
  }

  .desktop-system-header-actions {
    width: 100%;
    justify-content: flex-start;
  }
}
</style>
