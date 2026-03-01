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
        <el-button type="primary" size="small" :loading="savingModel" @click="saveModelSettings">
          {{ t('desktop.common.save') }}
        </el-button>
      </div>
    </div>

    <div class="desktop-system-settings-section">
      <div class="desktop-system-settings-form-grid">
        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.defaultChatModel') }}</span>
          <el-select v-model="defaultModel" class="desktop-system-settings-input" filterable allow-create>
            <el-option
              v-for="item in llmModelRows"
              :key="item.key || item.uid"
              :label="item.key || t('desktop.system.modelUnnamed')"
              :value="item.key"
            />
          </el-select>
        </label>

        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">
            {{ t('desktop.system.defaultEmbeddingModel') }}
          </span>
          <el-select
            v-model="defaultEmbeddingModel"
            class="desktop-system-settings-input"
            filterable
            clearable
          >
            <el-option
              v-for="item in embeddingModelRows"
              :key="item.key || item.uid"
              :label="item.key || t('desktop.system.modelUnnamed')"
              :value="item.key"
            />
          </el-select>
        </label>
      </div>
    </div>

    <div class="desktop-system-settings-layout">
      <aside class="desktop-system-settings-model-list-wrap">
        <div class="desktop-system-settings-model-list-head">
          <span class="desktop-system-settings-model-list-title">{{ t('desktop.system.modelsTitle') }}</span>
          <el-button type="primary" plain size="small" @click="addModel()">
            {{ t('desktop.system.modelAdd') }}
          </el-button>
        </div>
        <div class="desktop-system-settings-model-list">
          <button
            v-for="row in modelRows"
            :key="row.uid"
            class="desktop-system-settings-model-item"
            :class="{ active: selectedModelUid === row.uid }"
            type="button"
            @click="selectModel(row.uid)"
          >
            <div class="desktop-system-settings-model-item-head">
              <span class="desktop-system-settings-model-item-name">
                {{ row.key || t('desktop.system.modelUnnamed') }}
              </span>
              <span class="desktop-system-settings-model-item-type">
                {{
                  row.model_type === 'embedding'
                    ? t('desktop.system.modelTypeEmbedding')
                    : t('desktop.system.modelTypeLlm')
                }}
              </span>
            </div>
            <div class="desktop-system-settings-model-item-meta">
              {{ row.model || '-' }} · {{ row.base_url || '-' }}
            </div>
            <div class="desktop-system-settings-model-item-badges">
              <span v-if="row.key.trim() === defaultModel.trim()" class="desktop-system-settings-badge">
                {{ t('desktop.system.defaultChatModel') }}
              </span>
              <span
                v-if="
                  row.key.trim() &&
                  defaultEmbeddingModel.trim() &&
                  row.key.trim() === defaultEmbeddingModel.trim()
                "
                class="desktop-system-settings-badge desktop-system-settings-badge--alt"
              >
                {{ t('desktop.system.defaultEmbeddingModel') }}
              </span>
            </div>
          </button>
          <div v-if="!modelRows.length" class="desktop-system-settings-empty">
            {{ t('desktop.system.modelListEmpty') }}
          </div>
        </div>
      </aside>

      <section v-if="selectedModel" class="desktop-system-settings-detail">
        <div class="desktop-system-settings-detail-head">
          <div class="desktop-system-settings-detail-title">
            {{ selectedModel.key || t('desktop.system.modelUnnamed') }}
          </div>
          <div class="desktop-system-settings-actions">
            <el-button plain size="small" @click="setCurrentAsDefault('llm')">
              {{ t('desktop.system.setDefaultChatModel') }}
            </el-button>
            <el-button plain size="small" @click="setCurrentAsDefault('embedding')">
              {{ t('desktop.system.setDefaultEmbeddingModel') }}
            </el-button>
            <el-button plain type="danger" size="small" @click="removeModel(selectedModel)">
              {{ t('desktop.common.remove') }}
            </el-button>
          </div>
        </div>

        <div class="desktop-system-settings-section">
          <div class="desktop-system-settings-model-grid">
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.modelKey') }}</span>
              <el-input v-model="selectedModel.key" />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.modelType') }}</span>
              <el-select v-model="selectedModel.model_type" class="desktop-system-settings-input">
                <el-option :label="t('desktop.system.modelTypeLlm')" value="llm" />
                <el-option :label="t('desktop.system.modelTypeEmbedding')" value="embedding" />
              </el-select>
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.provider') }}</span>
              <el-input v-model="selectedModel.provider" />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.modelName') }}</span>
              <el-input
                v-model="selectedModel.model"
                :placeholder="t('desktop.system.modelNamePlaceholder')"
              />
            </label>
            <label class="desktop-system-settings-field desktop-system-settings-field--full">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.baseUrl') }}</span>
              <el-input
                v-model="selectedModel.base_url"
                :placeholder="t('desktop.system.baseUrlPlaceholder')"
              />
            </label>
            <label class="desktop-system-settings-field desktop-system-settings-field--full">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.apiKey') }}</span>
              <el-input v-model="selectedModel.api_key" show-password />
            </label>
          </div>
        </div>

        <div class="desktop-system-settings-section">
          <div class="desktop-system-settings-model-grid">
            <label v-if="selectedModel.model_type === 'llm'" class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.temperature') }}</span>
              <el-input v-model="selectedModel.temperature" />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.timeout') }}</span>
              <el-input v-model="selectedModel.timeout_s" />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.retry') }}</span>
              <el-input v-model="selectedModel.retry" />
            </label>
            <label v-if="selectedModel.model_type === 'llm'" class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.maxOutput') }}</span>
              <el-input v-model="selectedModel.max_output" />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.maxContext') }}</span>
              <el-input v-model="selectedModel.max_context" />
            </label>
            <label v-if="selectedModel.model_type === 'llm'" class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.toolCallMode') }}</span>
              <el-select v-model="selectedModel.tool_call_mode" class="desktop-system-settings-input">
                <el-option label="tool_call" value="tool_call" />
                <el-option label="function_call" value="function_call" />
              </el-select>
            </label>
          </div>
        </div>
      </section>

      <section v-else class="desktop-system-settings-empty-panel">
        {{ t('desktop.system.modelDetailEmpty') }}
      </section>
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
  getDesktopLocalToken,
  getDesktopRemoteApiBaseOverride,
  isDesktopRemoteAuthMode,
  setDesktopRemoteApiBaseOverride
} from '@/config/desktop';
import { useI18n } from '@/i18n';

