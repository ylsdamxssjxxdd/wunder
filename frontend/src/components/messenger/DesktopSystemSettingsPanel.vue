<template>
  <section
    v-if="showModelPanel"
    class="messenger-settings-card desktop-system-settings-panel desktop-system-settings-panel--llm"
    v-loading="loading"
  >
    <section class="desktop-system-settings-section">
      <div class="desktop-system-settings-section-head">
        <div class="desktop-system-settings-section-title">
          <i class="fa-solid fa-terminal" aria-hidden="true"></i>
          <span>{{ t('desktop.system.pythonInterpreterTitle') }}</span>
        </div>
        <div class="desktop-system-settings-actions">
          <el-button
            class="desktop-system-settings-btn"
            size="small"
            :disabled="!pythonInterpreterPath.trim()"
            :loading="savingRuntime"
            @click="resetPythonInterpreterPath"
          >
            {{ t('desktop.system.pythonInterpreterReset') }}
          </el-button>
          <el-button
            class="desktop-system-settings-btn desktop-system-settings-btn--primary"
            size="small"
            :loading="savingRuntime"
            @click="saveRuntimeSettings"
          >
            {{ t('desktop.common.save') }}
          </el-button>
        </div>
      </div>
      <label class="desktop-system-settings-field desktop-system-settings-field--full">
        <span class="desktop-system-settings-field-label">
          {{ t('desktop.system.pythonInterpreterPath') }}
        </span>
        <div class="desktop-system-settings-runtime-input">
          <el-input
            v-model="pythonInterpreterPath"
            clearable
            :placeholder="t('desktop.system.pythonInterpreterPathPlaceholder')"
          />
          <el-button class="desktop-system-settings-btn" @click="openPythonPathPicker">
            {{ t('desktop.common.browse') }}
          </el-button>
          <el-button
            class="desktop-system-settings-btn"
            :loading="loadingPythonCandidates"
            @click="loadPythonInterpreterCandidates(true)"
          >
            {{ t('desktop.system.pythonInterpreterDetect') }}
          </el-button>
        </div>
      </label>
      <div class="desktop-system-settings-runtime-hint">
        {{
          pythonInterpreterPath.trim()
            ? t('desktop.system.pythonInterpreterCustomHint')
            : t('desktop.system.pythonInterpreterBundledHint')
        }}
      </div>
      <div class="desktop-system-settings-runtime-subhint">
        {{ t('desktop.system.pythonInterpreterHint') }}
      </div>
      <div v-if="pythonInterpreterCandidates.length" class="desktop-system-settings-runtime-candidates">
        <div class="desktop-system-settings-runtime-candidates-title">
          {{ t('desktop.system.pythonInterpreterCandidates') }}
        </div>
        <div
          v-for="item in pythonInterpreterCandidates"
          :key="`python-candidate-${item.path}`"
          class="desktop-system-settings-runtime-candidate"
        >
          <div class="desktop-system-settings-runtime-candidate-main">
            <div class="desktop-system-settings-runtime-candidate-path" :title="item.path">
              {{ item.path }}
            </div>
            <div class="desktop-system-settings-runtime-candidate-meta">
              {{ formatPythonCandidateSource(item.source) }}
            </div>
          </div>
          <el-button class="desktop-system-settings-btn" size="small" @click="useDetectedPythonInterpreter(item.path)">
            {{ t('common.use') }}
          </el-button>
        </div>
      </div>
    </section>
    <div class="desktop-system-settings-layout">
      <aside class="desktop-system-settings-model-list-wrap">
        <div class="desktop-system-settings-model-list-head">
          <span class="desktop-system-settings-model-list-title">{{ t('desktop.system.modelsTitle') }}</span>
          <div class="desktop-system-settings-model-list-head-actions">
            <el-button class="desktop-system-settings-btn" size="small" @click="addModel()">
              {{ t('desktop.system.modelAdd') }}
            </el-button>
          </div>
        </div>
        <div class="desktop-system-settings-model-list">
          <button
            v-for="row in modelRowsForList"
            :key="row.uid"
            class="desktop-system-settings-model-item"
            :class="{ active: selectedModelUid === row.uid }"
            type="button"
            @click="selectModel(row.uid)"
          >
            <div class="desktop-system-settings-model-item-head">
              <span class="desktop-system-settings-model-item-name">
                <i
                  v-if="isDefaultModelRow(row)"
                  class="fa-solid fa-star desktop-system-settings-model-item-star"
                  aria-hidden="true"
                ></i>
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
            <el-button
              class="desktop-system-settings-btn"
              size="small"
              :loading="savingModel"
              @click="setCurrentAsDefault"
            >
              {{ setCurrentDefaultLabel }}
            </el-button>
            <el-button
              class="desktop-system-settings-btn desktop-system-settings-btn--primary"
              size="small"
              :loading="savingModel"
              @click="saveModelSettings"
            >
              {{ t('desktop.common.save') }}
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

        <div class="desktop-system-settings-group">
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
              <el-select
                v-model="selectedModel.provider"
                class="desktop-system-settings-input"
                popper-class="desktop-system-settings-popper"
                @change="handleProviderChange"
              >
                <el-option
                  v-for="provider in providerOptionsForSelectedModel"
                  :key="provider.id"
                  :label="provider.label"
                  :value="provider.id"
                />
              </el-select>
            </label>
            <label class="desktop-system-settings-field">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.modelName') }}</span>
              <el-input
                v-if="selectedProviderUsesManualModelInput"
                v-model="selectedModel.model"
                class="desktop-system-settings-input"
                :placeholder="t('desktop.system.modelNamePlaceholder')"
                @input="handleModelInput"
                @blur="handleModelBlur"
              />
              <el-autocomplete
                v-else
                v-model="selectedModel.model"
                class="desktop-system-settings-input"
                popper-class="desktop-system-settings-popper"
                :fetch-suggestions="queryModelSuggestions"
                :placeholder="t('desktop.system.modelNamePlaceholder')"
                :trigger-on-focus="false"
                clearable
                @input="handleModelInput"
                @select="handleModelSuggestionSelect"
                @blur="handleModelBlur"
              />
            </label>
            <label class="desktop-system-settings-field desktop-system-settings-field--full">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.baseUrl') }}</span>
              <el-input
                v-model="selectedModel.base_url"
                :placeholder="modelBaseUrlPlaceholder"
              />
            </label>
            <label class="desktop-system-settings-field desktop-system-settings-field--full">
              <span class="desktop-system-settings-field-label">{{ t('desktop.system.apiKey') }}</span>
              <el-input v-model="selectedModel.api_key" show-password />
            </label>
          </div>
        </div>

        <div class="desktop-system-settings-group">
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

        <div class="desktop-system-settings-group">
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
                  <input v-model="selectedModel.support_hearing" type="checkbox" />
                  <span>{{ t('desktop.system.supportHearing') }}</span>
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
                <el-option label="freeform_call" value="freeform_call" />
              </el-select>
            </label>
          </div>
          <div v-else class="desktop-system-settings-section-empty">
            {{ t('desktop.system.sectionLlmOnly') }}
          </div>
        </div>

        <div class="desktop-system-settings-group">
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

  <el-dialog
    v-model="pythonPathPickerVisible"
    :title="t('desktop.system.pythonPathPickerTitle')"
    width="720px"
    append-to-body
  >
    <div class="desktop-system-settings-path-picker">
      <div class="desktop-system-settings-path-picker-toolbar">
        <el-button
          size="small"
          :disabled="!pythonPathPickerParentPath"
          @click="loadPythonPickerDirectory(pythonPathPickerParentPath || undefined)"
        >
          {{ t('desktop.system.pythonPathPickerUp') }}
        </el-button>
      </div>
      <div class="desktop-system-settings-path-picker-current" :title="pythonPathPickerCurrentPath">
        {{ pythonPathPickerCurrentPath }}
      </div>
      <div class="desktop-system-settings-path-picker-roots">
        <button
          v-for="root in pythonPathPickerRoots"
          :key="`python-path-root-${root}`"
          class="desktop-system-settings-path-picker-root"
          type="button"
          @click="loadPythonPickerDirectory(root)"
        >
          {{ root }}
        </button>
      </div>
      <div class="desktop-system-settings-path-picker-list" v-loading="pythonPathPickerLoading">
        <button
          v-for="item in pythonPathPickerItems"
          :key="`python-path-item-${item.path}`"
          class="desktop-system-settings-path-picker-item"
          type="button"
          @click="handlePythonPathPickerSelect(item)"
        >
          <i
            :class="
              item.entry_type === 'file' ? 'fa-brands fa-python' : 'fa-regular fa-folder'
            "
            aria-hidden="true"
          ></i>
          <span>{{ item.name }}</span>
        </button>
        <div
          v-if="!pythonPathPickerLoading && !pythonPathPickerItems.length"
          class="desktop-system-settings-path-picker-empty"
        >
          {{ t('desktop.system.pythonPathPickerEmpty') }}
        </div>
      </div>
    </div>
  </el-dialog>

  <section
    v-if="showRemotePanel"
    class="messenger-settings-card desktop-system-settings-panel desktop-system-settings-panel--stack"
  >
    <div class="desktop-system-settings-section">
      <div class="desktop-system-settings-status-line">
        <span class="desktop-system-settings-remote-state" :class="{ connected: remoteConnected }">
          {{
            remoteConnected
              ? t('desktop.system.remote.connected')
              : t('desktop.system.remote.disconnected')
          }}
        </span>
      </div>
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

  <section
    v-if="showLanPanel"
    class="messenger-settings-card desktop-system-settings-panel desktop-system-settings-panel--stack"
  >
    <div class="desktop-system-settings-section desktop-system-settings-form-grid">
      <div class="desktop-system-settings-status-line desktop-system-settings-field--full">
        <span class="desktop-system-settings-remote-state" :class="{ connected: lanMeshEnabled }">
          {{ lanMeshEnabled ? t('desktop.system.lan.enabled') : t('desktop.system.lan.disabled') }}
        </span>
      </div>
      <label class="desktop-system-settings-field">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.enabledSwitch') }}</span>
        <el-switch v-model="lanMeshEnabled" />
      </label>
      <label class="desktop-system-settings-field">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.peerId') }}</span>
        <el-input v-model="lanPeerId" />
      </label>
      <label class="desktop-system-settings-field">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.displayName') }}</span>
        <el-input v-model="lanDisplayName" />
      </label>
      <label class="desktop-system-settings-field">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.listenHost') }}</span>
        <el-input v-model="lanListenHost" />
      </label>
      <label class="desktop-system-settings-field">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.listenPort') }}</span>
        <el-input v-model="lanListenPort" />
      </label>
      <label class="desktop-system-settings-field">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.discoveryPort') }}</span>
        <el-input v-model="lanDiscoveryPort" />
      </label>
      <label class="desktop-system-settings-field desktop-system-settings-field--full">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.allowSubnets') }}</span>
        <el-input
          v-model="lanAllowSubnetsText"
          type="textarea"
          :autosize="{ minRows: 2, maxRows: 6 }"
          :placeholder="t('desktop.system.lan.cidrPlaceholder')"
        />
      </label>
      <label class="desktop-system-settings-field desktop-system-settings-field--full">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.denySubnets') }}</span>
        <el-input
          v-model="lanDenySubnetsText"
          type="textarea"
          :autosize="{ minRows: 2, maxRows: 6 }"
          :placeholder="t('desktop.system.lan.cidrPlaceholder')"
        />
      </label>
      <label class="desktop-system-settings-field desktop-system-settings-field--full">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.peerBlacklist') }}</span>
        <el-input
          v-model="lanPeerBlacklistText"
          type="textarea"
          :autosize="{ minRows: 2, maxRows: 6 }"
          :placeholder="t('desktop.system.lan.peerBlacklistPlaceholder')"
        />
      </label>
      <label class="desktop-system-settings-field desktop-system-settings-field--full">
        <span class="desktop-system-settings-field-label">{{ t('desktop.system.lan.sharedSecret') }}</span>
        <el-input v-model="lanSharedSecret" show-password />
      </label>
      <div class="desktop-system-settings-actions desktop-system-settings-field--full">
        <el-button
          class="desktop-system-settings-btn desktop-system-settings-btn--primary"
          :loading="savingLan"
          @click="saveLanSettings"
        >
          {{ t('desktop.common.save') }}
        </el-button>
        <el-button class="desktop-system-settings-btn" :loading="loadingLanPeers" @click="refreshLanPeers">
          {{ t('desktop.system.lan.refreshPeers') }}
        </el-button>
      </div>
    </div>

    <div class="desktop-system-settings-section">
      <div class="desktop-system-settings-section-title">
        <i class="fa-solid fa-sitemap" aria-hidden="true"></i>
        <span>{{ t('desktop.system.lan.peerListTitle') }}</span>
      </div>
      <div v-if="lanPeers.length" class="desktop-system-settings-model-list desktop-system-settings-peer-list">
        <div
          v-for="peer in lanPeers"
          :key="`${peer.peer_id || ''}-${peer.lan_ip || ''}-${peer.listen_port || ''}`"
          class="desktop-system-settings-model-item"
        >
          <div class="desktop-system-settings-model-item-head">
            <span class="desktop-system-settings-model-item-name">{{ peer.display_name || peer.user_id || peer.peer_id }}</span>
            <span class="desktop-system-settings-model-item-type">{{ peer.peer_id }}</span>
          </div>
          <div class="desktop-system-settings-model-item-meta">
            {{ peer.lan_ip }}:{{ peer.listen_port }}
          </div>
        </div>
      </div>
      <div v-else class="desktop-system-settings-empty">{{ t('desktop.system.lan.peerListEmpty') }}</div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { ElMessage } from 'element-plus';
