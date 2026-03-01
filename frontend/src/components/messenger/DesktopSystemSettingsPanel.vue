<template>
  <section
    v-if="showModelPanel"
    class="messenger-settings-card desktop-system-settings-panel desktop-system-settings-panel--llm"
    v-loading="loading"
  >
    <div class="desktop-system-settings-section">
      <div class="desktop-system-settings-form-grid">
        <label class="desktop-system-settings-field">
          <span class="desktop-system-settings-field-label">{{ t('desktop.system.defaultChatModel') }}</span>
          <el-select
            v-model="defaultModel"
            class="desktop-system-settings-input"
            filterable
            allow-create
            popper-class="desktop-system-settings-popper"
          >
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
            popper-class="desktop-system-settings-popper"
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
          <div class="desktop-system-settings-model-list-head-actions">
            <el-button class="desktop-system-settings-btn" size="small" @click="addModel()">
              {{ t('desktop.system.modelAdd') }}
            </el-button>
            <el-button
              class="desktop-system-settings-btn desktop-system-settings-btn--primary"
              size="small"
              :loading="savingModel"
              @click="saveModelSettings"
            >
              {{ t('desktop.common.save') }}
            </el-button>
          </div>
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
            <el-button class="desktop-system-settings-btn" size="small" @click="setCurrentAsDefault">
              {{ setCurrentDefaultLabel }}
            </el-button>
            <el-button
              class="desktop-system-settings-btn desktop-system-settings-btn--danger"
              size="small"
              @click="removeModel(selectedModel)"
            >
              {{ t('desktop.common.remove') }}
            </el-button>
          </div>
        </div>

        <div class="desktop-system-settings-section">
          <div class="desktop-system-settings-section-head">
            <div class="desktop-system-settings-section-title">
              <i class="fa-solid fa-gear" aria-hidden="true"></i>
              <span>{{ t('desktop.system.section.basic') }}</span>
            </div>
          </div>
          <div class="desktop-system-settings-model-grid">
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.modelKey') }}</span>
              <el-input v-model="selectedModel.key" />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.modelType') }}</span>
              <el-select
                v-model="selectedModel.model_type"
                class="desktop-system-settings-input"
                popper-class="desktop-system-settings-popper"
              >
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
          <div class="desktop-system-settings-section-head">
            <div class="desktop-system-settings-section-title">
              <i class="fa-solid fa-sliders" aria-hidden="true"></i>
              <span>{{ t('desktop.system.section.generation') }}</span>
            </div>
          </div>
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
            <label v-if="selectedModel.model_type === 'llm'" class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.maxRounds') }}</span>
              <el-input v-model="selectedModel.max_rounds" />
            </label>
            <label v-if="selectedModel.model_type === 'llm'" class="desktop-system-settings-field desktop-system-settings-field--full">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.maxContext') }}</span>
              <div class="desktop-system-settings-inline">
                <el-input v-model="selectedModel.max_context" />
                <el-button
                  class="desktop-system-settings-btn"
                  :loading="probingContext"
                  @click="probeMaxContext"
                >
                  {{ t('desktop.system.maxContextProbe') }}
                </el-button>
              </div>
            </label>
          </div>
        </div>

        <div class="desktop-system-settings-section">
          <div class="desktop-system-settings-section-head">
            <div class="desktop-system-settings-section-title">
              <i class="fa-solid fa-wand-magic-sparkles" aria-hidden="true"></i>
              <span>{{ t('desktop.system.section.capabilities') }}</span>
            </div>
          </div>
          <div v-if="selectedModel.model_type === 'llm'" class="desktop-system-settings-model-grid">
            <div class="desktop-system-settings-field desktop-system-settings-field--full">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.capabilityToggle') }}</span>
              <div class="desktop-system-settings-checkbox-group">
                <label class="desktop-system-settings-checkbox">
                  <input v-model="selectedModel.support_vision" type="checkbox" />
                  <span>{{ t('desktop.system.supportVision') }}</span>
                </label>
                <label class="desktop-system-settings-checkbox">
                  <input v-model="selectedModel.stream_include_usage" type="checkbox" />
                  <span>{{ t('desktop.system.streamIncludeUsage') }}</span>
                </label>
              </div>
            </div>
            <label class="desktop-system-settings-field desktop-system-settings-field--full">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.toolCallMode') }}</span>
              <el-select
                v-model="selectedModel.tool_call_mode"
                class="desktop-system-settings-input"
                popper-class="desktop-system-settings-popper"
              >
                <el-option label="tool_call" value="tool_call" />
                <el-option label="function_call" value="function_call" />
              </el-select>
            </label>
          </div>
          <div v-else class="desktop-system-settings-section-empty">
            {{ t('desktop.system.sectionLlmOnly') }}
          </div>
        </div>

        <div class="desktop-system-settings-section">
          <div class="desktop-system-settings-section-head">
            <div class="desktop-system-settings-section-title">
              <i class="fa-solid fa-compress" aria-hidden="true"></i>
              <span>{{ t('desktop.system.section.compaction') }}</span>
            </div>
          </div>
          <div v-if="selectedModel.model_type === 'llm'" class="desktop-system-settings-model-grid">
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.historyCompactionRatio') }}</span>
              <el-input v-model="selectedModel.history_compaction_ratio" />
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.historyCompactionReset') }}</span>
              <el-select
                v-model="selectedModel.history_compaction_reset"
                class="desktop-system-settings-input"
                popper-class="desktop-system-settings-popper"
              >
                <el-option :label="t('desktop.system.compactionReset.zero')" value="zero" />
                <el-option :label="t('desktop.system.compactionReset.current')" value="current" />
                <el-option :label="t('desktop.system.compactionReset.keep')" value="keep" />
              </el-select>
            </label>
          </div>
          <div v-else class="desktop-system-settings-section-empty">
            {{ t('desktop.system.sectionLlmOnly') }}
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
        <el-button class="desktop-system-settings-btn desktop-system-settings-btn--primary" :loading="connectingRemote" @click="connectRemoteServer">
          {{ t('desktop.system.remote.connect') }}
        </el-button>
        <el-button class="desktop-system-settings-btn" :disabled="!remoteConnected || connectingRemote" @click="disconnectRemoteServer">
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