type ModelType = 'llm' | 'embedding';
type ToolCallMode = 'tool_call' | 'function_call';
type ModelRow = {
  uid: string;
  key: string;
  model_type: ModelType;
  provider: string;
  base_url: string;
  api_key: string;
  model: string;
  temperature: string;
  timeout_s: string;
  retry: string;
  max_output: string;
  max_context: string;
  tool_call_mode: ToolCallMode;
  raw: Record<string, unknown>;
};

const EMBEDDING_DEFAULT_MODEL_STORAGE_KEY = 'wunder_desktop_default_embedding_model';

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
const defaultModel = ref('');
const defaultEmbeddingModel = ref('');
const modelRows = ref<ModelRow[]>([]);
const selectedModelUid = ref('');
const remoteServerBaseUrl = ref('');
const remoteConnected = ref(false);
let nextModelUid = 1;

const makeModelUid = (): string => `desktop-model-${nextModelUid++}`;

const showModelPanel = computed(() => props.panel !== 'remote');
const showRemotePanel = computed(() => props.panel !== 'models');

const llmModelRows = computed(() => modelRows.value.filter((item) => item.model_type === 'llm'));
const embeddingModelRows = computed(() =>
  modelRows.value.filter((item) => item.model_type === 'embedding')
);
const selectedModel = computed(
  () => modelRows.value.find((item) => item.uid === selectedModelUid.value) || null
);

const normalizeModelType = (value: unknown): ModelType => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'embedding' || raw === 'embed' || raw === 'embeddings') {
    return 'embedding';
  }
  return 'llm';
};

const normalizeToolCallMode = (value: unknown): ToolCallMode =>
  String(value || '').trim().toLowerCase() === 'function_call' ? 'function_call' : 'tool_call';

const parseModelRows = (models: Record<string, Record<string, unknown>>): ModelRow[] =>
  Object.entries(models || {}).map(([key, raw]) => ({
    uid: makeModelUid(),
    key,
    model_type: normalizeModelType(raw.model_type),
    provider: String(raw.provider || 'openai_compatible'),
    base_url: String(raw.base_url || ''),
    api_key: String(raw.api_key || ''),
    model: String(raw.model || ''),
    temperature: raw.temperature == null ? '0.7' : String(raw.temperature),
    timeout_s: raw.timeout_s == null ? '120' : String(raw.timeout_s),
    retry: raw.retry == null ? '1' : String(raw.retry),
    max_output: raw.max_output == null ? '' : String(raw.max_output),
    max_context: raw.max_context == null ? '' : String(raw.max_context),
    tool_call_mode: normalizeToolCallMode(raw.tool_call_mode),
    raw: { ...raw }
  }));

const ensureSelectedModel = () => {
  if (!modelRows.value.length) {
    selectedModelUid.value = '';
    return;
  }
  if (!modelRows.value.some((item) => item.uid === selectedModelUid.value)) {
    selectedModelUid.value = modelRows.value[0].uid;
  }
};