import { useRouter } from 'vue-router';

import {
  detectDesktopPythonInterpreters,
  fetchDesktopSettings,
  listDesktopDirectories,
  listDesktopLanPeers,
  probeDesktopLlmContextWindow,
  updateDesktopSettings,
  type DesktopDirectoryEntry,
  type DesktopLanMeshSettings,
  type DesktopLanPeer,
  type DesktopPythonInterpreterItem,
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
import { resolveApiError } from '@/utils/apiError';
import {
  getProviderModelPresets,
  resolveProviderModelPresetMaxContext
} from '@/views/messenger/providerModelPresets';

type ModelType = 'llm' | 'embedding';
type ToolCallMode = 'tool_call' | 'function_call' | 'freeform_call';
type HistoryCompactionReset = 'zero' | 'current' | 'keep';
type SelectedModelPreference = {
  key: string;
  modelType: ModelType;
};
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
  support_hearing: boolean;
  stream_include_usage: boolean;
  tool_call_mode: ToolCallMode;
  history_compaction_ratio: string;
  history_compaction_reset: HistoryCompactionReset;
  raw: Record<string, unknown>;
};

const PYTHON_PICKER_FILE_NAMES = ['python.exe', 'python3.exe', 'python', 'python3'];

const EMBEDDING_DEFAULT_MODEL_STORAGE_KEY = 'wunder_desktop_default_embedding_model';