import {
  fetchDesktopSettings,
  probeDesktopLlmContextWindow,
  updateDesktopSettings,
  type DesktopRemoteGatewaySettings
} from '@/api/desktop';
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
type HistoryCompactionReset = 'zero' | 'current' | 'keep';
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
  max_rounds: string;
  max_output: string;
  max_context: string;
  support_vision: boolean;
  stream_include_usage: boolean;
  tool_call_mode: ToolCallMode;
  history_compaction_ratio: string;
  history_compaction_reset: HistoryCompactionReset;
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
const probingContext = ref(false);
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
const setCurrentDefaultLabel = computed(() => {
  const current = selectedModel.value;
  if (!current) return t('desktop.system.setDefaultChatModel');
  return normalizeModelType(current.model_type) === 'embedding'
    ? t('desktop.system.setDefaultEmbeddingModel')
    : t('desktop.system.setDefaultChatModel');
});

const normalizeModelType = (value: unknown): ModelType => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'embedding' || raw === 'embed' || raw === 'embeddings') {
    return 'embedding';
  }
  return 'llm';
};

const normalizeToolCallMode = (value: unknown): ToolCallMode =>
  String(value || '').trim().toLowerCase() === 'function_call' ? 'function_call' : 'tool_call';

const normalizeHistoryCompactionReset = (value: unknown): HistoryCompactionReset => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'current') return 'current';
  if (raw === 'keep') return 'keep';
  return 'zero';
};

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
    max_rounds: raw.max_rounds == null ? '10' : String(raw.max_rounds),
    max_output: raw.max_output == null ? '' : String(raw.max_output),
    max_context: raw.max_context == null ? '' : String(raw.max_context),
    support_vision: raw.support_vision === true,
    stream_include_usage: raw.stream_include_usage !== false,
    tool_call_mode: normalizeToolCallMode(raw.tool_call_mode),
    history_compaction_ratio:
      raw.history_compaction_ratio == null ? '0.8' : String(raw.history_compaction_ratio),
    history_compaction_reset: normalizeHistoryCompactionReset(raw.history_compaction_reset),
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
    max_rounds: modelType === 'llm' ? '10' : '',
    max_output: '',
    max_context: '',
    support_vision: false,
    stream_include_usage: true,
    tool_call_mode: 'tool_call',
    history_compaction_ratio: modelType === 'llm' ? '0.8' : '',
    history_compaction_reset: 'zero',
    raw: {}
  };
  modelRows.value.push(row);
  selectedModelUid.value = row.uid;
};

const selectModel = (uid: string) => {
  selectedModelUid.value = uid;
};

const setCurrentAsDefault = () => {
  const current = selectedModel.value;
  if (!current) return;
  const key = current.key.trim();
  if (!key) {
    ElMessage.warning(t('desktop.system.modelKeyRequired'));
    return;
  }
  const modelType = normalizeModelType(current.model_type);
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

  if (row.model_type === 'llm') {
    setFloat('temperature', row.temperature);
    setInt('max_rounds', row.max_rounds);
    setInt('max_output', row.max_output);
    setInt('max_context', row.max_context);
    output.support_vision = row.support_vision === true;
    output.stream_include_usage = row.stream_include_usage !== false;
    setText('tool_call_mode', row.tool_call_mode);
    setFloat('history_compaction_ratio', row.history_compaction_ratio);
    setText('history_compaction_reset', normalizeHistoryCompactionReset(row.history_compaction_reset));
  } else {
    delete output.temperature;
    delete output.max_rounds;
    delete output.max_output;
    delete output.max_context;
    delete output.support_vision;
    delete output.stream_include_usage;
    delete output.tool_call_mode;
    delete output.history_compaction_ratio;
    delete output.history_compaction_reset;
  }

  return output;
};