const findDefaultModelKeyByType = (
  rows: ModelRow[],
  modelType: ModelType,
  desiredKey: string
): string => {
  const desired = String(desiredKey || '').trim();
  if (desired) {
    const matched = rows.find(
      (item) => item.key.trim() === desired && normalizeModelType(item.model_type) === modelType
    );
    if (matched) {
      return matched.key.trim();
    }
  }
  return rows.find((item) => normalizeModelType(item.model_type) === modelType)?.key.trim() || '';
};

const readDefaultEmbeddingModel = (): string => {
  try {
    return String(localStorage.getItem(EMBEDDING_DEFAULT_MODEL_STORAGE_KEY) || '').trim();
  } catch {
    return '';
  }
};

const writeDefaultEmbeddingModel = (modelName: string): void => {
  const normalized = String(modelName || '').trim();
  try {
    if (normalized) {
      localStorage.setItem(EMBEDDING_DEFAULT_MODEL_STORAGE_KEY, normalized);
    } else {
      localStorage.removeItem(EMBEDDING_DEFAULT_MODEL_STORAGE_KEY);
    }
  } catch {
    // ignore localStorage failures
  }
};

const addModel = (modelType: ModelType = 'llm') => {
  const row: ModelRow = {
    uid: makeModelUid(),
    key: '',
    model_type: modelType,
    provider: 'openai_compatible',
    base_url: '',
    api_key: '',
    model: '',
    temperature: modelType === 'llm' ? '0.7' : '',
    timeout_s: '120',
    retry: '1',
    max_output: '',
    max_context: '',
    tool_call_mode: 'tool_call',
    raw: {}
  };
  modelRows.value.push(row);
  selectedModelUid.value = row.uid;
};

const selectModel = (uid: string) => {
  selectedModelUid.value = uid;
};

const setCurrentAsDefault = (modelType: ModelType) => {
  const current = selectedModel.value;
  if (!current) return;
  const key = current.key.trim();
  if (!key) {
    ElMessage.warning(t('desktop.system.modelKeyRequired'));
    return;
  }
  if (normalizeModelType(current.model_type) !== modelType) {
    ElMessage.warning(
      t('desktop.system.modelTypeMismatch', {
        type:
          modelType === 'embedding'
            ? t('desktop.system.modelTypeEmbedding')
            : t('desktop.system.modelTypeLlm')
      })
    );
    return;
  }
  if (modelType === 'embedding') {
    defaultEmbeddingModel.value = key;
  } else {
    defaultModel.value = key;
  }
};

const removeModel = (target: ModelRow) => {
  modelRows.value = modelRows.value.filter((item) => item.uid !== target.uid);
  defaultModel.value = findDefaultModelKeyByType(modelRows.value, 'llm', defaultModel.value);
  defaultEmbeddingModel.value = findDefaultModelKeyByType(
    modelRows.value,
    'embedding',
    defaultEmbeddingModel.value
  );
  ensureSelectedModel();
};

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

  const setFloat = (key: string, value: string) => {
    const cleaned = String(value || '').trim();
    if (!cleaned) {
      delete output[key];
      return;
    }
    const parsed = Number.parseFloat(cleaned);
    if (Number.isFinite(parsed)) {
      output[key] = parsed;
    } else {
      delete output[key];
    }
  };

  const setInt = (key: string, value: string) => {
    const cleaned = String(value || '').trim();
    if (!cleaned) {
      delete output[key];
      return;
    }
    const parsed = Number.parseInt(cleaned, 10);
    if (Number.isFinite(parsed)) {
      output[key] = parsed;
    } else {
      delete output[key];
    }
  };

  setText('model_type', row.model_type);
  setText('provider', row.provider);
  setText('base_url', row.base_url);
  setText('api_key', row.api_key);
  setText('model', row.model);
  setInt('timeout_s', row.timeout_s);
  setInt('retry', row.retry);
  setInt('max_context', row.max_context);

  if (row.model_type === 'llm') {
    setFloat('temperature', row.temperature);
    setInt('max_output', row.max_output);
    setText('tool_call_mode', row.tool_call_mode);
  } else {
    delete output.temperature;
    delete output.max_output;
    delete output.tool_call_mode;
  }

  return output;
};

const refreshRemoteConnected = () => {
  const override = getDesktopRemoteApiBaseOverride();
  remoteConnected.value = isDesktopRemoteAuthMode() && Boolean(override);
};