const props = withDefaults(
  defineProps<{
    panel?: 'models' | 'remote' | 'lan' | 'all';
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
const savingRuntime = ref(false);
const loadingPythonCandidates = ref(false);
const defaultModel = ref('');
const defaultEmbeddingModel = ref('');
const modelRows = ref<ModelRow[]>([]);
const selectedModelUid = ref('');
const remoteServerBaseUrl = ref('');
const remoteConnected = ref(false);
const pythonInterpreterPath = ref('');
const pythonInterpreterCandidates = ref<DesktopPythonInterpreterItem[]>([]);
const pythonPathPickerVisible = ref(false);
const pythonPathPickerLoading = ref(false);
const pythonPathPickerCurrentPath = ref('');
const pythonPathPickerParentPath = ref<string | null>(null);
const pythonPathPickerRoots = ref<string[]>([]);
const pythonPathPickerItems = ref<DesktopDirectoryEntry[]>([]);
const savingLan = ref(false);
const loadingLanPeers = ref(false);
const lanMeshEnabled = ref(false);
const lanPeerId = ref('');
const lanDisplayName = ref('');
const lanListenHost = ref('0.0.0.0');
const lanListenPort = ref('18661');
const lanDiscoveryPort = ref('18662');
const lanAllowSubnetsText = ref('');
const lanDenySubnetsText = ref('');
const lanPeerBlacklistText = ref('');
const lanSharedSecret = ref('');
const lanPeers = ref<DesktopLanPeer[]>([]);
let nextModelUid = 1;
const DEFAULT_PROVIDER_ID = 'openai_compatible';
const PROVIDER_PRESETS: Array<{ id: string; label: string; baseUrl: string }> = [
  { id: 'openai_compatible', label: 'openai_compatible', baseUrl: '' },
  { id: 'openai', label: 'openai', baseUrl: 'https://api.openai.com/v1' },
  { id: 'openrouter', label: 'openrouter', baseUrl: 'https://openrouter.ai/api/v1' },
  { id: 'siliconflow', label: 'siliconflow', baseUrl: 'https://api.siliconflow.cn/v1' },
  { id: 'deepseek', label: 'deepseek', baseUrl: 'https://api.deepseek.com' },
  { id: 'moonshot', label: 'moonshot', baseUrl: 'https://api.moonshot.ai/v1' },
  { id: 'qwen', label: 'qwen', baseUrl: 'https://dashscope.aliyuncs.com/compatible-mode/v1' },
  { id: 'groq', label: 'groq', baseUrl: 'https://api.groq.com/openai/v1' },
  { id: 'mistral', label: 'mistral', baseUrl: 'https://api.mistral.ai/v1' },
  { id: 'together', label: 'together', baseUrl: 'https://api.together.xyz/v1' },
  { id: 'ollama', label: 'ollama', baseUrl: 'http://127.0.0.1:11434/v1' },
  { id: 'lmstudio', label: 'lmstudio', baseUrl: 'http://127.0.0.1:1234/v1' }
];
const PROVIDER_PRESET_MAP = new Map(PROVIDER_PRESETS.map((item) => [item.id, item]));

const makeModelUid = (): string => `desktop-model-${nextModelUid++}`;

const showModelPanel = computed(() => props.panel === 'all' || props.panel === 'models');
const showRemotePanel = computed(() => props.panel === 'all' || props.panel === 'remote');
const showLanPanel = computed(() => props.panel === 'all' || props.panel === 'lan');

const embeddingModelRows = computed(() =>
  modelRows.value.filter((item) => item.model_type === 'embedding')
);
const isDefaultModelRow = (row: ModelRow): boolean => {
  const key = String(row?.key || '').trim();
  if (!key) return false;
  return key === defaultModel.value.trim() || key === defaultEmbeddingModel.value.trim();
};
const modelRowsForList = computed(() =>
  [...modelRows.value].sort((a, b) => {
    const keyA = String(a.key || '').trim();
    const keyB = String(b.key || '').trim();
    const rank = (key: string, modelType: ModelType): number => {
      if (!key) return 5;
      if (key === defaultModel.value.trim() && modelType === 'llm') return 0;
      if (key === defaultEmbeddingModel.value.trim() && modelType === 'embedding') return 1;
      return 3;
    };
    const rankDiff = rank(keyA, a.model_type) - rank(keyB, b.model_type);
    if (rankDiff !== 0) return rankDiff;
    return a.uid.localeCompare(b.uid);
  })
);
const selectedModel = computed(
  () => modelRows.value.find((item) => item.uid === selectedModelUid.value) || null
);
const selectedProviderUsesManualModelInput = computed(
  () => normalizeProviderId(selectedModel.value?.provider) === 'openai_compatible'
);
const modelOptionsForSelectedModel = computed(() => {
  const current = selectedModel.value;
  if (!current) return [];
  if (normalizeProviderId(current.provider) === 'openai_compatible') return [];

  const options: Array<{ value: string; label: string }> = [];
  const existing = new Set<string>();
  for (const preset of getProviderModelPresets(current.provider)) {
    const modelId = String(preset.id || '').trim();
    if (!modelId) continue;
    const normalized = modelId.toLowerCase();
    if (existing.has(normalized)) continue;
    existing.add(normalized);
    options.push({
      value: modelId,
      label: preset.label || modelId
    });
  }

  const currentModelId = String(current.model || '').trim();
  if (currentModelId) {
    const normalizedCurrent = currentModelId.toLowerCase();
    if (!existing.has(normalizedCurrent)) {
      options.unshift({
        value: currentModelId,
        label: currentModelId
      });
    }
  }

  return options;
});
const providerOptionsForSelectedModel = computed(() => {
  const currentProvider = normalizeProviderId(selectedModel.value?.provider);
  const options = PROVIDER_PRESETS.map((item) => ({
    id: item.id,
    label: item.label
  }));
  if (currentProvider && !PROVIDER_PRESET_MAP.has(currentProvider)) {
    options.unshift({ id: currentProvider, label: currentProvider });
  }
  return options;
});
const modelBaseUrlPlaceholder = computed(() => {
  const provider = selectedModel.value?.provider;
  return resolveProviderBaseUrl(provider) || t('desktop.system.baseUrlPlaceholder');
});
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

const normalizeProviderId = (value: unknown): string => {
  const raw = String(value || '').trim();
  if (!raw) {
    return DEFAULT_PROVIDER_ID;
  }
  const normalized = raw.toLowerCase().replace(/[\s-]+/g, '_');
  switch (normalized) {
    case 'openai_compat':
      return 'openai_compatible';
    case 'openai_native':
      return 'openai';
    case 'silicon_flow':
      return 'siliconflow';
    case 'kimi':
      return 'moonshot';
    case 'dashscope':
      return 'qwen';
    case 'lm_studio':
      return 'lmstudio';
    default:
      return normalized;
  }
};

const getProviderPreset = (provider: unknown) => PROVIDER_PRESET_MAP.get(normalizeProviderId(provider));

const resolveProviderBaseUrl = (provider: unknown): string => getProviderPreset(provider)?.baseUrl || '';

const resolveDefaultToolCallMode = (provider: unknown): ToolCallMode =>
  normalizeProviderId(provider) === 'openai' ? 'freeform_call' : 'function_call';

const normalizeToolCallMode = (value: unknown, provider?: unknown): ToolCallMode => {
  const normalized = String(value || '').trim().toLowerCase();
  if (!normalized) {
    return resolveDefaultToolCallMode(provider);
  }
  if (normalized === 'freeform_call' || normalized === 'freeform') return 'freeform_call';
  if (normalized === 'function_call') return 'function_call';
  if (normalized === 'tool_call') return 'tool_call';
  return resolveDefaultToolCallMode(provider);
};

const applyModelPresetContext = (row: ModelRow, force = true) => {
  if (!row || normalizeModelType(row.model_type) !== 'llm') return;
  if (!force && String(row.max_context || '').trim()) return;
  const maxContext = resolveProviderModelPresetMaxContext(row.provider, row.model);
  if (!Number.isFinite(maxContext) || Number(maxContext) <= 0) return;
  row.max_context = String(Math.round(Number(maxContext)));
};

const handleProviderChange = (value: string) => {
  const current = selectedModel.value;
  if (!current) return;
  const previousProvider = current.provider;
  const nextProvider = normalizeProviderId(value);
  current.provider = nextProvider;
  const prevDefault = resolveDefaultToolCallMode(previousProvider);
  const currentMode = normalizeToolCallMode(current.tool_call_mode, previousProvider);
  if (!current.tool_call_mode || currentMode === prevDefault) {
    current.tool_call_mode = resolveDefaultToolCallMode(nextProvider);
  }
  applyModelPresetContext(current);
};

const queryModelSuggestions = (
  queryString: string,
  callback: (items: Array<{ value: string }>) => void
) => {
  const keyword = String(queryString || '').trim().toLowerCase();
  const items = modelOptionsForSelectedModel.value
    .filter((option) => {
      if (!keyword) return true;
      return option.value.toLowerCase().includes(keyword) || option.label.toLowerCase().includes(keyword);
    })
    .map((option) => ({ value: option.value }));
  callback(items);
};

const handleModelInput = (value: string) => {
  const current = selectedModel.value;
  if (!current) return;
  current.model = String(value || '');
  applyModelPresetContext(current);
};

const handleModelBlur = (event: FocusEvent) => {
  const current = selectedModel.value;
  if (!current) return;
  const target = event.target as HTMLInputElement | null;
  current.model = String(target?.value ?? current.model ?? '').trim();
  applyModelPresetContext(current);
};

const handleModelSuggestionSelect = (item: { value?: string }) => {
  const current = selectedModel.value;
  if (!current) return;
  current.model = String(item?.value || '').trim();
  applyModelPresetContext(current);
};

const normalizeHistoryCompactionReset = (value: unknown): HistoryCompactionReset => {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === 'current') return 'current';
  if (raw === 'keep') return 'keep';
  return 'zero';
};

const FLOAT_INPUT_PRECISION = 7;

const roundFloat = (value: number): number => {
  const factor = 10 ** FLOAT_INPUT_PRECISION;
  return Math.round(value * factor) / factor;
};

const trimTrailingZeros = (valueText: string): string => {
  if (!valueText.includes('.')) {
    return valueText;
  }
  const trimmed = valueText.replace(/(?:\.0+|(\.\d*?[1-9])0+)$/, '$1').replace(/\.$/, '');
  return trimmed === '-0' ? '0' : trimmed;
};

const formatFloatForInput = (value: unknown, fallback: number): string => {
  const numeric = typeof value === 'number' ? value : Number.parseFloat(String(value ?? ''));
  const resolved = Number.isFinite(numeric) ? numeric : fallback;
  if (!Number.isFinite(resolved)) {
    return '';
  }
  return trimTrailingZeros(roundFloat(resolved).toFixed(FLOAT_INPUT_PRECISION));
};

const parseModelRows = (models: Record<string, Record<string, unknown>>): ModelRow[] =>
  Object.entries(models || {}).map(([key, raw]) => ({
    uid: makeModelUid(),
    key,
    model_type: normalizeModelType(raw.model_type),
    provider: normalizeProviderId(raw.provider),
    base_url: String(raw.base_url || ''),
    api_key: String(raw.api_key || ''),
    model: String(raw.model || ''),
    temperature: formatFloatForInput(raw.temperature, 0.7),
    timeout_s: raw.timeout_s == null ? '120' : String(raw.timeout_s),
    retry: raw.retry == null ? '1' : String(raw.retry),
    max_rounds: raw.max_rounds == null ? '1000' : String(raw.max_rounds),
    max_output: raw.max_output == null ? '' : String(raw.max_output),
    max_context: raw.max_context == null ? '' : String(raw.max_context),
    support_vision: raw.support_vision === true,
    support_hearing: raw.support_hearing === true,
    stream_include_usage: raw.stream_include_usage !== false,
    tool_call_mode: normalizeToolCallMode(raw.tool_call_mode, raw.provider),
    history_compaction_ratio: formatFloatForInput(raw.history_compaction_ratio, 0.8),
    history_compaction_reset: normalizeHistoryCompactionReset(raw.history_compaction_reset),
    raw: { ...raw }
  }));

const buildSelectedModelPreference = (row: ModelRow | null): SelectedModelPreference | null => {
  if (!row) return null;
  const key = row.key.trim();
  if (!key) return null;
  return {
    key,
    modelType: normalizeModelType(row.model_type)
  };
};

const resolveSelectedModelUid = (
  rows: ModelRow[],
  preference: SelectedModelPreference | null | undefined
): string => {
  const preferredKey = String(preference?.key || '').trim();
  if (!preferredKey) return '';
  const preferredType = normalizeModelType(preference?.modelType);
  return (
    rows.find(
      (item) => item.key.trim() === preferredKey && normalizeModelType(item.model_type) === preferredType
    )?.uid || rows.find((item) => item.key.trim() === preferredKey)?.uid || ''
  );
};

const ensureSelectedModel = (preference?: SelectedModelPreference | null) => {
  if (!modelRows.value.length) {
    selectedModelUid.value = '';
    return;
  }
  const matchedUid = resolveSelectedModelUid(modelRows.value, preference);
  if (matchedUid) {
    selectedModelUid.value = matchedUid;
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
    provider: DEFAULT_PROVIDER_ID,
    base_url: '',
    api_key: '',
    model: '',
    temperature: modelType === 'llm' ? '0.7' : '',
    timeout_s: '120',
    retry: '1',
    max_rounds: modelType === 'llm' ? '1000' : '',
    max_output: '',
    max_context: '',
    support_vision: false,
    support_hearing: false,
    stream_include_usage: true,
    tool_call_mode: resolveDefaultToolCallMode(DEFAULT_PROVIDER_ID),
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

const setCurrentAsDefault = async () => {
  const current = selectedModel.value;
  if (!current) return;
  if (savingModel.value) return;
  const key = current.key.trim();
  if (!key) {
    ElMessage.warning(t('desktop.system.modelKeyRequired'));
    return;
  }
  const previousDefaultModel = defaultModel.value;
  const previousDefaultEmbeddingModel = defaultEmbeddingModel.value;
  const modelType = normalizeModelType(current.model_type);
  if (modelType === 'embedding') {
    defaultEmbeddingModel.value = key;
  } else {
    defaultModel.value = key;
  }
  const saved = await saveModelSettings();
  if (!saved) {
    defaultModel.value = previousDefaultModel;
    defaultEmbeddingModel.value = previousDefaultEmbeddingModel;
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
      output[key] = roundFloat(parsed);
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
  setText('provider', normalizeProviderId(row.provider));
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
    output.support_hearing = row.support_hearing === true;
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
    delete output.support_hearing;
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
      provider: normalizeProviderId(current.provider),
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

const formatPythonCandidateSource = (source: string): string => {
  const normalized = String(source || '').trim();
  if (!normalized) return '-';
  return t(`desktop.system.pythonInterpreterSource.${normalized}`);
};

const resolvePythonPickerInitialPath = (): string | undefined => {
  const value = pythonInterpreterPath.value.trim();
  if (!value) {
    return undefined;
  }
  const separatorIndex = Math.max(value.lastIndexOf('/'), value.lastIndexOf('\\'));
  if (separatorIndex <= 0) {
    return undefined;
  }
  return value.slice(0, separatorIndex);
};

const normalizeLineList = (value: string): string[] =>
  String(value || '')
    .split(/\r?\n/)
    .map((item) => item.trim())
    .filter((item) => item.length > 0);

const formatLineList = (items: string[] | undefined): string =>
  Array.isArray(items)
    ? items
        .map((item) => String(item || '').trim())
        .filter((item) => item.length > 0)
        .join('\n')
    : '';

const parsePositiveInt = (value: string, fallback: number): number => {
  const parsed = Number.parseInt(String(value || '').trim(), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
};

const applyLanSettings = (lanMesh: DesktopLanMeshSettings | Record<string, any> | undefined) => {
  const data = (lanMesh || {}) as Record<string, any>;
  lanMeshEnabled.value = data.enabled === true;
  lanPeerId.value = String(data.peer_id || '').trim();
  lanDisplayName.value = String(data.display_name || '').trim();
  lanListenHost.value = String(data.listen_host || '0.0.0.0').trim() || '0.0.0.0';
  lanListenPort.value = String(data.listen_port ?? 18661);
  lanDiscoveryPort.value = String(data.discovery_port ?? 18662);
  lanAllowSubnetsText.value = formatLineList(data.allow_subnets as string[]);
  lanDenySubnetsText.value = formatLineList(data.deny_subnets as string[]);
  lanPeerBlacklistText.value = formatLineList(data.peer_blacklist as string[]);
  lanSharedSecret.value = String(data.shared_secret || '');
};

const applySettingsData = (
  data: Record<string, any>,
  preferredSelection: SelectedModelPreference | null = buildSelectedModelPreference(selectedModel.value)
) => {
  const llm = data.llm || {};
  modelRows.value = parseModelRows((llm.models as Record<string, Record<string, unknown>>) || {});
  modelRows.value.forEach((row) => applyModelPresetContext(row, false));
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

  ensureSelectedModel(preferredSelection);
  pythonInterpreterPath.value = String(data.python_interpreter_path || '').trim();
  remoteServerBaseUrl.value = String(data.remote_gateway?.server_base_url || '').trim();
  applyLanSettings(data.lan_mesh as DesktopLanMeshSettings | undefined);
  refreshRemoteConnected();
};

const loadPythonInterpreterCandidates = async (notifyWhenEmpty = false) => {
  loadingPythonCandidates.value = true;
  try {
    const response = await detectDesktopPythonInterpreters();
    const items = (response?.data?.data?.items || []) as DesktopPythonInterpreterItem[];
    pythonInterpreterCandidates.value = Array.isArray(items)
      ? items
          .map((item) => ({
            path: String(item.path || '').trim(),
            source: String(item.source || '').trim()
          }))
          .filter((item) => item.path)
      : [];
    if (!pythonInterpreterCandidates.value.length && notifyWhenEmpty) {
      ElMessage.info(t('desktop.system.pythonInterpreterDetectNone'));
    }
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.system.pythonInterpreterDetectFailed'));
  } finally {
    loadingPythonCandidates.value = false;
  }
};

const useDetectedPythonInterpreter = (path: string) => {
  pythonInterpreterPath.value = String(path || '').trim();
  ElMessage.success(t('desktop.system.pythonInterpreterSelected'));
};

const loadPythonPickerDirectory = async (path?: string) => {
  pythonPathPickerLoading.value = true;
  try {
    const response = await listDesktopDirectories(path, {
      includeFiles: true,
      fileNames: PYTHON_PICKER_FILE_NAMES
    });
    const data = (response?.data?.data || {}) as Record<string, unknown>;
    pythonPathPickerCurrentPath.value = String(data.current_path || '').trim();
    pythonPathPickerParentPath.value = data.parent_path ? String(data.parent_path) : null;
    pythonPathPickerRoots.value = Array.isArray(data.roots)
      ? data.roots.map((item) => String(item || '').trim()).filter(Boolean)
      : [];
    pythonPathPickerItems.value = Array.isArray(data.items)
      ? (data.items as unknown[])
          .map((item) => item as Record<string, unknown>)
          .map((item) => ({
            name: String(item.name || '').trim(),
            path: String(item.path || '').trim(),
            entry_type: (item.entry_type === 'file' ? 'file' : 'dir') as 'file' | 'dir'
          }))
          .filter((item) => item.name && item.path)
      : [];
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.system.pythonPathPickerLoadFailed'));
  } finally {
    pythonPathPickerLoading.value = false;
  }
};

const openPythonPathPicker = async () => {
  pythonPathPickerVisible.value = true;
  await loadPythonPickerDirectory(resolvePythonPickerInitialPath());
};

const handlePythonPathPickerSelect = async (item: DesktopDirectoryEntry) => {
  if (item.entry_type === 'file') {
    pythonInterpreterPath.value = String(item.path || '').trim();
    pythonPathPickerVisible.value = false;
    return;
  }
  await loadPythonPickerDirectory(item.path);
};

const saveRuntimeSettings = async () => {
  savingRuntime.value = true;
  try {
    const preferredSelection = buildSelectedModelPreference(selectedModel.value);
    const response = await updateDesktopSettings({
      python_interpreter_path: pythonInterpreterPath.value.trim()
    });
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data, preferredSelection);
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(resolveApiError(error, t('desktop.common.saveFailed')).message);
  } finally {
    savingRuntime.value = false;
  }
};

const resetPythonInterpreterPath = async () => {
  pythonInterpreterPath.value = '';
  await saveRuntimeSettings();
};

const refreshLanPeers = async () => {
  loadingLanPeers.value = true;
  try {
    const response = await listDesktopLanPeers();
    const items = (response?.data?.data?.items || []) as DesktopLanPeer[];
    lanPeers.value = Array.isArray(items) ? items : [];
  } catch (error) {
    console.error(error);
    lanPeers.value = [];
  } finally {
    loadingLanPeers.value = false;
  }
};

const loadSettings = async () => {
  loading.value = true;
  try {
    const preferredSelection = buildSelectedModelPreference(selectedModel.value);
    const response = await fetchDesktopSettings();
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data, preferredSelection);
    await refreshLanPeers();
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const saveLanSettings = async () => {
  savingLan.value = true;
  try {
    const preferredSelection = buildSelectedModelPreference(selectedModel.value);
    const payload = {
      lan_mesh: {
        enabled: lanMeshEnabled.value,
        peer_id: lanPeerId.value.trim(),
        display_name: lanDisplayName.value.trim(),
        listen_host: lanListenHost.value.trim() || '0.0.0.0',
        listen_port: parsePositiveInt(lanListenPort.value, 18661),
        discovery_port: parsePositiveInt(lanDiscoveryPort.value, 18662),
        discovery_interval_ms: 2500,
        peer_ttl_ms: 15000,
        allow_subnets: normalizeLineList(lanAllowSubnetsText.value),
        deny_subnets: normalizeLineList(lanDenySubnetsText.value),
        peer_blacklist: normalizeLineList(lanPeerBlacklistText.value),
        shared_secret: lanSharedSecret.value,
        max_inbound_dedup: 4096,
        relay_http_fallback: true,
        peer_ws_path: '/wunder/desktop/lan/ws',
        peer_http_path: '/wunder/desktop/lan/envelope'
      }
    };
    const response = await updateDesktopSettings(payload);
    const data = (response?.data?.data || {}) as Record<string, any>;
    applySettingsData(data, preferredSelection);
    await refreshLanPeers();
    ElMessage.success(t('desktop.common.saveSuccess'));
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
  } finally {
    savingLan.value = false;
  }
};

const saveModelSettings = async (): Promise<boolean> => {
  const models: Record<string, Record<string, unknown>> = {};
  const preferredSelection = buildSelectedModelPreference(selectedModel.value);

  for (const row of modelRows.value) {
    const key = row.key.trim();
    if (!key) {
      ElMessage.warning(t('desktop.system.modelKeyRequired'));
      return false;
    }
    if (models[key]) {
      ElMessage.warning(t('desktop.system.modelKeyDuplicate', { key }));
      return false;
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
    return false;
  }
  if (!models[currentDefaultModel]) {
    ElMessage.warning(t('desktop.system.defaultModelMissing'));
    return false;
  }

  const defaultModelConfig = models[currentDefaultModel] || {};
  const defaultBaseUrl = String(defaultModelConfig.base_url || '').trim();
  const defaultModelName = String(defaultModelConfig.model || '').trim();
  if (!defaultBaseUrl || !defaultModelName) {
    ElMessage.warning(t('desktop.system.defaultModelConfigRequired'));
    return false;
  }

  const currentDefaultEmbedding = findDefaultModelKeyByType(
    modelRows.value,
    'embedding',
    defaultEmbeddingModel.value.trim()
  );
  if (embeddingModelRows.value.length > 0 && !currentDefaultEmbedding) {
    ElMessage.warning(t('desktop.system.defaultEmbeddingModelRequired'));
    return false;
  }
  if (currentDefaultEmbedding && !models[currentDefaultEmbedding]) {
    ElMessage.warning(t('desktop.system.defaultEmbeddingModelMissing'));
    return false;
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
    applySettingsData(data, preferredSelection);
    ElMessage.success(t('desktop.common.saveSuccess'));
    return true;
  } catch (error) {
    console.error(error);
    ElMessage.error(t('desktop.common.saveFailed'));
    return false;
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
  void loadPythonInterpreterCandidates();
});
</script>

<style scoped>
.desktop-system-settings-panel {
  display: grid;
  gap: 14px;
  min-height: 0;
}

.desktop-system-settings-panel--llm {
  height: 100%;
  min-height: 0;
  grid-template-rows: auto minmax(0, 1fr);
}

.desktop-system-settings-panel--stack {
  height: 100%;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  padding-right: 2px;
}

.desktop-system-settings-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
}

.desktop-system-settings-status-line {
  display: flex;
  align-items: center;
  justify-content: flex-end;
}

.desktop-system-settings-layout {
  display: grid;
  grid-template-columns: minmax(220px, 300px) minmax(0, 1fr);
  gap: 12px;
  min-height: 0;
  height: 100%;
}

.desktop-system-settings-model-list-wrap {
  border: 1px solid #d8dee8;
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  overflow: hidden;
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
  border: 1px solid #d8dee8;
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
  display: flex;
  flex-direction: column;
  gap: 0;
  min-width: 0;
  min-height: 0;
  border: 1px solid #d8dee8;
  border-radius: 12px;
  background: var(--portal-panel);
  padding: 12px;
  overflow-y: auto;
  overflow-x: hidden;
}

.desktop-system-settings-detail-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  flex-wrap: wrap;
  padding-bottom: 12px;
  border-bottom: 1px solid #e4e9f0;
}

.desktop-system-settings-group {
  display: grid;
  gap: 10px;
  padding-top: 12px;
  margin-top: 12px;
  border-top: 1px solid #e4e9f0;
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

.desktop-system-settings-runtime-hint {
  font-size: 12px;
  font-weight: 600;
  color: var(--portal-text);
}

.desktop-system-settings-runtime-input {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  gap: 8px;
  align-items: center;
}

.desktop-system-settings-runtime-subhint {
  font-size: 12px;
  color: var(--portal-muted);
  line-height: 1.6;
}

.desktop-system-settings-runtime-candidates {
  display: grid;
  gap: 8px;
}

.desktop-system-settings-runtime-candidates-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--portal-text);
}

.desktop-system-settings-runtime-candidate {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 10px;
  align-items: center;
  border: 1px solid #d8dee8;
  border-radius: 10px;
  padding: 8px 10px;
  background: var(--portal-surface);
}

.desktop-system-settings-runtime-candidate-main {
  min-width: 0;
  display: grid;
  gap: 4px;
}

.desktop-system-settings-runtime-candidate-path {
  font-size: 12px;
  color: var(--portal-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-system-settings-runtime-candidate-meta {
  font-size: 12px;
  color: var(--portal-muted);
}

.desktop-system-settings-path-picker {
  display: grid;
  gap: 12px;
}

.desktop-system-settings-path-picker-toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
}

.desktop-system-settings-path-picker-current {
  font-size: 12px;
  color: var(--portal-text);
  border: 1px solid var(--portal-border);
  background: var(--portal-surface);
  border-radius: 10px;
  padding: 8px 10px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-system-settings-path-picker-roots {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.desktop-system-settings-path-picker-root {
  border: 1px solid var(--portal-border);
  background: var(--portal-surface);
  color: var(--portal-text);
  border-radius: 999px;
  padding: 5px 10px;
  font-size: 12px;
  cursor: pointer;
}

.desktop-system-settings-path-picker-list {
  border: 1px solid var(--portal-border);
  background: var(--portal-surface);
  border-radius: 12px;
  max-height: 320px;
  overflow: auto;
  padding: 8px;
  display: grid;
  gap: 6px;
}

.desktop-system-settings-path-picker-item {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid transparent;
  border-radius: 8px;
  background: transparent;
  color: var(--portal-text);
  font-size: 12px;
  padding: 8px 10px;
  cursor: pointer;
  text-align: left;
}

.desktop-system-settings-path-picker-item:hover,
.desktop-system-settings-path-picker-root:hover {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
  background: var(--ui-accent-soft-2);
  color: var(--ui-accent-deep);
}

.desktop-system-settings-path-picker-empty {
  font-size: 12px;
  color: var(--portal-muted);
  padding: 12px 4px;
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
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
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
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--portal-text);
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.desktop-system-settings-model-item-star {
  color: #f59e0b;
  font-size: 11px;
  flex-shrink: 0;
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

.desktop-system-settings-panel :deep(.el-textarea__inner) {
  background: #ffffff;
  border: 1px solid #d8dce4;
  border-radius: 10px;
  min-height: 64px;
  transition: border-color 0.15s ease, box-shadow 0.15s ease, background-color 0.15s ease;
}

.desktop-system-settings-panel :deep(.el-textarea__inner:hover) {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
}

.desktop-system-settings-panel :deep(.el-textarea__inner:focus) {
  border-color: rgba(var(--ui-accent-rgb), 0.62);
  box-shadow: 0 0 0 2px rgba(var(--ui-accent-rgb), 0.14);
}

.desktop-system-settings-panel :deep(.el-input__inner),
.desktop-system-settings-panel :deep(.el-select__selected-item),
.desktop-system-settings-panel :deep(.el-select__placeholder) {
  color: #374151;
  outline: none !important;
  box-shadow: none !important;
}

.desktop-system-settings-panel :deep(input:focus),
.desktop-system-settings-panel :deep(input:focus-visible),
.desktop-system-settings-panel :deep(textarea:focus),
.desktop-system-settings-panel :deep(textarea:focus-visible),
.desktop-system-settings-panel :deep(select:focus),
.desktop-system-settings-panel :deep(select:focus-visible) {
  outline: none !important;
}

.desktop-system-settings-peer-list {
  max-height: min(42vh, 360px);
  overflow-y: auto;
  overflow-x: hidden;
  padding-right: 2px;
}

.desktop-system-settings-peer-list::-webkit-scrollbar {
  width: 8px;
}

.desktop-system-settings-peer-list::-webkit-scrollbar-thumb {
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.45);
}

.desktop-system-settings-peer-list::-webkit-scrollbar-track {
  background: transparent;
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
}

@media (max-width: 900px) {
  .desktop-system-settings-form-grid,
  .desktop-system-settings-model-grid {
    grid-template-columns: 1fr;
  }

  .desktop-system-settings-inline {
    grid-template-columns: 1fr;
  }

  .desktop-system-settings-runtime-input,
  .desktop-system-settings-runtime-candidate {
    grid-template-columns: 1fr;
  }
}
</style>
