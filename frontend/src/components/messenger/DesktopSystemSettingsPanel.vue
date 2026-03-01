<template>
  <section
    v-if="showModelPanel"
    class="messenger-settings-card desktop-system-settings-panel desktop-system-settings-panel--llm"
    v-loading="loading"
  >
    <div class="desktop-system-settings-head">
      <div>
        <div class="messenger-settings-title">{{ t('desktop.system.llm') }}</div>
        <div class="messenger-settings-subtitle">{{ t('desktop.system.llmHint') }}</div>
      </div>
      <div class="desktop-system-settings-actions">
        <el-button type="primary" plain size="small" @click="addModel">
          {{ t('desktop.system.modelAdd') }}
        </el-button>
        <el-button type="primary" size="small" :loading="savingModel" @click="saveModelSettings">
          {{ t('desktop.common.save') }}
        </el-button>
      </div>
    </div>

    <div class="desktop-system-settings-section">
      <div class="desktop-system-settings-form-grid">
        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.language') }}</span>
          <el-select v-model="language" class="desktop-system-settings-input">
            <el-option
              v-for="item in supportedLanguages"
              :key="item"
              :label="getLanguageLabel(item)"
              :value="item"
            />
          </el-select>
        </label>

        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.defaultModel') }}</span>
          <el-select v-model="defaultModel" class="desktop-system-settings-input" filterable allow-create>
            <el-option
              v-for="item in modelRows"
              :key="item.key || item.uid"
              :label="item.key || t('desktop.system.modelUnnamed')"
              :value="item.key"
            />
          </el-select>
        </label>

        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.toolCallMode') }}</span>
          <el-select v-model="toolCallMode" class="desktop-system-settings-input">
            <el-option label="tool_call" value="tool_call" />
            <el-option label="function_call" value="function_call" />
          </el-select>
          <span class="desktop-system-settings-field-hint">{{ t('desktop.system.toolCallHint') }}</span>
        </label>
      </div>
    </div>

    <div class="desktop-system-settings-section">
      <div class="desktop-system-settings-model-list-title">{{ t('desktop.system.modelsTitle') }}</div>
      <div class="desktop-system-settings-model-list">
        <div v-for="row in modelRows" :key="row.uid" class="desktop-system-settings-model-item">
          <div class="desktop-system-settings-model-item-head">
            <span class="desktop-system-settings-model-item-name">
              {{ row.key || t('desktop.system.modelUnnamed') }}
            </span>
            <el-button link type="danger" @click="removeModel(row)">
              {{ t('desktop.common.remove') }}
            </el-button>
          </div>
          <div class="desktop-system-settings-model-grid">
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.modelKey') }}</span>
              <el-input v-model="row.key" />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.baseUrl') }}</span>
              <el-input v-model="row.base_url" :placeholder="t('desktop.system.baseUrlPlaceholder')" />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.apiKey') }}</span>
              <el-input v-model="row.api_key" show-password />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.modelName') }}</span>
              <el-input v-model="row.model" :placeholder="t('desktop.system.modelNamePlaceholder')" />
            </label>
          </div>
        </div>
      </div>
    </div>
  </section>

  <section v-if="showRemotePanel" class="messenger-settings-card desktop-system-settings-panel">
    <div class="desktop-system-settings-head">
      <div>
        <div class="messenger-settings-title">{{ t('desktop.system.remote.title') }}</div>
        <div class="messenger-settings-subtitle">{{ t('desktop.system.remote.hint') }}</div>
      </div>
      <span class="desktop-system-settings-remote-state" :class="{ connected: remoteConnected }">
        {{
          remoteConnected
            ? t('desktop.system.remote.connected')
            : t('desktop.system.remote.disconnected')
        }}
      </span>
    </div>

    <div class="desktop-system-settings-section">
      <label class="desktop-system-settings-field">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.remote.serverBaseUrl') }}</span>
        <el-input
          v-model="remoteServerBaseUrl"
          :placeholder="t('desktop.system.remote.serverPlaceholder')"
        />
      </label>

      <div class="desktop-system-settings-actions">
        <el-button type="primary" :loading="connectingRemote" @click="connectRemoteServer">
          {{ t('desktop.system.remote.connect') }}
        </el-button>
        <el-button :disabled="!remoteConnected || connectingRemote" @click="disconnectRemoteServer">
          {{ t('desktop.system.remote.disconnect') }}
        </el-button>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { ElMessage } from 'element-plus';
import { useRouter } from 'vue-router';

import { fetchDesktopSettings, updateDesktopSettings, type DesktopRemoteGatewaySettings } from '@/api/desktop';
import {
  clearDesktopRemoteApiBaseOverride,
  getDesktopToolCallMode,
  getDesktopLocalToken,
  getDesktopRemoteApiBaseOverride,
  isDesktopRemoteAuthMode,
  setDesktopToolCallMode,
  setDesktopRemoteApiBaseOverride
} from '@/config/desktop';
import { useI18n, getLanguageLabel, setLanguage } from '@/i18n';
import type { DesktopToolCallMode } from '@/config/desktop';

type ModelRow = {
  uid: string;
  key: string;
  base_url: string;
  api_key: string;
  model: string;
  raw: Record<string, unknown>;
};

