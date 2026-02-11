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
                  <el-table-column :label="t('desktop.system.modelKey')" width="180">
                    <template #default="{ row }">
                      <el-input v-model="row.key" />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.system.provider')" width="150">
                    <template #default="{ row }">
                      <el-input v-model="row.provider" />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.system.baseUrl')" min-width="220">
                    <template #default="{ row }">
                      <el-input v-model="row.base_url" />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.system.apiKey')" width="180">
                    <template #default="{ row }">
                      <el-input v-model="row.api_key" show-password />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.system.modelName')" min-width="180">
                    <template #default="{ row }">
                      <el-input v-model="row.model" />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.system.maxContext')" width="120">
                    <template #default="{ row }">
                      <el-input-number v-model="row.max_context" :min="1" :step="1024" controls-position="right" />
                    </template>
                  </el-table-column>
                  <el-table-column :label="t('desktop.system.toolCallMode')" width="160">
                    <template #default="{ row }">
                      <el-select v-model="row.tool_call_mode" class="desktop-full-width">
                        <el-option label="tool_call" value="tool_call" />
                        <el-option label="function_call" value="function_call" />
                      </el-select>
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
                  <span>{{ t('desktop.system.remote.title') }}</span>
                </template>
                <el-form label-position="top" class="desktop-form-grid">
                  <el-form-item :label="t('desktop.system.remote.enabled')">
                    <el-switch v-model="remoteGateway.enabled" />
                  </el-form-item>
                  <el-form-item :label="t('desktop.system.remote.serverBaseUrl')">
                    <el-input v-model="remoteGateway.server_base_url" :placeholder="t('desktop.system.remote.serverPlaceholder')" />
                  </el-form-item>
                  <el-form-item :label="t('desktop.system.remote.apiKey')">
                    <el-input v-model="remoteGateway.api_key" show-password />
                  </el-form-item>
                  <el-form-item :label="t('desktop.system.remote.roleName')">
                    <el-input v-model="remoteGateway.role_name" :placeholder="t('desktop.system.remote.rolePlaceholder')" />
                  </el-form-item>
                  <el-form-item :label="t('desktop.system.remote.useRemoteSandbox')">
                    <el-switch v-model="remoteGateway.use_remote_sandbox" />
                  </el-form-item>
                </el-form>
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
import { onMounted, reactive, ref } from 'vue';
import { ElMessage } from 'element-plus';
import { useRouter } from 'vue-router';

import {
  fetchDesktopSettings,
  updateDesktopSettings,
  type DesktopRemoteGatewaySettings
} from '@/api/desktop';
import { useI18n, getLanguageLabel, setLanguage } from '@/i18n';
import { setDesktopToolCallMode } from '@/config/desktop';
import UserTopbar from '@/components/user/UserTopbar.vue';

type ToolCallMode = 'tool_call' | 'function_call';

type ModelRow = {
  key: string;
  provider: string;
  base_url: string;
  api_key: string;
  model: string;
  max_context: number | null;
  tool_call_mode: ToolCallMode;
  raw: Record<string, unknown>;
};

const normalizeToolCallMode = (value: unknown): ToolCallMode =>
  String(value || '').trim().toLowerCase() === 'function_call' ? 'function_call' : 'tool_call';

const toNullableNumber = (value: unknown): number | null => {
  if (value === null || value === undefined || value === '') {
    return null;
  }
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return null;
  }
  return Math.floor(parsed);
};

const parseModelRows = (models: Record<string, Record<string, unknown>>): ModelRow[] =>
  Object.entries(models || {}).map(([key, raw]) => ({
    key,
    provider: String(raw.provider || ''),
    base_url: String(raw.base_url || ''),
    api_key: String(raw.api_key || ''),
    model: String(raw.model || ''),
    max_context: toNullableNumber(raw.max_context),
    tool_call_mode: normalizeToolCallMode(raw.tool_call_mode),
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

  const setNumber = (key: string, value: number | null) => {
    if (value === null || value === undefined) {
      delete output[key];
    } else {
      output[key] = value;
    }
  };

  setText('provider', row.provider);
  setText('base_url', row.base_url);
  setText('api_key', row.api_key);
  setText('model', row.model);
  setNumber('max_context', row.max_context);
  output.tool_call_mode = row.tool_call_mode;

  return output;
};

const { t } = useI18n();
const router = useRouter();

const loading = ref(false);
const saving = ref(false);
const language = ref('zh-CN');
const supportedLanguages = ref<string[]>(['zh-CN', 'en-US']);
const defaultModel = ref('');
const modelRows = ref<ModelRow[]>([]);
const remoteGateway = reactive<DesktopRemoteGatewaySettings>({
  enabled: false,
  server_base_url: '',
  api_key: '',
  role_name: '',
  use_remote_sandbox: false
});

const goBackToSettings = () => {
  router.push('/desktop/settings');
};

const addModel = () => {
  modelRows.value.push({
    key: '',
    provider: '',
    base_url: '',
    api_key: '',
    model: '',
    max_context: null,
    tool_call_mode: 'tool_call',
    raw: {}
  });
};

const removeModel = (target: ModelRow) => {
  modelRows.value = modelRows.value.filter((item) => item !== target);
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

    if (defaultModel.value && !modelRows.value.some((item) => item.key === defaultModel.value)) {
      modelRows.value.unshift({
        key: defaultModel.value,
        provider: '',
        base_url: '',
        api_key: '',
        model: '',
        max_context: null,
        tool_call_mode: 'tool_call',
        raw: {}
      });
    }

    Object.assign(remoteGateway, {
      enabled: Boolean(data.remote_gateway?.enabled),
      server_base_url: String(data.remote_gateway?.server_base_url || ''),
      api_key: String(data.remote_gateway?.api_key || ''),
      role_name: String(data.remote_gateway?.role_name || ''),
      use_remote_sandbox: Boolean(data.remote_gateway?.use_remote_sandbox)
    });
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

  const currentDefaultModel = defaultModel.value.trim();
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
      },
      remote_gateway: {
        enabled: Boolean(remoteGateway.enabled),
        server_base_url: String(remoteGateway.server_base_url || '').trim(),
        api_key: String(remoteGateway.api_key || '').trim(),
        role_name: String(remoteGateway.role_name || '').trim(),
        use_remote_sandbox: Boolean(remoteGateway.use_remote_sandbox)
      }
    });

    const defaultRow = modelRows.value.find((item) => item.key.trim() === currentDefaultModel);
    if (defaultRow) {
      setDesktopToolCallMode(defaultRow.tool_call_mode);
    }
    setLanguage(selectedLanguage, { force: true });

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

.desktop-settings-hint {
  margin: 12px 0 0;
  font-size: 12px;
  color: var(--dark-muted);
}

.desktop-settings-footer {
  display: flex;
  justify-content: flex-end;
}

:root[data-user-theme='light'] .desktop-settings-hint {
  color: #64748b;
}
</style>