const probeMaxContext = async () => {
  const current = selectedModel.value;
  if (!current || normalizeModelType(current.model_type) !== 'llm') return;
  const modelName = current.model.trim();
  if (!modelName) {
    ElMessage.warning(t('desktop.system.modelNameRequired'));
    return;
  }
  const targetUid = current.uid;
  probingContext.value = true;
  try {
    const response = await probeDesktopLlmContextWindow({
      provider: String(current.provider || '').trim(),
      base_url: String(current.base_url || '').trim(),
      api_key: String(current.api_key || '').trim(),
      model: modelName,
      timeout_s: 15
    });
    const payload = (response?.data || {}) as Record<string, unknown>;
    const maxContext = Number(payload.max_context);
    const message = String(payload.message || '').trim();
    const latest = selectedModel.value;
    if (!latest || latest.uid !== targetUid) return;
    if (Number.isFinite(maxContext) && maxContext > 0) {
      latest.max_context = String(Math.round(maxContext));
      ElMessage.success(t('desktop.system.maxContextProbeSuccess', { value: latest.max_context }));
      return;
    }
    if (message) {
      ElMessage.info(message);
      return;
    }
    ElMessage.info(t('desktop.system.maxContextProbeNoResult'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.system.maxContextProbeFailed'));
  } finally {
    probingContext.value = false;
  }
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

.desktop-system-settings-model-list-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.desktop-system-settings-section {
  border: 1px solid var(--portal-border);
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 12px;
  display: grid;
  gap: 12px;
}

.desktop-system-settings-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.desktop-system-settings-section-title {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  font-weight: 600;
  color: var(--portal-text);
}

.desktop-system-settings-section-title i {
  color: var(--portal-muted);
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

.desktop-system-settings-btn {
  border-radius: 9px;
  border: 1px solid #d8dce4;
  background: #ffffff;
  color: #4b5563;
  font-weight: 600;
  box-shadow: none;
  transition: border-color 0.15s ease, color 0.15s ease, background-color 0.15s ease;
}

.desktop-system-settings-btn:hover:not(:disabled) {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
  color: var(--ui-accent-deep);
  background: var(--ui-accent-soft-2);
}

.desktop-system-settings-btn--primary {
  border-color: transparent;
  background: var(--ui-accent);
  color: #ffffff;
}

.desktop-system-settings-btn--primary:hover:not(:disabled) {
  border-color: transparent;
  background: var(--ui-accent-hover);
  color: #ffffff;
}

.desktop-system-settings-btn--danger {
  border-color: rgba(185, 28, 28, 0.22);
  color: #b91c1c;
  background: #fff4f4;
}

.desktop-system-settings-btn--danger:hover:not(:disabled) {
  border-color: rgba(185, 28, 28, 0.38);
  background: #ffe9e9;
  color: #991b1b;
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

.desktop-system-settings-inline {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 8px;
  align-items: center;
}

.desktop-system-settings-checkbox-group {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}

.desktop-system-settings-checkbox {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--portal-text);
  font-size: 12px;
}

.desktop-system-settings-section-empty {
  color: var(--portal-muted);
  font-size: 12px;
  padding: 2px 0;
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
  background: #ffffff;
  border: 1px solid #d8dce4;
  box-shadow: none;
  border-radius: 10px;
  min-height: 36px;
  transition: border-color 0.15s ease, box-shadow 0.15s ease, background-color 0.15s ease;
}

.desktop-system-settings-panel :deep(.el-input__wrapper:hover),
.desktop-system-settings-panel :deep(.el-select__wrapper:hover) {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
  box-shadow: none;
}

.desktop-system-settings-panel :deep(.el-input__wrapper.is-focus),
.desktop-system-settings-panel :deep(.el-select__wrapper.is-focused) {
  border-color: rgba(var(--ui-accent-rgb), 0.62);
  box-shadow: 0 0 0 2px rgba(var(--ui-accent-rgb), 0.14);
}

.desktop-system-settings-panel :deep(.el-input__inner),
.desktop-system-settings-panel :deep(.el-select__selected-item),
.desktop-system-settings-panel :deep(.el-select__placeholder) {
  color: #374151;
}

.desktop-system-settings-panel :deep(.desktop-system-settings-popper.el-select__popper.el-popper) {
  border: 1px solid #d8dce4;
  border-radius: 10px;
  box-shadow: 0 12px 24px rgba(15, 23, 42, 0.12);
}

.desktop-system-settings-panel :deep(.desktop-system-settings-popper .el-select-dropdown__item) {
  color: #374151;
  font-size: 12px;
}

.desktop-system-settings-panel :deep(.desktop-system-settings-popper .el-select-dropdown__item.hover),
.desktop-system-settings-panel :deep(.desktop-system-settings-popper .el-select-dropdown__item:hover) {
  background: var(--ui-accent-soft-2);
  color: var(--ui-accent-deep);
}

.desktop-system-settings-panel :deep(.desktop-system-settings-popper .el-select-dropdown__item.is-selected) {
  color: var(--ui-accent-deep);
  font-weight: 600;
  background: rgba(var(--ui-accent-rgb), 0.1);
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

  .desktop-system-settings-inline {
    grid-template-columns: 1fr;
  }
}
</style>