const applySettingsData = (data: Record<string, any>) => {
  const llm = data.llm || {};
  modelRows.value = parseModelRows((llm.models as Record<string, Record<string, unknown>>) || {});
  if (!modelRows.value.length) {
    addModel('llm');
  }

  defaultModel.value = findDefaultModelKeyByType(
    modelRows.value,
    'llm',
    String(llm.default || '').trim()
  );
  defaultEmbeddingModel.value = findDefaultModelKeyByType(
    modelRows.value,
    'embedding',
    readDefaultEmbeddingModel()
  );

  ensureSelectedModel();
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
    models[key] = buildModelPayload(row);
  }

  const currentDefaultModel = findDefaultModelKeyByType(
    modelRows.value,
    'llm',
    defaultModel.value.trim() || Object.keys(models)[0] || ''
  );
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

  const currentDefaultEmbedding = findDefaultModelKeyByType(
    modelRows.value,
    'embedding',
    defaultEmbeddingModel.value.trim()
  );
  if (embeddingModelRows.value.length > 0 && !currentDefaultEmbedding) {
    ElMessage.warning(t('desktop.system.defaultEmbeddingModelRequired'));
    return;
  }
  if (currentDefaultEmbedding && !models[currentDefaultEmbedding]) {
    ElMessage.warning(t('desktop.system.defaultEmbeddingModelMissing'));
    return;
  }

  savingModel.value = true;
  try {
    const response = await updateDesktopSettings({
      llm: {
        default: currentDefaultModel,
        models
      }
    });
    const data = (response?.data?.data || {}) as Record<string, any>;
    writeDefaultEmbeddingModel(currentDefaultEmbedding);
    applySettingsData(data);
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

.desktop-system-settings-layout {
  display: grid;
  grid-template-columns: minmax(220px, 300px) minmax(0, 1fr);
  gap: 12px;
  min-height: 0;
}

.desktop-system-settings-model-list-wrap {
  border: 1px solid var(--portal-border);
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 10px;
  display: grid;
  gap: 10px;
  align-content: start;
  min-height: 0;
}

.desktop-system-settings-model-list-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
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

.desktop-system-settings-detail {
  display: grid;
  gap: 10px;
  min-width: 0;
}

.desktop-system-settings-detail-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.desktop-system-settings-detail-title {
  font-size: 14px;
  font-weight: 700;
  color: var(--portal-text);
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

.desktop-system-settings-field--full {
  grid-column: 1 / -1;
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
  gap: 8px;
  max-height: 520px;
  overflow: auto;
  padding-right: 2px;
}

.desktop-system-settings-model-item {
  border: 1px solid var(--portal-border);
  border-radius: 10px;
  background: var(--portal-surface, rgba(255, 255, 255, 0.9));
  padding: 10px;
  display: grid;
  gap: 6px;
  cursor: pointer;
  text-align: left;
  transition: border-color 0.16s ease, background-color 0.16s ease, transform 0.16s ease;
}

.desktop-system-settings-model-item:hover {
  border-color: rgba(var(--ui-accent-rgb), 0.36);
  background: var(--ui-accent-soft-2);
  transform: translateY(-1px);
}

.desktop-system-settings-model-item.active {
  border-color: rgba(var(--ui-accent-rgb), 0.52);
  background: var(--ui-accent-soft-2);
}

.desktop-system-settings-model-item-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.desktop-system-settings-model-item-name {
  font-size: 12px;
  color: var(--portal-text);
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-system-settings-model-item-type {
  font-size: 11px;
  color: var(--portal-muted);
  border: 1px solid var(--portal-border);
  border-radius: 999px;
  padding: 1px 8px;
  flex-shrink: 0;
}

.desktop-system-settings-model-item-meta {
  font-size: 11px;
  color: var(--portal-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-system-settings-model-item-badges {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.desktop-system-settings-badge {
  font-size: 10px;
  color: var(--ui-accent-deep);
  border: 1px solid rgba(var(--ui-accent-rgb), 0.4);
  border-radius: 999px;
  background: var(--ui-accent-soft-2);
  padding: 1px 7px;
}

.desktop-system-settings-badge--alt {
  color: #4f46e5;
  border-color: rgba(79, 70, 229, 0.35);
  background: rgba(79, 70, 229, 0.08);
}

.desktop-system-settings-model-grid {
  display: grid;
  gap: 10px;
  grid-template-columns: repeat(2, minmax(0, 1fr));
}

.desktop-system-settings-empty,
.desktop-system-settings-empty-panel {
  border: 1px dashed var(--portal-border);
  border-radius: 10px;
  color: var(--portal-muted);
  font-size: 12px;
  text-align: center;
  padding: 16px 10px;
}

.desktop-system-settings-empty-panel {
  background: var(--portal-panel);
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

@media (max-width: 1100px) {
  .desktop-system-settings-layout {
    grid-template-columns: 1fr;
  }

  .desktop-system-settings-model-list {
    max-height: 260px;
  }
}

@media (max-width: 900px) {
  .desktop-system-settings-form-grid,
  .desktop-system-settings-model-grid {
    grid-template-columns: 1fr;
  }
}
</style>
