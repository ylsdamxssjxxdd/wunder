<template>
  <div class="portal-shell desktop-settings-shell">
    <UserTopbar
      :title="t('desktop.system.title')"
      :subtitle="t('desktop.system.subtitle')"
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
                    <span class="desktop-settings-card-title">{{ t('desktop.system.language') }}</span>
                    <el-button @click="goBackToSettings">{{ t('desktop.common.backSettings') }}</el-button>
                  </div>
                </template>

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
                </el-form>
              </el-card>

              <el-card>
                <template #header>
                  <div class="desktop-settings-title-row">
                    <span>{{ t('desktop.system.llm') }}</span>
                    <el-button type="primary" plain @click="addModel">{{ t('desktop.system.modelAdd') }}</el-button>
                  </div>
                </template>

                <el-form label-position="top" class="desktop-form-grid">
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

                <el-table :data="modelRows" border>
                  <el-table-column :label="t('desktop.system.modelKey')" width="200">
                    <template #default="{ row }">
                      <el-input v-model="row.key" />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.system.baseUrl')" min-width="260">
                    <template #default="{ row }">
                      <el-input v-model="row.base_url" :placeholder="t('desktop.system.baseUrlPlaceholder')" />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.system.apiKey')" min-width="220">
                    <template #default="{ row }">
                      <el-input v-model="row.api_key" show-password />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.system.modelName')" min-width="220">
                    <template #default="{ row }">
                      <el-input v-model="row.model" :placeholder="t('desktop.system.modelNamePlaceholder')" />
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

              <el-card>
                <template #header>
                  <div class="desktop-settings-title-row">
                    <span>{{ t('desktop.system.remote.title') }}</span>
                    <span class="desktop-settings-remote-state" :class="{ connected: remoteConnected }">
                      {{ remoteConnected ? t('desktop.system.remote.connected') : t('desktop.system.remote.disconnected') }}
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
                  <el-button type="primary" :loading="connectingRemote" @click="connectRemoteServer">
                    {{ t('desktop.system.remote.connect') }}
                  </el-button>
                  <el-button :disabled="!remoteConnected" @click="disconnectRemoteServer">
                    {{ t('desktop.system.remote.disconnect') }}
                  </el-button>
                </div>
                <p class="desktop-settings-hint">{{ t('desktop.system.remote.hint') }}</p>
              </el-card>

              <div class="desktop-settings-footer">
                <el-button type="primary" :loading="saving" @click="saveSettings">
                  {{ t('desktop.common.save') }}
                </el-button>
              </div>
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

import {
  fetchDesktopSettings,
  updateDesktopSettings,
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

const { t } = useI18n();
const router = useRouter();

const loading = ref(false);
const saving = ref(false);
const connectingRemote = ref(false);
const language = ref('zh-CN');
const supportedLanguages = ref<string[]>(['zh-CN', 'en-US']);
const defaultModel = ref('');
const modelRows = ref<ModelRow[]>([]);
const remoteServerBaseUrl = ref('');

const remoteConnected = ref(false);

const refreshRemoteConnected = () => {
  remoteConnected.value = isDesktopRemoteAuthMode();
};

const goBackToSettings = () => {
  router.push('/desktop/settings');
};

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

const loadSettings = async () => {
  loading.value = true;
  try {
    const response = await fetchDesktopSettings();
    const data = response?.data?.data || {};

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

    remoteServerBaseUrl.value = String(data.remote_gateway?.server_base_url || '').trim();
    refreshRemoteConnected();
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const saveSettings = async () => {
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

  const selectedLanguage = language.value.trim();
  if (!selectedLanguage) {
    ElMessage.warning(t('desktop.system.languageRequired'));
    return;
  }

  saving.value = true;
  try {
    await updateDesktopSettings({
      language: selectedLanguage,
      llm: {
        default: currentDefaultModel,
        models
      }
    });

    defaultModel.value = currentDefaultModel;
    setLanguage(selectedLanguage, { force: true });
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    saving.value = false;
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
}

.desktop-settings-hint {
  margin: 12px 0 0;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-settings-footer {
  display: flex;
  justify-content: flex-end;
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

:root[data-user-theme='light'] .desktop-settings-remote-state.connected {
  color: #16a34a;
}
</style>