const props = withDefaults(
  defineProps<{
    panel?: 'models' | 'remote' | 'all';
  }>(),
  {
    panel: 'all'
  }
);

const { t } = useI18n();
const router = useRouter();

const loading = ref(false);
const savingModel = ref(false);
const connectingRemote = ref(false);
const supportedLanguages = ref<string[]>(['zh-CN', 'en-US']);
const language = ref('zh-CN');
const defaultModel = ref('');
const modelRows = ref<ModelRow[]>([]);
const toolCallMode = ref<DesktopToolCallMode>(getDesktopToolCallMode());
const remoteServerBaseUrl = ref('');
const remoteConnected = ref(false);
let nextModelUid = 1;

const makeModelUid = (): string => `desktop-model-${nextModelUid++}`;

const showModelPanel = computed(() => props.panel !== 'remote');
const showRemotePanel = computed(() => props.panel !== 'models');

const normalizeToolCallMode = (value: unknown): DesktopToolCallMode =>
  String(value || '').trim().toLowerCase() === 'function_call' ? 'function_call' : 'tool_call';

const resolveToolCallMode = (models: Record<string, Record<string, unknown>>, fallbackModel: string): DesktopToolCallMode => {
  const preferredModel = models[fallbackModel] || Object.values(models)[0] || {};
  return normalizeToolCallMode(preferredModel?.tool_call_mode || getDesktopToolCallMode());
};

const parseModelRows = (models: Record<string, Record<string, unknown>>): ModelRow[] =>
  Object.entries(models || {}).map(([key, raw]) => ({
    uid: makeModelUid(),
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

const refreshRemoteConnected = () => {
  const override = getDesktopRemoteApiBaseOverride();
  remoteConnected.value = isDesktopRemoteAuthMode() && Boolean(override);
};

const addModel = () => {
  modelRows.value.push({
    uid: makeModelUid(),
    key: '',
    base_url: '',
    api_key: '',
    model: '',
    raw: {}
  });
};

const removeModel = (target: ModelRow) => {
  modelRows.value = modelRows.value.filter((item) => item.uid !== target.uid);
  if (!modelRows.value.some((item) => item.key.trim() === defaultModel.value.trim())) {
    defaultModel.value = modelRows.value[0]?.key || '';
  }
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
  toolCallMode.value = resolveToolCallMode(
    (llm.models as Record<string, Record<string, unknown>>) || {},
    defaultModel.value
  );
  setDesktopToolCallMode(toolCallMode.value);

  remoteServerBaseUrl.value = String(data.remote_gateway?.server_base_url || '').trim();
  refreshRemoteConnected();
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
    const payload = buildModelPayload(row);
    payload.tool_call_mode = toolCallMode.value;
    models[key] = payload;
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
    setDesktopToolCallMode(toolCallMode.value);
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    savingModel.value = false;
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
  void loadSettings();
});
</script>

<style scoped>
.desktop-system-settings-panel {
  display: grid;
  gap: 14px;
}

.desktop-system-settings-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.desktop-system-settings-section {
  border: 1px solid var(--portal-border);
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 12px;
  display: grid;
  gap: 12px;
}

.desktop-system-settings-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.desktop-system-settings-field-label {
  color: var(--portal-text);
  font-size: 12px;
}

.desktop-system-settings-form-grid {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.desktop-system-settings-field {
  display: grid;
  gap: 6px;
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-system-settings-field-hint {
  font-size: 11px;
  color: var(--portal-muted);
}

.desktop-system-settings-input {
  width: 100%;
}

.desktop-system-settings-model-list-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--portal-text);
}

.desktop-system-settings-model-list {
  display: grid;
  gap: 10px;
}

.desktop-system-settings-model-item {
  border: 1px solid var(--portal-border);
  border-radius: 10px;
  background: var(--portal-surface, rgba(255, 255, 255, 0.9));
  padding: 11px;
  display: grid;
  gap: 10px;
}

.desktop-system-settings-model-item-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.desktop-system-settings-model-item-name {
  font-size: 12px;
  color: var(--portal-text);
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-system-settings-model-grid {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.desktop-system-settings-remote-state {
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-system-settings-remote-state.connected {
  color: #16a34a;
}

.desktop-system-settings-panel :deep(.el-input__wrapper),
.desktop-system-settings-panel :deep(.el-select__wrapper) {
  background: var(--portal-surface, rgba(255, 255, 255, 0.86));
  box-shadow: 0 0 0 1px var(--portal-border) inset;
  border-radius: 10px;
  transition: box-shadow 0.15s ease, background-color 0.15s ease;
}

.desktop-system-settings-panel :deep(.el-input__wrapper:hover),
.desktop-system-settings-panel :deep(.el-select__wrapper:hover) {
  box-shadow: 0 0 0 1px rgba(var(--ui-accent-rgb), 0.35) inset;
}

.desktop-system-settings-panel :deep(.el-input__wrapper.is-focus),
.desktop-system-settings-panel :deep(.el-select__wrapper.is-focused) {
  box-shadow: 0 0 0 1.5px rgba(var(--ui-accent-rgb), 0.58) inset;
}

@media (max-width: 900px) {
  .desktop-system-settings-form-grid,
  .desktop-system-settings-model-grid {
    grid-template-columns: 1fr;
  }
}
</style>
